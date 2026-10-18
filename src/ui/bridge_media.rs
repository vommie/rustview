use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::bridge_inspector::BridgeInspector;
use super::bridge_state::{LastActivePanel, UiState};
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tab_grid::BridgeTabGrid;
use super::bridge_tags::{BridgeTags, TagActionContext};
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::browse_folders::BrowseFolders;
use super::browse_window::{
    DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, GRID_MARGIN_ROWS, TABLE_MARGIN_ROWS, ViewGeometry, apply_record, margin_rows_for,
    sync_window, timed, update_tile,
};
use super::fallback_view::{FallbackEvaluationParams, FallbackViewEvaluator};
use super::list_loader;
use super::models::rgb8_to_shared_buffer;
use super::thumbnail_regeneration::ThumbnailRegeneration;
use super::video_preview_animator::VideoPreviewAnimator;
use super::virtual_folder_wording;
use super::MainWindow;
use crate::library::models::{FolderId, MediaId, MediaRecord};
use crate::library::Library;
use crate::media::metadata::MetadataExtractor;
use crate::media::{MediaFormat, MediaHasher, MediaType};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{FolderDir, ListLayout, ListRequest, MediaStore, MediaTypeFilter, ScopeMode, SortColumn, SortOrder, lay_out};
use crate::settings::{LogObscurer, SettingsManager};
use crate::thumbnails::{
    PipelineItem, ThumbnailConfig, ThumbnailGenerator, ThumbnailLocation, ThumbnailManager, VideoPreviewRecord,
};
use crate::viewer::{HighResImageLoader, ViewerTabManager};
use parking_lot::RwLock;
use rayon::prelude::*;
use slint::{ComponentHandle, Image, Timer};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

thread_local! {
    static ACTIVE_PREVIEW_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
    static ACTIVE_HOVER_MEDIA_ID: RefCell<Option<u64>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaReloadOptions {
    pub start_pipeline: bool,
    pub reload_lmdb: bool,
    pub anchor_index: Option<usize>,
}

impl MediaReloadOptions {
    pub fn new(start_pipeline: bool, reload_lmdb: bool) -> Self {
        Self {
            start_pipeline,
            reload_lmdb,
            anchor_index: None,
        }
    }

    pub fn with_anchor(start_pipeline: bool, reload_lmdb: bool, anchor_index: usize) -> Self {
        Self {
            start_pipeline,
            reload_lmdb,
            anchor_index: Some(anchor_index),
        }
    }
}

struct ProcessedRefreshedItem {
    media_id: MediaId,
    old_hash: crate::media::FileHash,
    new_hash: crate::media::FileHash,
    updated_record: MediaRecord,
    primary_loc: Option<ThumbnailLocation>,
    preview_rec: Option<VideoPreviewRecord>,
    primary_pixels: Vec<u8>,
    primary_width: u32,
    primary_height: u32,
    tags_count: usize,
}

struct RefreshedMetadataOnlyItem {
    media_id: MediaId,
    updated_record: MediaRecord,
    tags_count: usize,
}

pub struct BridgeMedia;

impl BridgeMedia {
    pub fn register_query_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        tab_manager: Arc<ViewerTabManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_search_query_changed(move |query| {
            tracing::info!(
                query = %LogObscurer::query(&query),
                "[SearchFilter: QueryChanged] User modified search filter query"
            );
            st.write().last_active_panel = LastActivePanel::Media;
            st.write().filter.search_query = query.to_string();
            Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_regex_mode_toggled(move |enabled| {
            tracing::info!(
                enabled,
                "[SearchFilter: RegexToggle] Regular expression filter mode toggled"
            );
            st.write().last_active_panel = LastActivePanel::Media;
            st.write().filter.is_regex = enabled;
            Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
        });

        ThumbnailRegeneration::register_callbacks(
            window,
            thumbnails.clone(),
            settings.clone(),
            state.clone(),
        );

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_virtual_folder_toggled(move |_active| {
            st.write().last_active_panel = LastActivePanel::Media;
            Self::toggle_virtual_folder_mode(&win, &lib, &thumb, &tm, &set, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_media_type_filter_changed(move |idx| {
            let new_filter = MediaTypeFilter::from_index(idx);
            tracing::info!(
                media_type = ?new_filter,
                "[MediaTypeFilter: Changed] Media type view filter updated"
            );

            {
                let mut s = st.write();
                s.last_active_panel = LastActivePanel::Media;
                s.filter.media_type_filter = new_filter;
            }

            {
                let mut app_settings = set.get();
                app_settings.view.media_type_filter = new_filter.as_str().to_string();
                set.save_quiet_async(&app_settings);
            }

            if st.read().is_query_active {
                BridgeTags::execute_query_flow(&TagActionContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                });
            } else {
                Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
            }
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_sort_column_clicked(move |col_idx| {
            let target_col = SortColumn::from_index(col_idx);
            let (new_col, new_ord) = {
                let mut s = st.write();
                s.last_active_panel = LastActivePanel::Media;
                if s.sort_column == target_col {
                    s.sort_order = s.sort_order.toggle();
                } else {
                    s.sort_column = target_col;
                    s.sort_order = SortOrder::Ascending;
                }
                (s.sort_column, s.sort_order)
            };

            {
                let mut app_settings = set.get();
                app_settings.view.sort_column = new_col.as_str().to_string();
                app_settings.view.sort_order = new_ord.as_str().to_string();
                set.save_quiet(&app_settings);
            }

            win.set_sort_column_idx(new_col.to_index());
            win.set_sort_order_idx(new_ord.to_index());

            Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
        });

        let win = window.clone_strong();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();

        let win_hover = win.clone_strong();
        window.on_media_hover_changed(move |id, is_hover| {
            let media_id = id as u64;
            if !set.get().appearance.enable_hover_previews {
                return;
            }

            if is_hover {
                ACTIVE_HOVER_MEDIA_ID.with(|active_id| *active_id.borrow_mut() = Some(media_id));
                ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                let (format, full_path_opt) = {
                    let st_r = st.read();
                    if let Some(item) = st_r.all_folder_media.find(media_id) {
                        (item.record.format, Some(item.full_path()))
                    } else {
                        (MediaFormat::Unknown, None)
                    }
                };

                let win_w = win_hover.as_weak();
                let st_c = st.clone();

                if format == MediaFormat::Gif
                    && let Some(file_path) = full_path_opt
                {
                    if let Some(preview) = thumb.try_get_animated_preview(media_id) {
                        let timed_frames = VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                        let timer = VideoPreviewAnimator::start_variable_delay_animation(timed_frames, move |img| {
                            if let Some(w) = win_w.upgrade() {
                                update_tile(&w, &st_c.read(), media_id, |row| row.thumbnail = img);
                            }
                        });
                        ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                    } else {
                        let config = ThumbnailConfig::from_performance_settings(&set.get().performance);
                        let win_w_async = win_hover.as_weak();
                        let st_async = st.clone();

                        thumb.get_or_load_animated_preview_async(
                            media_id,
                            file_path,
                            config,
                            move |res| {
                                if let Ok(preview) = res {
                                    let _ = win_w_async.upgrade_in_event_loop(move |w| {
                                        let is_still_hovered = ACTIVE_HOVER_MEDIA_ID.with(|id| *id.borrow() == Some(media_id));
                                        if is_still_hovered {
                                            let timed_frames = VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                                            let timer = VideoPreviewAnimator::start_variable_delay_animation(timed_frames, move |img| {
                                                update_tile(&w, &st_async.read(), media_id, |row| row.thumbnail = img);
                                            });
                                            ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                                        }
                                    });
                                }
                            },
                        );
                    }
                } else if let Some(images) = VideoPreviewAnimator::load_preview_images(&thumb, media_id) {
                    let timer = VideoPreviewAnimator::start_animation(images, move |img| {
                        if let Some(w) = win_w.upgrade() {
                            update_tile(&w, &st_c.read(), media_id, |row| row.thumbnail = img);
                        }
                    });
                    ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                }
            } else {
                ACTIVE_HOVER_MEDIA_ID.with(|active_id| *active_id.borrow_mut() = None);
                ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                let st_r = st.read();
                if let Some(buf) = st_r.thumbs.get(media_id) {
                    update_tile(&win_hover, &st_r, media_id, |row| row.thumbnail = Image::from_rgb8(buf));
                }
            }
        });

        let thumb_stream = thumbnails.clone();
        let st_stream = state.clone();
        let win_stream = window.clone_strong();
        window.on_viewport_changed(move |first_row, visible_rows, columns, total| {
            Self::handle_viewport_changed(
                first_row.max(0) as usize,
                visible_rows.max(1) as usize,
                columns.max(1) as usize,
                total.max(0) as usize,
                &win_stream,
                &st_stream,
                &thumb_stream,
            );
        });

        let last_resize_time = Arc::new(parking_lot::Mutex::new(Instant::now()));
        let resize_events_count = Arc::new(AtomicUsize::new(0));

        let last_time_clone = last_resize_time;
        let count_clone = resize_events_count;

        window.on_column_resized(move |_col_idx, _new_width| {
            let _count = count_clone.fetch_add(1, Ordering::Relaxed);
            let mut last_log = last_time_clone.lock();
            if last_log.elapsed().as_millis() >= 100 {
                *last_log = Instant::now();
                count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_res = settings.clone();
        window.on_column_resize_finished(move |col_idx, final_width| {
            let width_f32 = final_width;
            let mut app_settings = set_res.get();

            match col_idx {
                0 => app_settings.view.col_filename_width = width_f32,
                1 => app_settings.view.col_path_width = width_f32,
                2 => app_settings.view.col_modified_date_width = width_f32,
                3 => app_settings.view.col_created_date_width = width_f32,
                4 => app_settings.view.col_size_width = width_f32,
                5 => app_settings.view.col_rating_width = width_f32,
                6 => app_settings.view.col_tags_width = width_f32,
                7 => app_settings.view.col_width_width = width_f32,
                8 => app_settings.view.col_height_width = width_f32,
                9 => app_settings.view.col_ar_width = width_f32,
                10 => app_settings.view.col_duration_width = width_f32,
                11 => app_settings.view.col_format_width = width_f32,
                12 => app_settings.view.col_flags_width = width_f32,
                _ => {}
            }

            set_res.save_quiet(&app_settings);
        });

        let set_mode = settings;
        let win_mode = window.clone_strong();
        let thumb_mode = thumbnails;
        let st_mode = state;

        window.on_view_mode_toggled_with_index(move |is_table, top_item_idx| {
            st_mode.write().last_active_panel = LastActivePanel::Media;
            Self::handle_view_mode_toggled(
                is_table,
                top_item_idx as usize,
                &win_mode,
                &thumb_mode,
                &set_mode,
                &st_mode,
            );
        });
    }

    pub fn update_virtual_folder_button_state(
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
    ) {
        let st = state.read();

        let is_virtual = st.is_virtual_folder_mode;
        let (is_enabled, disabled_reason): (bool, Option<&str>) = if st.is_query_active && st.filter.scope_mode == ScopeMode::Global {
            (false, Some("Recursive folder view is disabled during global query"))
        } else if st.is_query_active {
            (false, Some("Recursive folder view is disabled while query filter is active"))
        } else if let Some(folder_id) = st.current_folder_id {
            let has_subfolders = library
                .folders()
                .get_descendant_folder_ids(folder_id)
                .map(|ids| ids.len() > 1)
                .unwrap_or(false);

            if has_subfolders {
                (true, None)
            } else {
                (false, Some("No subfolders available in current folder"))
            }
        } else {
            (false, Some("No folder selected"))
        };

        drop(st);

        window.set_is_virtual_folder_btn_enabled(is_enabled);
        window.set_virtual_folder_tooltip(virtual_folder_wording::tooltip(is_virtual, disabled_reason).into());
        // The New Folder button depends on the same view state (no query, no recursive view, a folder that exists).
        BrowseFolders::update_new_folder_button(window, state);
    }

    pub fn toggle_virtual_folder_mode(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if Self::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(window, "Cannot switch view mode: another operation is in progress.");
            return;
        }

        let t0 = Instant::now();
        thumbnails.cancel_pipeline();

        let (current_f_id, current_p_opt, new_mode) = {
            let mut st = state.write();

            if let Some(prev_flag) = st.active_folder_cancel_flag.take() {
                prev_flag.store(true, Ordering::Release);
            }
            let new_flag = Arc::new(AtomicBool::new(false));
            st.active_folder_cancel_flag = Some(new_flag);

            st.active_folder_token = st.active_folder_token.wrapping_add(1);
            st.is_virtual_folder_mode = !st.is_virtual_folder_mode;
            st.active_indices.clear();
            st.mark_list_bytes_dirty();
            st.file_selection.clear();
            st.invalidate_view_window();

            (st.current_folder_id, st.current_folder_path.clone(), st.is_virtual_folder_mode)
        };

        window.set_is_virtual_folder(new_mode);

        if let (Some(folder_id), Some(path)) = (current_f_id, current_p_opt) {
            tracing::info!(
                folder_id,
                path = %LogObscurer::path(&path),
                is_virtual = new_mode,
                "[BridgeMedia: VirtualFolder] Toggled virtual recursive folder mode"
            );
            if state.read().is_query_active {
                BridgeTags::execute_query_flow(&TagActionContext {
                    window,
                    library,
                    thumbnails,
                    tab_manager,
                    settings,
                    state,
                });
            } else {
                let target_folder_ids = if new_mode {
                    library.folders().get_descendant_folder_ids(folder_id).unwrap_or_else(|_| vec![folder_id])
                } else {
                    vec![folder_id]
                };

                let virtual_count = target_folder_ids.len();
                {
                    let mut st = state.write();
                    st.virtual_folder_count = virtual_count;
                }
                StatusBarBridge::update_virtual_folder(window, new_mode, virtual_count);

                let mut media_ids = Vec::new();
                if let Ok(rtxn) = library.raw_db().env.read_txn() {
                    for fid in &target_folder_ids {
                        if let Ok(Some(ids)) = library.raw_db().media_by_folder.get(&rtxn, fid) {
                            media_ids.extend(ids);
                        }
                    }
                }

                let mut known_media = Vec::with_capacity(media_ids.len());
                let mut seen_ids = HashSet::new();
                if let Ok(rtxn) = library.raw_db().env.read_txn() {
                    for id in media_ids {
                        if seen_ids.insert(id)
                            && let Ok(Some(rec)) = library.raw_db().media.get(&rtxn, &id)
                        {
                            known_media.push(rec);
                        }
                    }
                }

                let indexed_media = if new_mode {
                    BridgeMedia::preindex_records(library, known_media)
                } else {
                    BridgeMedia::preindex(library, known_media, &path)
                };

                {
                    let mut st = state.write();
                    st.all_folder_media.replace(indexed_media);
                    st.file_selection.clear();
                }

                window.invoke_scroll_media_to_top();
                Self::reload(window, library, thumbnails, settings, state, false, false);
                BridgeInspector::update(window, state);
                BridgeTags::sync_tag_selection(window, library, state);
                Self::update_virtual_folder_button_state(window, library, state);
                StatusBarBridge::update_metrics(window, state);

                let folder_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                if new_mode {
                    StatusBarBridge::info(
                        window,
                        format!(
                            "Recursive view enabled: Showing {} folder(s) under '{}' ({}ms)",
                            virtual_count, folder_name, t0.elapsed().as_millis()
                        ),
                    );
                } else {
                    StatusBarBridge::info(
                        window,
                        format!(
                            "Recursive view disabled: Showing '{}' only ({}ms)",
                            folder_name, t0.elapsed().as_millis()
                        ),
                    );
                }
            }
        }
    }

    pub fn is_system_busy(
        window: &MainWindow,
        thumbnails: &ThumbnailManager,
        state: &RwLock<UiState>,
    ) -> bool {
        if let Some(st) = state.try_read()
            && (st.is_refreshing_media || st.is_refreshing_folders)
        {
            return true;
        }

        if window.get_is_indexing_media() || window.get_is_refreshing_folders() {
            return true;
        }

        let progress = thumbnails.progress();
        progress.is_preloading || progress.is_generating || progress.active_workers > 0
    }

    pub fn handle_view_mode_toggled(
        is_table: bool,
        anchor_item_idx: usize,
        window: &MainWindow,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t0 = Instant::now();

        {
            let mut app_settings = settings.get();
            app_settings.view.is_table_view = is_table;
            settings.save_quiet(&app_settings);
        }

        {
            let mut st = state.write();
            st.reset_view_window();
            st.scroll_bench.is_table_view.store(is_table, Ordering::Relaxed);
        }

        window.set_is_table_view(is_table);
        window.invoke_scroll_media_to_index(anchor_item_idx as i32, is_table);

        Self::refill_view(window, thumbnails, state, anchor_item_idx);

        let elapsed_us = t0.elapsed().as_micros();
        tracing::info!(
            target_view = if is_table { "TableView" } else { "GridView" },
            anchor_index = anchor_item_idx,
            transition_time_us = elapsed_us,
            "[ViewTransition: Sync] View mode transition completed with scroll synchronization"
        );
    }

    /// The viewport of the grid or the table moved or changed size: keeps the tiles around it filled.
    pub fn handle_viewport_changed(
        first_row: usize,
        visible_rows: usize,
        columns: usize,
        total: usize,
        window: &MainWindow,
        state: &RwLock<UiState>,
        thumbnails: &ThumbnailManager,
    ) {
        if total == 0 {
            return;
        }

        let is_table = window.get_is_table_view();
        let margin = if is_table { TABLE_MARGIN_ROWS } else { GRID_MARGIN_ROWS };
        let geometry = ViewGeometry::new(columns, visible_rows, margin);

        let mut st = state.write();
        let scroll_bench = st.scroll_bench.clone();
        scroll_bench.record_event();
        scroll_bench.is_table_view.store(is_table, Ordering::Relaxed);

        let unchanged = st.view_geometry == Some(geometry)
            && st.ring.range() == geometry.window(first_row, total)
            && st.ring.pool() == geometry.pool_size(total);
        if unchanged {
            scroll_bench.record_hysteresis_hit();
            return;
        }

        let (outcome, slice_us) = timed(|| sync_window(window, &mut st, thumbnails, geometry, first_row, total, false));
        scroll_bench.record_slice_reload(slice_us);
        tracing::trace!(
            first_row,
            rows_written = outcome.rows_written,
            rebuilt = outcome.rebuilt,
            requested = outcome.requested,
            elapsed_us = slice_us,
            "[BrowseWindow: Sync] Tiles updated for the viewport"
        );
    }

    pub fn execute_metadata_only_refresh(
        targets: Vec<(MediaId, PathBuf, MediaRecord)>,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
        operation_desc: &str,
    ) {
        let total_items = targets.len();
        if total_items == 0 {
            return;
        }

        if Self::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(
                window,
                "Cannot start refresh: another operation is already in progress.",
            );
            return;
        }

        {
            let mut st = state.write();
            st.is_refreshing_media = true;
        }
        window.set_is_system_busy(true);

        StatusBarBridge::update_media_index_progress(window, true, 0, total_items);
        StatusBarBridge::info(
            window,
            format!("Refreshing metadata for {}...", operation_desc),
        );

        let w_weak = window.as_weak();
        let lib_arc = library.clone();
        let thumb_arc = thumbnails.clone();
        let st_arc = state.clone();
        let op_desc_owned = operation_desc.to_string();

        std::thread::Builder::new()
            .name("media-metadata-fast-refresher".into())
            .spawn(move || {
                let t_start = Instant::now();
                let progress_counter = Arc::new(AtomicUsize::new(0));
                let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));
                let w_weak_progress = w_weak.clone();

                let progress_cb = Arc::new(move |cur: usize, tot: usize| {
                    let mut last = last_progress_report.lock();
                    if last.elapsed().as_millis() >= 35 || cur == tot {
                        *last = Instant::now();
                        let _ = w_weak_progress.upgrade_in_event_loop(move |w| {
                            StatusBarBridge::update_media_index_progress(&w, true, cur, tot);
                        });
                    }
                });

                let progress_counter_ref = progress_counter.clone();
                let progress_cb_ref = progress_cb.clone();
                let lib_ref = lib_arc.clone();

                let processed_results: Vec<Result<RefreshedMetadataOnlyItem, (MediaId, PathBuf, String)>> = targets
                    .into_par_iter()
                    .map(|(media_id, file_path, old_record)| {
                        if !file_path.exists() {
                            tracing::warn!(
                                media_id,
                                path = %LogObscurer::path(&file_path),
                                "[BridgeMedia: MetadataRefresh] File missing on disk"
                            );
                            return Err((media_id, file_path, "File not found on disk".to_string()));
                        }

                        let fs_meta = match std::fs::metadata(&file_path) {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::error!(
                                    media_id,
                                    path = %LogObscurer::path(&file_path),
                                    error = %e,
                                    "[BridgeMedia: MetadataRefresh] Failed to read metadata"
                                );
                                return Err((media_id, file_path, e.to_string()));
                            }
                        };

                        let file_size = fs_meta.len();
                        let mtime = fs_meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(old_record.modification_time);

                        let ctime = fs_meta
                            .created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(old_record.creation_time);

                        let is_zero_byte = file_size == 0;

                        let (raw_meta, meta_ok) = if is_zero_byte {
                            (
                                crate::media::RawMetadata {
                                    file_size: 0,
                                    modification_time: mtime,
                                    creation_time: ctime,
                                    duration_ms: 0,
                                    width: 0,
                                    height: 0,
                                    media_type: old_record.format.media_type().unwrap_or(MediaType::Image),
                                    format: old_record.format,
                                },
                                false,
                            )
                        } else {
                            match MetadataExtractor::extract(&file_path) {
                                Ok(m) => (m, true),
                                Err(e) => {
                                    tracing::warn!(
                                        media_id,
                                        path = %LogObscurer::path(&file_path),
                                        error = %e,
                                        "[BridgeMedia: MetadataExtract] Extraction fallback"
                                    );
                                    (
                                        crate::media::RawMetadata {
                                            file_size,
                                            modification_time: mtime,
                                            creation_time: ctime,
                                            duration_ms: 0,
                                            width: 0,
                                            height: 0,
                                            media_type: old_record.format.media_type().unwrap_or(MediaType::Image),
                                            format: old_record.format,
                                        },
                                        false,
                                    )
                                }
                            }
                        };

                        let assigned_tags_count = lib_ref
                            .tags()
                            .get_tags_for_media(media_id)
                            .map(|t| t.len())
                            .unwrap_or(0);

                        let is_corrupted = is_zero_byte || !meta_ok || (raw_meta.width == 0 && raw_meta.height == 0);

                        let updated_record = MediaRecord {
                            id: media_id,
                            folder_id: old_record.folder_id,
                            filename: old_record.filename.clone(),
                            file_hash: old_record.file_hash,
                            file_size,
                            modification_time: mtime,
                            creation_time: ctime,
                            duration_ms: raw_meta.duration_ms,
                            media_type: raw_meta.media_type,
                            format: raw_meta.format,
                            width: raw_meta.width,
                            height: raw_meta.height,
                            rating: old_record.rating,
                            last_seen: 1,
                            is_deleted: false,
                            is_corrupted,
                            source: old_record.source.clone(),
                        };

                        let cur_count = progress_counter_ref.fetch_add(1, Ordering::Relaxed) + 1;
                        progress_cb_ref(cur_count, total_items);

                        Ok(RefreshedMetadataOnlyItem {
                            media_id,
                            updated_record,
                            tags_count: assigned_tags_count,
                        })
                    })
                    .collect();

                let mut successful_items = Vec::with_capacity(processed_results.len());
                let mut missing_files = Vec::new();

                for res in processed_results {
                    match res {
                        Ok(item) => successful_items.push(item),
                        Err((id, path, _err)) => {
                            let _ = lib_arc.media().set_deleted(id, true);
                            let fname = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            missing_files.push(MissingFileItem {
                                filename: fname,
                                path,
                            });
                        }
                    }
                }

                if !successful_items.is_empty()
                    && let Ok(mut wtxn) = lib_arc.raw_db().env.write_txn()
                {
                    let current_gen = lib_arc.raw_db().next_generation(&mut wtxn).unwrap_or(1);

                    for item in &successful_items {
                        let mut rec = item.updated_record.clone();
                        rec.last_seen = current_gen;
                        let _ = lib_arc.raw_db().media.put(&mut wtxn, &item.media_id, &rec);
                    }
                    let _ = wtxn.commit();
                }

                let refreshed_count = successful_items.len();
                let missing_count = missing_files.len();

                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    {
                        let mut st = st_arc.write();
                        st.is_refreshing_media = false;

                        for item in &successful_items {
                            st.mark_list_bytes_dirty();

                            if let Some(target_indexed) = st.all_folder_media.find_mut(item.media_id) {
                                target_indexed.set_record(item.updated_record.clone());
                                target_indexed.set_tags_count(item.tags_count);
                            }
                        }

                        for missing in &missing_files {
                            if let Some(target_indexed) = st.all_folder_media.iter_mut().find(|m| m.is_at(&missing.path))
                                && !target_indexed.is_pending
                            {
                                target_indexed.mark_deleted();
                            }
                        }
                    }

                    {
                        let st_read = st_arc.read();
                        for item in &successful_items {
                            if let Some(indexed) = st_read.record_by_id(item.media_id) {
                                update_tile(&w, &st_read, item.media_id, |view| apply_record(view, indexed));
                            }
                        }
                    }

                    StatusBarBridge::update_media_index_progress(&w, false, 0, 0);

                    if missing_count > 0 && total_items == 1 {
                        let missing_dialog = MissingFileDialogController::new();
                        missing_dialog.show(&w, "Refresh Metadata", missing_files);
                    } else if missing_count > 0 {
                        StatusBarBridge::warn(
                            &w,
                            format!(
                                "Refreshed metadata for {} file(s). ({} missing)",
                                refreshed_count, missing_count
                            ),
                        );
                    } else {
                        StatusBarBridge::success(
                            &w,
                            format!(
                                "Refreshed metadata for {} in {}ms.",
                                op_desc_owned,
                                t_start.elapsed().as_millis()
                            ),
                        );
                    }

                    BridgeInspector::update(&w, &st_arc);
                    BridgeTags::sync_tag_selection(&w, &lib_arc, &st_arc);
                    Self::update_virtual_folder_button_state(&w, &lib_arc, &st_arc);
                    let busy = Self::is_system_busy(&w, &thumb_arc, &st_arc);
                    w.set_is_system_busy(busy);
                    StatusBarBridge::update_metrics(&w, &st_arc);
                });
            })
            .expect("Failed to spawn parallel media metadata refresher thread");
    }

    pub fn refresh_selected_metadata_only(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if Self::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(
                window,
                "Cannot start refresh: another operation is already in progress.",
            );
            return;
        }

        let selected_ids = {
            let st = state.read();
            st.file_selection.selected_ids().clone()
        };

        if selected_ids.is_empty() {
            return;
        }

        let mut targets: Vec<(MediaId, PathBuf, MediaRecord)> = Vec::with_capacity(selected_ids.len());
        {
            let st = state.read();
            let folder_path_opt = st.current_folder_path.clone();

            for &id in &selected_ids {
                if let Some(item) = st.all_folder_media.find(id) {
                    targets.push((id, item.full_path(), item.record.clone()));
                } else if let Some(ref folder_path) = folder_path_opt
                    && let Ok(Some(record)) = library.media().get(id)
                {
                    targets.push((id, folder_path.join(&record.filename), record));
                }
            }
        }

        if targets.is_empty() {
            return;
        }

        let count = targets.len();
        let desc = if count == 1 {
            "1 selected file".to_string()
        } else {
            format!("{} selected files", count)
        };

        Self::execute_metadata_only_refresh(
            targets,
            window,
            library,
            thumbnails,
            state,
            &desc,
        );
    }

    pub fn refresh_selected_media(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        Self::execute_media_refresh(
            window,
            library,
            thumbnails,
            tab_manager,
            settings,
            state,
        );
    }

    fn execute_media_refresh(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if Self::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(
                window,
                "Cannot start refresh: another operation is already in progress.",
            );
            return;
        }

        let selected_ids = {
            let st = state.read();
            if window.get_active_view_mode() == 1 {
                st.active_tab_selection()
                    .unwrap_or_else(|| st.file_selection.selected_ids().clone())
            } else {
                st.file_selection.selected_ids().clone()
            }
        };

        if selected_ids.is_empty() {
            return;
        }

        let mut targets: Vec<(MediaId, PathBuf, MediaRecord)> = Vec::with_capacity(selected_ids.len());
        {
            let st = state.read();
            let folder_path_opt = st.current_folder_path.clone();

            for &id in &selected_ids {
                if let Some(item) = st.all_folder_media.find(id) {
                    targets.push((id, item.full_path(), item.record.clone()));
                } else if let Some(ref folder_path) = folder_path_opt
                    && let Ok(Some(record)) = library.media().get(id)
                {
                    targets.push((id, folder_path.join(&record.filename), record));
                }
            }
        }

        let total_items = targets.len();
        if total_items == 0 {
            return;
        }

        {
            let mut st = state.write();
            st.is_refreshing_media = true;
        }
        window.set_is_system_busy(true);

        StatusBarBridge::update_media_index_progress(window, true, 0, total_items);

        let is_tab_mode_initial = window.get_active_view_mode() == 1;
        let start_msg = if total_items == 1 {
            if is_tab_mode_initial {
                "Refreshing the file (Full)...".to_string()
            } else {
                "Refreshing the selected file (Full)...".to_string()
            }
        } else {
            format!("Refreshing {} selected files (Full)...", total_items)
        };
        StatusBarBridge::info(window, start_msg);

        let config = ThumbnailConfig::from_performance_settings(&settings.get().performance);

        let w_weak = window.as_weak();
        let lib_arc = library.clone();
        let thumb_arc = thumbnails.clone();
        let tab_mgr_arc = tab_manager.clone();
        let set_arc = settings.clone();
        let st_arc = state.clone();

        std::thread::Builder::new()
            .name("media-item-parallel-refresher".into())
            .spawn(move || {
                let t_start = Instant::now();
                let generator = Arc::new(ThumbnailGenerator::new());
                let store = thumb_arc.store();

                let progress_counter = Arc::new(AtomicUsize::new(0));
                let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));
                let w_weak_progress = w_weak.clone();

                let progress_cb = Arc::new(move |cur: usize, tot: usize| {
                    let mut last = last_progress_report.lock();
                    if last.elapsed().as_millis() >= 35 || cur == tot {
                        *last = Instant::now();
                        let _ = w_weak_progress.upgrade_in_event_loop(move |w| {
                            StatusBarBridge::update_media_index_progress(&w, true, cur, tot);
                        });
                    }
                });

                let progress_counter_ref = progress_counter.clone();
                let progress_cb_ref = progress_cb.clone();
                let lib_ref = lib_arc.clone();
                let gen_ref = generator.clone();
                let cfg_ref = config.clone();

                let processed_results: Vec<Result<ProcessedRefreshedItem, (MediaId, PathBuf, String)>> = targets
                    .into_par_iter()
                    .map(|(media_id, file_path, old_record)| {
                        if !file_path.exists() {
                            tracing::warn!(
                                media_id,
                                path = %LogObscurer::path(&file_path),
                                "[BridgeMedia: Refresh] File does not exist on disk"
                            );
                            return Err((media_id, file_path, "File not found on disk".to_string()));
                        }

                        let fs_meta = match std::fs::metadata(&file_path) {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::error!(
                                    media_id,
                                    path = %LogObscurer::path(&file_path),
                                    error = %e,
                                    "[BridgeMedia: Refresh] Failed to read metadata"
                                );
                                return Err((media_id, file_path, e.to_string()));
                            }
                        };

                        let file_size = fs_meta.len();
                        let mtime = fs_meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(old_record.modification_time);

                        let ctime = fs_meta
                            .created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(old_record.creation_time);

                        let is_zero_byte = file_size == 0;

                        let (new_hash, hash_ok) = if is_zero_byte {
                            (MediaHasher::hash_bytes(b""), true)
                        } else {
                            match MediaHasher::hash_file(&file_path) {
                                Ok(h) => (h, true),
                                Err(_) => (MediaHasher::hash_bytes(file_path.to_string_lossy().as_bytes()), false),
                            }
                        };

                        let format = MediaFormat::from_path(&file_path);
                        let is_video = format.media_type() == Some(MediaType::Video);

                        let item_cfg = &cfg_ref;

                        let (primary_frame_opt, preview_rec, orig_w, orig_h, duration_ms, gen_ok) = if is_zero_byte {
                            (None, None, 0, 0, 0, false)
                        } else if is_video {
                            match gen_ref.generate_video_thumbnails(&file_path, item_cfg) {
                                Ok(video_res) => {
                                    let primary = video_res.primary_frame;
                                    let mut preview_rec = None;

                                    if !video_res.preview_frames.is_empty() {
                                        let (p_format, p_quality) = if item_cfg.custom_preview_settings {
                                            (item_cfg.preview_format, item_cfg.preview_quality)
                                        } else {
                                            (item_cfg.format, item_cfg.quality)
                                        };

                                        let mut locations = Vec::with_capacity(video_res.preview_frames.len());
                                        for p_frame in &video_res.preview_frames {
                                            if let Ok(p_bytes) = gen_ref.compress(
                                                &p_frame.rgb_pixels,
                                                p_frame.width,
                                                p_frame.height,
                                                p_format,
                                                p_quality,
                                            ) && let Ok(p_loc) = store.append(&p_bytes)
                                            {
                                                locations.push(p_loc);
                                            }
                                        }
                                        if !locations.is_empty() {
                                            preview_rec = Some(VideoPreviewRecord {
                                                frame_count: locations.len() as u32,
                                                frame_locations: locations,
                                            });
                                        }
                                    }

                                    (Some(primary), preview_rec, video_res.orig_width, video_res.orig_height, video_res.duration_ms, true)
                                }
                                Err(e) => {
                                    tracing::warn!(media_id, error = %e, "[BridgeMedia: VideoThumbnail] Video extraction failed");
                                    (None, None, 0, 0, 0, false)
                                }
                            }
                        } else {
                            match gen_ref.generate_image_thumbnail(
                                &file_path,
                                item_cfg.max_bound,
                                item_cfg.scaling_algorithm,
                            ) {
                                Ok(frame) => (Some(frame.clone()), None, frame.orig_width, frame.orig_height, 0u64, true),
                                Err(e) => {
                                    tracing::warn!(media_id, error = %e, "[BridgeMedia: ImageThumbnail] Image generation failed");
                                    (None, None, 0, 0, 0, false)
                                }
                            }
                        };

                        let primary_loc = if let Some(ref primary_frame) = primary_frame_opt {
                            if let Ok(compressed_primary) = gen_ref.compress(
                                &primary_frame.rgb_pixels,
                                primary_frame.width,
                                primary_frame.height,
                                item_cfg.format,
                                item_cfg.quality,
                            ) {
                                store.append(&compressed_primary).ok()
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let assigned_tags_count = lib_ref
                            .tags()
                            .get_tags_for_media(media_id)
                            .map(|t| t.len())
                            .unwrap_or(0);

                        let is_corrupted = is_zero_byte || !hash_ok || !gen_ok || (orig_w == 0 && orig_h == 0);

                        let updated_record = MediaRecord {
                            id: media_id,
                            folder_id: old_record.folder_id,
                            filename: old_record.filename.clone(),
                            file_hash: new_hash,
                            file_size,
                            modification_time: mtime,
                            creation_time: ctime,
                            duration_ms,
                            media_type: if is_video { MediaType::Video } else { MediaType::Image },
                            format,
                            width: orig_w,
                            height: orig_h,
                            rating: old_record.rating,
                            last_seen: 1,
                            is_deleted: false,
                            is_corrupted,
                            source: old_record.source.clone(),
                        };

                        let cur_count = progress_counter_ref.fetch_add(1, Ordering::Relaxed) + 1;
                        progress_cb_ref(cur_count, total_items);

                        let (pixels, pw, ph) = if let Some(f) = primary_frame_opt {
                            (f.rgb_pixels, f.width, f.height)
                        } else {
                            (Vec::new(), 0, 0)
                        };

                        Ok(ProcessedRefreshedItem {
                            media_id,
                            old_hash: old_record.file_hash,
                            new_hash,
                            updated_record,
                            primary_loc,
                            preview_rec,
                            primary_pixels: pixels,
                            primary_width: pw,
                            primary_height: ph,
                            tags_count: assigned_tags_count,
                        })
                    })
                    .collect();

                let mut successful_items = Vec::with_capacity(processed_results.len());
                let mut missing_files = Vec::new();

                for res in processed_results {
                    match res {
                        Ok(item) => successful_items.push(item),
                        Err((id, path, _err)) => {
                            let rec_opt = lib_arc.media().get(id).ok().flatten();
                            let is_pending_stub = rec_opt.as_ref().map(|r| r.source.is_some() && (r.file_size == 0 || r.width == 0)).unwrap_or(false);

                            if !is_pending_stub {
                                let _ = lib_arc.media().set_deleted(id, true);
                            }

                            let fname = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            missing_files.push(MissingFileItem {
                                filename: fname,
                                path,
                            });
                        }
                    }
                }

                if !successful_items.is_empty()
                    && let Ok(mut wtxn) = lib_arc.raw_db().env.write_txn()
                {
                    let current_gen = lib_arc.raw_db().next_generation(&mut wtxn).unwrap_or(1);

                    for item in &successful_items {
                        if item.old_hash != item.new_hash {
                            let _ = lib_arc.raw_db().update_media_hash_index(
                                &mut wtxn,
                                item.media_id,
                                Some(&item.old_hash),
                                Some(&item.new_hash),
                            );
                        }

                        let mut rec = item.updated_record.clone();
                        rec.last_seen = current_gen;
                        let _ = lib_arc.raw_db().media.put(&mut wtxn, &item.media_id, &rec);

                        if let Some(loc) = item.primary_loc {
                            let _ = lib_arc.raw_db().thumbnails.put(&mut wtxn, &item.media_id, &loc);
                        } else {
                            let _ = lib_arc.raw_db().thumbnails.delete(&mut wtxn, &item.media_id);
                        }

                        if let Some(ref p_rec) = item.preview_rec {
                            let _ = lib_arc.raw_db().video_previews.put(&mut wtxn, &item.media_id, p_rec);
                        } else {
                            let _ = lib_arc.raw_db().video_previews.delete(&mut wtxn, &item.media_id);
                        }
                    }
                    let _ = wtxn.commit();
                }

                let refreshed_count = successful_items.len();
                let missing_count = missing_files.len();

                let mut image_tabs_to_reload: Vec<(MediaId, u64, PathBuf)> = Vec::new();
                for item in &successful_items {
                    if matches!(item.updated_record.media_type, MediaType::Video) {
                        continue;
                    }
                    let open_tabs = tab_mgr_arc.get_tabs_by_media_id(item.media_id);
                    for tab in open_tabs {
                        let full_file_path = lib_arc
                            .resolve_media_path(item.media_id)
                            .unwrap_or_else(|_| PathBuf::from(&item.updated_record.filename));
                        image_tabs_to_reload.push((item.media_id, tab.id, full_file_path));
                    }
                }

                let active_video_reload_path = successful_items
                    .iter()
                    .find_map(|item| {
                        if matches!(item.updated_record.media_type, MediaType::Video)
                            && tab_mgr_arc.get_active_media_id() == Some(item.media_id)
                        {
                            lib_arc.resolve_media_path(item.media_id).ok()
                        } else {
                            None
                        }
                    });

                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    {
                        let mut st = st_arc.write();
                        st.is_refreshing_media = false;

                        for item in &successful_items {
                            if item.primary_width > 0 && item.primary_height > 0 && !item.primary_pixels.is_empty() {
                                let shared_buf = rgb8_to_shared_buffer(
                                    &item.primary_pixels,
                                    item.primary_width,
                                    item.primary_height,
                                );

                                st.thumbs.insert(item.media_id, shared_buf.clone());
                            }

                            st.mark_list_bytes_dirty();

                            if let Some(target_indexed) = st.all_folder_media.find_mut(item.media_id) {
                                target_indexed.set_record(item.updated_record.clone());
                                target_indexed.set_tags_count(item.tags_count);
                            }

                            tab_mgr_arc.notify_media_refreshed(item.media_id, &item.updated_record);
                        }

                        for missing in &missing_files {
                            if let Some(target_indexed) = st.all_folder_media.iter_mut().find(|m| m.is_at(&missing.path))
                                && !target_indexed.is_pending
                            {
                                target_indexed.mark_deleted();
                            }
                        }
                    }

                    if let Some(ref v_path) = active_video_reload_path
                        && let Some(player) = tab_mgr_arc.get_active_player()
                    {
                        tracing::info!(
                            path = %LogObscurer::path(v_path),
                            "[BridgeMedia: VideoReload] Re-loading modified video file in MPV"
                        );
                        let _ = player.load_file(v_path);
                    }

                    for (media_id, tab_id, full_file_path) in image_tabs_to_reload {
                        let tm_async = tab_mgr_arc.clone();
                        let w_weak_tab = w.as_weak();
                        let lib_tab = lib_arc.clone();
                        let set_tab = set_arc.clone();
                        let st_tab = st_arc.clone();

                        rayon::spawn(move || {
                            let load_res = HighResImageLoader::load_full_resolution(&full_file_path);
                            let _ = w_weak_tab.upgrade_in_event_loop(move |w_ui| {
                                if tm_async.get_tab(tab_id).map(|t| t.media_id) != Some(media_id) {
                                    return;
                                }

                                match load_res {
                                    Ok((w_px, h_px, pixel_buffer)) => {
                                        tm_async.update_tab_image_buffer(tab_id, w_px, h_px, pixel_buffer);
                                    }
                                    Err(err) => {
                                        tm_async.set_tab_error(tab_id, err.to_string());
                                    }
                                }
                                BridgeViewerTabs::sync_ui(&w_ui, &tm_async, &lib_tab, &set_tab, &st_tab);
                            });
                        });
                    }

                    let is_table = w.get_is_table_view();

                    {
                        let st_read = st_arc.read();
                        for item in &successful_items {
                            let fresh_image = (!is_table).then(|| st_read.thumbs.get(item.media_id)).flatten();
                            let indexed = st_read.record_by_id(item.media_id);
                            update_tile(&w, &st_read, item.media_id, |view| {
                                if let Some(buf) = fresh_image {
                                    view.thumbnail = Image::from_rgb8(buf);
                                }
                                if let Some(indexed) = indexed {
                                    apply_record(view, indexed);
                                }
                            });
                        }
                    }

                    StatusBarBridge::update_media_index_progress(&w, false, 0, 0);

                    if missing_count > 0 && total_items == 1 {
                        let missing_dialog = MissingFileDialogController::new();
                        missing_dialog.show(&w, "Refresh File", missing_files);
                    } else if missing_count > 0 {
                        StatusBarBridge::warn(
                            &w,
                            format!(
                                "Refreshed {} file(s). ({} missing / pending)",
                                refreshed_count, missing_count
                            ),
                        );
                    } else {
                        let is_tab_view = w.get_active_view_mode() == 1;
                        let success_msg = if total_items == 1 {
                            if is_tab_view {
                                "Refreshed the file.".to_string()
                            } else {
                                "Refreshed the selected file.".to_string()
                            }
                        } else {
                            format!(
                                "Refreshed {} of {} selected files in {}ms.",
                                refreshed_count,
                                total_items,
                                t_start.elapsed().as_millis()
                            )
                        };
                        StatusBarBridge::success(&w, success_msg);
                    }

                    BridgeInspector::update(&w, &st_arc);
                    BridgeTags::sync_tag_selection(&w, &lib_arc, &st_arc);
                    Self::update_virtual_folder_button_state(&w, &lib_arc, &st_arc);
                    BridgeViewerTabs::sync_ui(&w, &tab_mgr_arc, &lib_arc, &set_arc, &st_arc);
                    if w.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(&w, &tab_mgr_arc, &lib_arc, &set_arc, &st_arc);
                    }
                    let busy = Self::is_system_busy(&w, &thumb_arc, &st_arc);
                    w.set_is_system_busy(busy);
                    StatusBarBridge::update_metrics(&w, &st_arc);
                });
            })
            .expect("Failed to spawn parallel media item refresher thread");
    }

    /// Drops records whose id was already seen (a record can be listed by more than one scope).
    fn unique_by_id(raw_records: Vec<MediaRecord>) -> Vec<MediaRecord> {
        let mut seen = HashSet::with_capacity(raw_records.len());
        raw_records.into_iter().filter(|r| seen.insert(r.id)).collect()
    }

    fn tag_counts(library: &Library, records: &[MediaRecord]) -> HashMap<MediaId, usize> {
        let Ok(rtxn) = library.raw_db().env.read_txn() else {
            return HashMap::new();
        };
        records
            .iter()
            .filter_map(|r| match library.raw_db().tags_by_media.get(&rtxn, &r.id) {
                Ok(Some(tags)) => Some((r.id, tags.len())),
                _ => None,
            })
            .collect()
    }

    pub fn preindex(library: &Library, raw_records: Vec<MediaRecord>, folder_path: &Path) -> Vec<IndexedMediaRecord> {
        let folder = FolderDir::new(folder_path);
        let records = Self::unique_by_id(raw_records);
        let tag_counts = Self::tag_counts(library, &records);

        records
            .into_par_iter()
            .enumerate()
            .map(|(index, r)| {
                let tags_count = tag_counts.get(&r.id).copied().unwrap_or(0);
                IndexedMediaRecord::new(index, r, folder.clone(), tags_count)
            })
            .collect()
    }

    pub fn preindex_records(library: &Library, raw_records: Vec<MediaRecord>) -> Vec<IndexedMediaRecord> {
        let records = Self::unique_by_id(raw_records);
        let tag_counts = Self::tag_counts(library, &records);
        let mut folders: HashMap<FolderId, Arc<FolderDir>> = HashMap::new();

        records
            .into_iter()
            .enumerate()
            .map(|(index, r)| {
                let folder = folders
                    .entry(r.folder_id)
                    .or_insert_with(|| {
                        let path = library.folders().get_folder(r.folder_id).ok().flatten().map(|f| PathBuf::from(f.path));
                        FolderDir::new(path.unwrap_or_default())
                    })
                    .clone();
                let tags_count = tag_counts.get(&r.id).copied().unwrap_or(0);
                IndexedMediaRecord::new(index, r, folder, tags_count)
            })
            .collect()
    }

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

    /// The items of the folder that can have a thumbnail (not deleted, not a pending stub), without their paths.
    pub fn pipeline_items(records: &[IndexedMediaRecord]) -> Vec<PipelineItem> {
        records
            .iter()
            .filter(|item| !item.is_deleted && !item.is_pending)
            .map(|item| PipelineItem {
                media_id: item.record.id,
                folder_id: item.record.folder_id,
                is_video: matches!(item.record.media_type, MediaType::Video),
            })
            .collect()
    }

    /// Refills the tile pool for the list that is already laid out (view mode switch); nothing is filtered or sorted again.
    fn refill_view(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>, anchor_index: usize) {
        let is_table = window.get_is_table_view();
        let total = usize::try_from(window.get_total_items_count()).unwrap_or(0);
        let mut st = state.write();
        st.invalidate_view_window();
        let geometry = st
            .view_geometry
            .unwrap_or_else(|| ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table)));
        let outcome = sync_window(window, &mut st, thumbnails, geometry, anchor_index / geometry.columns, total, true);
        tracing::debug!(rows_written = outcome.rows_written, requested = outcome.requested, "[BrowseWindow: ViewMode] Tile pool refilled for the new view mode");
    }

    /// Shows an empty list at once (a large folder is loading): the tiles of the previous list must not stay on screen and clickable
    /// while the records behind them are gone.
    pub(super) fn show_empty_list(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>) {
        let is_table = window.get_is_table_view();
        let mut st = state.write();
        st.media_row_map.clear();
        st.active_indices.clear();
        st.invalidate_view_window();
        let geometry = st
            .view_geometry
            .unwrap_or_else(|| ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table)));
        window.set_total_items_count(0);
        sync_window(window, &mut st, thumbnails, geometry, 0, 0, true);
    }

    /// The inputs of a layout (filter, sort, refinement) as the state holds them now.
    pub(super) fn capture_request(state: &RwLock<UiState>) -> (ListRequest, String) {
        let st = state.read();
        let current_query = st.filter.search_query.trim().to_string();
        let last_query = &st.last_executed_query;
        let is_refinement = !current_query.is_empty() && !last_query.is_empty() && current_query.starts_with(last_query.as_str());
        let request = ListRequest {
            filter: st.filter.clone(),
            sort_column: st.sort_column,
            sort_order: st.sort_order,
            refine_from: (is_refinement && !st.active_indices.is_empty()).then(|| st.active_indices.clone()),
        };
        (request, current_query)
    }

    /// How many records the folder (or, for the recursive view, its whole subtree) holds; a cheap index read.
    pub(super) fn folder_record_count(library: &Library, folder_id: FolderId, is_virtual_folder: bool) -> usize {
        let folder_ids = if is_virtual_folder {
            library.folders().get_descendant_folder_ids(folder_id).unwrap_or_else(|_| vec![folder_id])
        } else {
            vec![folder_id]
        };
        let Ok(rtxn) = library.raw_db().env.read_txn() else { return 0 };
        folder_ids
            .iter()
            .filter_map(|fid| library.raw_db().media_by_folder.get(&rtxn, fid).ok().flatten())
            .map(|ids| ids.len())
            .sum()
    }

    /// The indexed records of a folder (or of its whole subtree), read from the library; no UI access, safe on any thread.
    pub(super) fn fetch_folder_records(library: &Library, folder_id: FolderId, folder_path: &Path, is_virtual_folder: bool) -> Vec<IndexedMediaRecord> {
        if is_virtual_folder {
            let descendant_ids = library.folders().get_descendant_folder_ids(folder_id).unwrap_or_else(|_| vec![folder_id]);
            let mut known = Vec::new();
            let mut seen = HashSet::new();
            if let Ok(rtxn) = library.raw_db().env.read_txn() {
                for fid in descendant_ids {
                    let Ok(Some(ids)) = library.raw_db().media_by_folder.get(&rtxn, &fid) else { continue };
                    for mid in ids {
                        if seen.insert(mid)
                            && let Ok(Some(rec)) = library.raw_db().media.get(&rtxn, &mid)
                        {
                            known.push(rec);
                        }
                    }
                }
            }
            Self::preindex_records(library, known)
        } else {
            let raw_media = library.media().get_all_by_folder(folder_id).unwrap_or_default();
            Self::preindex(library, raw_media, folder_path)
        }
    }

    /// Loads the records of the open folder from the library into the state (the sync path of a folder reload).
    fn load_current_folder_records(library: &Library, state: &RwLock<UiState>) {
        let (folder_id, path_opt, is_virtual_folder, is_query_active) = {
            let s = state.read();
            (s.current_folder_id, s.current_folder_path.clone(), s.is_virtual_folder_mode, s.is_query_active)
        };
        let (Some(f_id), Some(p)) = (folder_id, path_opt) else { return };
        if is_query_active {
            return;
        }
        let indexed = Self::fetch_folder_records(library, f_id, &p, is_virtual_folder);
        state.write().all_folder_media.replace(indexed);
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
        Self::commit_layout(window, library, thumbnails, settings, state, PendingList { store: None, layout, query: current_query }, options);
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
        tracing::debug!(job, records = len, "[ListLoader: Relayout] Large list; laying it out on the worker thread");
        let started = Instant::now();

        let weak = window.as_weak();
        let (library, thumbnails, settings, state) = (library.clone(), thumbnails.clone(), settings.clone(), state.clone());
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
                    tracing::debug!(job, "[ListLoader: Relayout] The records changed while the list was laid out; laying out again");
                    Self::reload_with_options(&window, &library, &thumbnails, &settings, &state, options);
                    return;
                }
                let matched = layout.indices.len();
                let layout_ms = started.elapsed().as_secs_f64() * 1000.0;
                let t_commit = Instant::now();
                let pending = PendingList { store: None, layout, query: current_query };
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
    pub(super) fn update_fallback_view(window: &MainWindow, app_settings: &crate::settings::AppSettings, state: &RwLock<UiState>) {
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
        let root_exists =
            total_in_dataset > 0 || app_settings.general.root_folder.as_ref().map(|p| p.is_dir()).unwrap_or(false);
        let current_folder_name = path_opt.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str());

        let fallback_state = FallbackViewEvaluator::evaluate(FallbackEvaluationParams {
            total_in_dataset,
            matched_count,
            is_query_active,
            filter: &filter,
            has_root,
            root_exists,
            current_folder_name,
        });

        window.set_empty_state_active(fallback_state.is_active);
        window.set_empty_state_title(fallback_state.title.into());
        window.set_empty_state_description(fallback_state.description.into());
        window.set_empty_state_icon(crate::icons::to_slint(fallback_state.icon));
    }

    /// Swaps a finished layout into the state and refills the tile pool; O(tile pool + selection), whatever the list size.
    pub(super) fn commit_layout(
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
                st_write.file_selection.prune_where(|id| layout.row_map.contains_key(id));
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

            let geometry = st_write
                .view_geometry
                .unwrap_or_else(|| ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table)));
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
            bench.slint_commit_time_us.store(t_slint_start.elapsed().as_micros() as u64, Ordering::Relaxed);
        }

        if selection_pruned {
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
        }

        Self::update_virtual_folder_button_state(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        if options.start_pipeline {
            let st_read = state.read();
            if let Some(f_id) = st_read.current_folder_id {
                let items = Self::pipeline_items(&st_read.all_folder_media);
                if !items.is_empty() {
                    let config = ThumbnailConfig::from_performance_settings(&settings.get().performance);
                    thumbnails.start_pipeline(f_id, items, config);
                }
            }
        }
    }
}

/// A finished layout waiting to be committed; `store` carries freshly loaded records (folder switch) built off the UI thread.
pub struct PendingList {
    pub store: Option<MediaStore>,
    pub layout: ListLayout,
    pub query: String,
}
