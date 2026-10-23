//! Reloading and re-laying out the Browse list: synchronous and deferred (worker) variants and the commit of a finished layout.

use super::{BridgeMedia, MediaReloadOptions, PendingList};
use crate::ui::MainWindow;
use crate::ui::browse_folders::BrowseFolders;
use crate::ui::browse_window::{
    DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, ViewGeometry, margin_rows_for, sync_window,
};
use crate::ui::list_loader;
use crate::ui::media::fallback_view::{FallbackEvaluationParams, FallbackViewEvaluator};
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::media::FormatPolicy;
use rustview_core::query::{ListRequest, lay_out};
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::{ThumbnailConfig, ThumbnailManager};
use slint::ComponentHandle;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

impl BridgeMedia {
    pub fn reload(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        start_pipeline: bool,
        reload_lmdb: bool,
    ) {
        Self::reload_with_options(
            window,
            library,
            thumbnails,
            settings,
            state,
            MediaReloadOptions::new(start_pipeline, reload_lmdb),
        );
    }

    /// Refills the tile pool for the list that is already laid out (view mode switch); nothing is filtered or sorted again.
    pub(super) fn refill_view(
        window: &MainWindow,
        thumbnails: &ThumbnailManager,
        state: &RwLock<UiState>,
        anchor_index: usize,
    ) {
        let is_table = window.get_is_table_view();
        let total = usize::try_from(window.get_total_items_count()).unwrap_or(0);
        let mut st = state.write();
        st.invalidate_view_window();
        let geometry = st.view_geometry.unwrap_or_else(|| {
            ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table))
        });
        let outcome = sync_window(
            window,
            &mut st,
            thumbnails,
            geometry,
            anchor_index / geometry.columns,
            total,
            true,
        );
        tracing::debug!(
            rows_written = outcome.rows_written,
            requested = outcome.requested,
            "[BrowseWindow: ViewMode] Tile pool refilled for the new view mode"
        );
    }

    /// Shows an empty list at once (a large folder is loading): the tiles of the previous list must not stay on screen and clickable
    /// while the records behind them are gone.
    pub(in crate::ui) fn show_empty_list(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>) {
        let is_table = window.get_is_table_view();
        let mut st = state.write();
        st.media_row_map.clear();
        st.active_indices.clear();
        st.invalidate_view_window();
        let geometry = st.view_geometry.unwrap_or_else(|| {
            ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table))
        });
        window.set_total_items_count(0);
        sync_window(window, &mut st, thumbnails, geometry, 0, 0, true);
    }

    /// The inputs of a layout (filter, sort, refinement) as the state holds them now.
    pub(in crate::ui) fn capture_request(state: &RwLock<UiState>) -> (ListRequest, String) {
        let st = state.read();
        let current_query = st.filter.search_query.trim().to_string();
        let last_query = &st.last_executed_query;
        let is_refinement =
            !current_query.is_empty() && !last_query.is_empty() && current_query.starts_with(last_query.as_str());
        let request = ListRequest {
            filter: st.filter.clone(),
            sort_column: st.sort_column,
            sort_order: st.sort_order,
            refine_from: (is_refinement && !st.active_indices.is_empty()).then(|| st.active_indices.clone()),
        };
        (request, current_query)
    }

    pub fn reload_with_options(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        options: MediaReloadOptions,
    ) {
        // A synchronous reload is the newest truth: a relayout that is still running must not overwrite it.
        list_loader::supersede_relayouts();

        if options.reload_lmdb {
            Self::load_current_folder_records(library, state);
        }

        let (request, current_query) = Self::capture_request(state);
        let layout = {
            let st = state.read();
            lay_out(library, &st.all_folder_media, &request)
        };
        Self::commit_layout(
            window,
            library,
            thumbnails,
            settings,
            state,
            PendingList {
                store: None,
                layout,
                query: current_query,
            },
            options,
        );
    }

    /// Like `reload_with_options` for a user action that changes only how the list is filtered or sorted: a large list is laid out on
    /// the worker thread against a snapshot while the UI keeps responding, and the result is swapped in when it is ready.
    pub fn reload_deferred(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        options: MediaReloadOptions,
    ) {
        let len = state.read().all_folder_media.len();
        if options.reload_lmdb || len <= list_loader::ASYNC_ABOVE_RECORDS {
            Self::reload_with_options(window, library, thumbnails, settings, state, options);
            return;
        }

        let (request, current_query) = Self::capture_request(state);
        let (snapshot, generation) = {
            let st = state.read();
            (st.all_folder_media.snapshot(), st.all_folder_media.generation())
        };
        let job = list_loader::begin_relayout();
        tracing::debug!(
            job,
            records = len,
            "[ListLoader: Relayout] Large list; laying it out on the worker thread"
        );
        let started = Instant::now();

        let weak = window.as_weak();
        let (library, thumbnails, settings, state) =
            (library.clone(), thumbnails.clone(), settings.clone(), state.clone());
        list_loader::worker().submit(move || {
            if !list_loader::relayout_is_current(job) {
                return;
            }
            let layout = lay_out(&library, &snapshot, &request);
            drop(snapshot);
            let _ = weak.upgrade_in_event_loop(move |window| {
                if !list_loader::relayout_is_current(job) {
                    return;
                }
                if state.read().all_folder_media.generation() != generation {
                    tracing::debug!(
                        job,
                        "[ListLoader: Relayout] The records changed while the list was laid out; laying out again"
                    );
                    Self::reload_with_options(&window, &library, &thumbnails, &settings, &state, options);
                    return;
                }
                let matched = layout.indices.len();
                let layout_ms = started.elapsed().as_secs_f64() * 1000.0;
                let t_commit = Instant::now();
                let pending = PendingList {
                    store: None,
                    layout,
                    query: current_query,
                };
                Self::commit_layout(&window, &library, &thumbnails, &settings, &state, pending, options);
                tracing::info!(
                    job,
                    matched,
                    layout_ms = format!("{layout_ms:.1}"),
                    commit_ms = format!("{:.1}", t_commit.elapsed().as_secs_f64() * 1000.0),
                    "[ListLoader: Relayout] List laid out on the worker thread and committed"
                );
            });
        });
    }

    /// Shows or hides the empty-state overlay of the Browse view for what the state holds now (memory only, except the check of
    /// the root folder when the folder holds nothing at all).
    pub(in crate::ui) fn update_fallback_view(
        window: &MainWindow,
        app_settings: &rustview_core::settings::AppSettings,
        state: &RwLock<UiState>,
    ) {
        let (filter, is_query_active, path_opt, total_in_dataset, matched_count) = {
            let st = state.read();
            (
                st.filter.clone(),
                st.is_query_active,
                st.current_folder_path.clone(),
                st.all_folder_media.len(),
                // Folder rows are content too: a folder that only holds subfolders is not "empty".
                st.active_indices.len() + st.folder_rows.listed_folders(),
            )
        };
        let has_root = app_settings.general.root_folder.is_some();
        // The root is only probed when the folder holds nothing at all; any record proves that the root was there.
        let root_exists = total_in_dataset > 0 || app_settings.general.root_folder.as_ref().is_some_and(|p| p.is_dir());
        let current_folder_name = path_opt.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str());

        let fallback_state = FallbackViewEvaluator::evaluate(FallbackEvaluationParams {
            total_in_dataset,
            matched_count,
            is_query_active,
            filter: &filter,
            has_root,
            root_exists,
            current_folder_name,
            formats_switched_off: !FormatPolicy::mask().is_all_enabled(),
        });

        window.set_empty_state_active(fallback_state.is_active);
        window.set_empty_state_title(fallback_state.title.into());
        window.set_empty_state_description(fallback_state.description.into());
        window.set_empty_state_icon(crate::icons::to_slint(fallback_state.icon));
    }

    /// Swaps a finished layout into the state and refills the tile pool; O(tile pool + selection), whatever the list size.
    pub(in crate::ui) fn commit_layout(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        pending: PendingList,
        options: MediaReloadOptions,
    ) {
        let PendingList { store, layout, query } = pending;
        // Folder dates are read again for the rows that show them (the old ones stay until the new ones arrive).
        state.read().folder_times.mark_stale();
        let bench_opt = state.read().active_folder_bench.clone();
        let matched_len = layout.indices.len();

        if let Some(ref bench) = bench_opt {
            bench.filter_time_us.store(layout.filter_us, Ordering::Relaxed);
            bench.items_matched.store(matched_len, Ordering::Relaxed);
            bench.sort_time_us.store(layout.sort_us, Ordering::Relaxed);
            bench.view_model_map_time_us.store(layout.map_us, Ordering::Relaxed);
        }

        let is_table = window.get_is_table_view();
        let t_slint_start = Instant::now();
        let mut selection_pruned = false;
        let folder_selection_before = state.read().folder_rows.selection_snapshot();
        let app_settings = settings.get();
        let folder_snapshot = BrowseFolders::snapshot_if_wanted(library, state);
        let folder_row_count;
        {
            let mut st_write = state.write();
            if let Some(store) = store {
                st_write.all_folder_media = store;
            }

            let prev_sel_count = st_write.file_selection.count();
            if prev_sel_count > 0 {
                st_write
                    .file_selection
                    .prune_where(|id| layout.row_map.contains_key(id));
                selection_pruned = st_write.file_selection.count() != prev_sel_count;
            }

            st_write.media_row_map = layout.row_map;
            st_write.active_indices = layout.indices;
            st_write.last_executed_query = query;
            st_write.set_visible_total_bytes(layout.total_bytes);
            st_write.invalidate_view_window();
            BrowseFolders::rebuild_rows(&mut st_write, folder_snapshot.as_deref(), &app_settings.folders);
            let view_len = st_write.view_len();
            folder_row_count = st_write.folder_rows.folder_rows();

            let geometry = st_write.view_geometry.unwrap_or_else(|| {
                ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table))
            });
            let first_row = options.anchor_index.unwrap_or(0) / geometry.columns;
            window.set_total_items_count(view_len as i32);
            let outcome = sync_window(window, &mut st_write, thumbnails, geometry, first_row, view_len, true);
            tracing::debug!(
                items = matched_len,
                folder_rows = folder_row_count,
                rows_written = outcome.rows_written,
                requested = outcome.requested,
                "[BrowseWindow: Reload] Tile pool refilled for the new list"
            );
        }

        Self::update_fallback_view(window, &app_settings, state);

        if let Some(ref bench) = bench_opt {
            bench
                .slint_commit_time_us
                .store(t_slint_start.elapsed().as_micros() as u64, Ordering::Relaxed);
        }

        // Folder rows the new list no longer shows (filter, hidden folders) took their highlight with them.
        let folders_pruned = state.read().folder_rows.selection_snapshot() != folder_selection_before;
        if selection_pruned || folders_pruned {
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
        }

        Self::update_virtual_folder_button_state(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        if options.start_pipeline {
            let st_read = state.read();
            if let Some(f_id) = st_read.current_folder_id {
                let items = Self::pipeline_items(&st_read.all_folder_media, false);
                if !items.is_empty() {
                    let config = ThumbnailConfig::from_performance_settings(&settings.get().performance);
                    thumbnails.start_pipeline(f_id, items, config);
                }
            }
        }
    }
}
