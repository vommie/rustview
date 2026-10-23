//! Querying by tags and ratings and restoring the folder view afterwards.

use super::TagTreeSync;
use super::{BridgeTags, TagActionContext};
use super::{QueryTimings, TagActionHandler};
use crate::ui::list_loader;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::media::{BridgeMedia, MediaReloadOptions, PendingList};
use crate::ui::models::rgb8_to_shared_buffer;
use crate::ui::status_bar::StatusBarBridge;
use rustview_core::library::models::{FolderId, MediaId, MediaRecord, MediaSearchParams};
use rustview_core::query::{MediaStore, ScopeMode, lay_out};
use rustview_core::thumbnails::ThumbnailConfig;
use slint::ComponentHandle;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

impl TagActionHandler {
    pub fn reset_query(ctx: &TagActionContext<'_>) {
        let (current_f_id, current_f_path) = {
            let mut st = ctx.state.write();
            st.selected_rating = 0;
            st.filter.rating = None;
            st.filter.tag_ids.clear();
            st.filter.search_query.clear();
            st.last_executed_query.clear();
            st.is_query_active = false;
            st.is_virtual_folder_mode = false;
            st.virtual_folder_count = 0;
            (st.current_folder_id, st.current_folder_path.clone())
        };

        ctx.window.set_is_virtual_folder(false);
        ctx.window.set_virtual_folder_count(0);
        ctx.window.set_selected_rating(0);
        ctx.window.set_search_query("".into());
        ctx.window.invoke_scroll_media_to_top();

        if let (Some(f_id), Some(path)) = (current_f_id, current_f_path.as_ref()) {
            let raw_media = ctx.library.media().get_all_by_folder(f_id).unwrap_or_default();
            let indexed = BridgeMedia::preindex(ctx.library, raw_media, path);
            let valid_ids: HashSet<MediaId> = indexed.iter().map(|m| m.record.id).collect();

            {
                let mut st = ctx.state.write();
                st.all_folder_media.replace(indexed);
                st.file_selection.prune_missing(&valid_ids);
            }
        }

        TagTreeSync::sync_in_place(ctx.window, ctx.library, ctx.state);
        BridgeMedia::reload(
            ctx.window,
            ctx.library,
            ctx.thumbnails,
            ctx.settings,
            ctx.state,
            false,
            false,
        );
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeMedia::update_virtual_folder_button_state(ctx.window, ctx.library, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        StatusBarBridge::info(ctx.window, "Query reset to default folder view.");
    }

    pub fn execute_query_flow(ctx: &TagActionContext<'_>) {
        let t_flow_start = Instant::now();

        ctx.window.set_search_query("".into());
        ctx.window.invoke_scroll_media_to_top();

        let (params, current_folder_p) = {
            let mut st = ctx.state.write();
            st.filter.search_query.clear();
            st.last_executed_query.clear();

            let is_rating_active = st.selected_rating > 0;
            st.filter.rating = if is_rating_active {
                Some(st.selected_rating)
            } else {
                None
            };

            let is_query = is_rating_active || !st.filter.tag_ids.is_empty();
            st.is_query_active = is_query;

            if st.filter.scope_mode == ScopeMode::Global && is_query {
                st.is_virtual_folder_mode = false;
                st.virtual_folder_count = 0;
            }

            let params = MediaSearchParams {
                rating: st.filter.rating,
                tag_ids: st.filter.tag_ids.clone(),
                match_mode: st.filter.match_mode,
                scope_mode: st.filter.scope_mode,
                folder_id: st.current_folder_id,
                media_type_filter: st.filter.media_type_filter,
            };

            (params, st.current_folder_path.clone())
        };

        if params.scope_mode == ScopeMode::Global {
            ctx.window.set_is_virtual_folder(false);
            ctx.window.set_virtual_folder_count(0);
        }

        let is_query_active = params.rating.is_some_and(|r| r > 0) || !params.tag_ids.is_empty();

        if !is_query_active {
            let t_restore = Instant::now();
            let is_virt = ctx.state.read().is_virtual_folder_mode;
            let restore_target = {
                let mut st = ctx.state.write();
                st.is_query_active = false;
                st.current_folder_id.zip(current_folder_p.clone())
            };

            if let Some((f_id, path)) = restore_target.as_ref()
                && BridgeMedia::folder_record_count(ctx.library, *f_id, is_virt) > list_loader::ASYNC_ABOVE_RECORDS
            {
                Self::restore_folder_deferred(ctx, *f_id, path.clone(), is_virt, t_flow_start, t_restore);
                return;
            }

            if let Some((f_id, path)) = restore_target.as_ref() {
                let indexed = BridgeMedia::fetch_folder_records(ctx.library, *f_id, path, is_virt);
                let valid_ids: HashSet<MediaId> = indexed.iter().map(|m| m.record.id).collect();
                let mut st = ctx.state.write();
                st.all_folder_media.replace(indexed);
                st.file_selection.prune_missing(&valid_ids);
            }

            TagTreeSync::sync_in_place(ctx.window, ctx.library, ctx.state);
            BridgeMedia::reload(
                ctx.window,
                ctx.library,
                ctx.thumbnails,
                ctx.settings,
                ctx.state,
                false,
                false,
            );
            Self::finish_restore(ctx, t_flow_start, t_restore);
            return;
        }

        let total_records = ctx
            .library
            .raw_db()
            .env
            .read_txn()
            .ok()
            .and_then(|rtxn| ctx.library.raw_db().media.len(&rtxn).ok())
            .map_or(0, rustview_core::util::cast::SaturatingAs::saturating_as::<usize>);
        if total_records > list_loader::ASYNC_ABOVE_RECORDS {
            Self::query_deferred(ctx, params, t_flow_start);
            return;
        }

        let t_db = Instant::now();
        let search_results = ctx.library.media().search(&params).unwrap_or_default();
        let db_search_us = t_db.elapsed().as_micros();

        let t_preindex = Instant::now();
        let indexed_results = BridgeMedia::preindex_records(ctx.library, search_results.clone());
        let preindex_us = t_preindex.elapsed().as_micros();

        let valid_ids: HashSet<MediaId> = indexed_results.iter().map(|m| m.record.id).collect();

        {
            let mut st = ctx.state.write();
            st.all_folder_media.replace(indexed_results);
            st.file_selection.prune_missing(&valid_ids);
        }

        let t_reload = Instant::now();
        TagTreeSync::sync_in_place(ctx.window, ctx.library, ctx.state);
        BridgeMedia::reload(
            ctx.window,
            ctx.library,
            ctx.thumbnails,
            ctx.settings,
            ctx.state,
            false,
            false,
        );
        Self::finish_query(
            ctx,
            &params,
            &search_results,
            &QueryTimings {
                t_flow_start,
                db_search_us,
                preindex_us,
                t_reload,
            },
        );
    }

    /// Everything that follows once a tag or rating query is shown: inspector, status and benchmark log, then the automatic
    /// completion of external stubs among the results.
    pub(in crate::ui::tags) fn finish_query(
        ctx: &TagActionContext<'_>,
        params: &MediaSearchParams,
        search_results: &[MediaRecord],
        timings: &QueryTimings,
    ) {
        let result_count = search_results.len();
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeMedia::update_virtual_folder_button_state(ctx.window, ctx.library, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        let reload_us = timings.t_reload.elapsed().as_micros();

        let total_flow_us = timings.t_flow_start.elapsed().as_micros();

        let scope_str = params.scope_mode.as_str();
        if result_count == 0 {
            StatusBarBridge::info(ctx.window, format!("Query ({scope_str}) returned no results."));
        } else {
            StatusBarBridge::info(
                ctx.window,
                format!(
                    "Query ({}) returned {} result{}.",
                    scope_str,
                    result_count,
                    if result_count == 1 { "" } else { "s" }
                ),
            );
        }

        tracing::info!(
            rating = ?params.rating,
            tags_count = params.tag_ids.len(),
            match_mode = ?params.match_mode,
            scope_mode = ?params.scope_mode,
            media_type_filter = ?params.media_type_filter,
            matched_count = result_count,
            db_search_us = timings.db_search_us,
            preindex_us = timings.preindex_us,
            view_reload_us = reload_us,
            total_flow_us = total_flow_us,
            "[Benchmark: QueryFlow] Query evaluation completed and rendered"
        );

        // Automatic External Stubs completion during Query execution
        if ctx.settings.get().api.auto_complete_external_stubs {
            let (ready_stubs, still_pending, sources) = ctx
                .library
                .external_media_completer()
                .find_pending_stubs_in_records(search_results);

            if !ready_stubs.is_empty() {
                let stubs_count = ready_stubs.len();
                tracing::info!(
                    stubs_count,
                    sources = ?sources,
                    still_pending,
                    "[QueryFlow: StubsDetected] Auto-completing external stubs matching active query in background..."
                );

                {
                    let mut st = ctx.state.write();
                    st.is_refreshing_media = true;
                }
                ctx.window.set_is_system_busy(true);

                // Activate ONLY ThumbnailPipelineProgress (prevent dual progress bar anomaly)
                ctx.thumbnails.tracker().set_generate_total(stubs_count);
                StatusBarBridge::info(ctx.window, format!("Auto-completing {stubs_count} external file(s)..."));

                let w_weak = ctx.window.as_weak();
                let lib_arc = ctx.library.clone();
                let thumb_arc = ctx.thumbnails.clone();
                let st_arc = ctx.state.clone();
                let set_arc = ctx.settings.clone();
                let config = ThumbnailConfig::from_performance_settings(&ctx.settings.get().performance);

                std::thread::Builder::new()
                    .name("query-stubs-completer".into())
                    .spawn(move || {
                        let w_weak_progress = w_weak.clone();
                        let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));
                        let tracker_ref = thumb_arc.tracker();

                        let progress_cb = Arc::new(move |cur: usize, tot: usize| {
                            tracker_ref.increment_generate_done();
                            let snap = tracker_ref.snapshot();
                            let mut last = last_progress_report.lock();
                            if last.elapsed().as_millis() >= 35 || cur == tot {
                                *last = Instant::now();
                                let _ = w_weak_progress.upgrade_in_event_loop(move |w| {
                                    StatusBarBridge::update_thumbnail_progress(&w, &snap);
                                });
                            }
                        });

                        let store = thumb_arc.store();
                        let (report, completed_items) = lib_arc.external_media_completer().complete_candidates(
                            ready_stubs,
                            &config,
                            store,
                            Some(move |cur, tot| progress_cb(cur, tot)),
                        );

                        let _ = w_weak.upgrade_in_event_loop(move |w| {
                            {
                                let mut st = st_arc.write();
                                st.is_refreshing_media = false;

                                for item in &completed_items {
                                    if item.primary_width > 0
                                        && item.primary_height > 0
                                        && !item.primary_pixels.is_empty()
                                    {
                                        let shared_buf = rgb8_to_shared_buffer(
                                            &item.primary_pixels,
                                            item.primary_width,
                                            item.primary_height,
                                        );
                                        st.thumbs.insert(item.media_id, shared_buf);
                                    }

                                    st.mark_list_bytes_dirty();

                                    if let Some(target_indexed) = st.all_folder_media.find_mut(item.media_id) {
                                        target_indexed.set_record(item.updated_record.clone());
                                    }
                                }
                            }

                            thumb_arc.tracker().reset();

                            BridgeMedia::reload(&w, &lib_arc, &thumb_arc, &set_arc, &st_arc, false, false);
                            BridgeInspector::update(&w, &st_arc);
                            BridgeTags::sync_tag_selection(&w, &lib_arc, &st_arc);
                            let busy = BridgeMedia::is_system_busy(&w, &thumb_arc, &st_arc);
                            w.set_is_system_busy(busy);
                            StatusBarBridge::update_metrics(&w, &st_arc);
                            StatusBarBridge::success(
                                &w,
                                format!("Auto-completed {} external media item(s).", report.completed_count),
                            );
                        });
                    })
                    .ok();
            }
        }
    }

    /// The status, inspector and log after the default folder view was restored (query cleared).
    pub(in crate::ui::tags) fn finish_restore(ctx: &TagActionContext<'_>, t_flow_start: Instant, t_restore: Instant) {
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeMedia::update_virtual_folder_button_state(ctx.window, ctx.library, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        StatusBarBridge::info(ctx.window, "Query cleared. Regular folder view active.");

        tracing::info!(
            restore_us = t_restore.elapsed().as_micros(),
            total_flow_us = t_flow_start.elapsed().as_micros(),
            "[QueryFlow: Inactive] Restored default folder dataset"
        );
    }

    /// The query runs over a large library: the view shows an empty list while a worker thread searches, indexes, filters and
    /// sorts the results; the UI thread only swaps them in. A newer folder selection or query supersedes it.
    pub(in crate::ui::tags) fn query_deferred(
        ctx: &TagActionContext<'_>,
        params: MediaSearchParams,
        t_flow_start: Instant,
    ) {
        let job = list_loader::begin_folder_load();
        tracing::info!(
            job,
            "[ListLoader: Query] Large library; running the query on the worker thread"
        );
        BridgeMedia::show_empty_list(ctx.window, ctx.thumbnails, ctx.state);
        StatusBarBridge::info(ctx.window, "Searching...");

        let weak = ctx.window.as_weak();
        let (library, thumbnails, tab_manager, settings, state) = (
            ctx.library.clone(),
            ctx.thumbnails.clone(),
            ctx.tab_manager.clone(),
            ctx.settings.clone(),
            ctx.state.clone(),
        );
        list_loader::worker().submit(move || {
            if !list_loader::folder_load_is_current(job) {
                return;
            }
            let t_db = Instant::now();
            let search_results = library.media().search(&params).unwrap_or_default();
            let db_search_us = t_db.elapsed().as_micros();

            let t_preindex = Instant::now();
            let store = MediaStore::from_records(BridgeMedia::preindex_records(&library, search_results.clone()));
            let preindex_us = t_preindex.elapsed().as_micros();
            if !list_loader::folder_load_is_current(job) {
                return;
            }
            let (mut request, query) = BridgeMedia::capture_request(&state);
            request.refine_from = None;
            let layout = lay_out(&library, &store, &request);

            let _ = weak.upgrade_in_event_loop(move |window| {
                if !list_loader::folder_load_is_current(job) {
                    return;
                }
                let t_reload = Instant::now();
                let pending = PendingList {
                    store: Some(store),
                    layout,
                    query,
                };
                BridgeMedia::commit_layout(
                    &window,
                    &library,
                    &thumbnails,
                    &settings,
                    &state,
                    pending,
                    MediaReloadOptions::new(false, false),
                );
                let ctx = TagActionContext {
                    window: &window,
                    library: &library,
                    thumbnails: &thumbnails,
                    tab_manager: &tab_manager,
                    settings: &settings,
                    state: &state,
                };
                TagTreeSync::sync_in_place(ctx.window, ctx.library, ctx.state);
                Self::finish_query(
                    &ctx,
                    &params,
                    &search_results,
                    &QueryTimings {
                        t_flow_start,
                        db_search_us,
                        preindex_us,
                        t_reload,
                    },
                );
            });
        });
    }

    /// Restoring a large folder after a query was cleared: the same worker path as a folder switch.
    pub(in crate::ui::tags) fn restore_folder_deferred(
        ctx: &TagActionContext<'_>,
        folder_id: FolderId,
        folder_path: PathBuf,
        is_virtual_folder: bool,
        t_flow_start: Instant,
        t_restore: Instant,
    ) {
        let job = list_loader::begin_folder_load();
        tracing::info!(
            job,
            folder_id,
            "[ListLoader: Query] Large folder; restoring it on the worker thread"
        );
        BridgeMedia::show_empty_list(ctx.window, ctx.thumbnails, ctx.state);
        StatusBarBridge::info(ctx.window, "Loading...");

        let weak = ctx.window.as_weak();
        let (library, thumbnails, tab_manager, settings, state) = (
            ctx.library.clone(),
            ctx.thumbnails.clone(),
            ctx.tab_manager.clone(),
            ctx.settings.clone(),
            ctx.state.clone(),
        );
        list_loader::worker().submit(move || {
            if !list_loader::folder_load_is_current(job) {
                return;
            }
            let store = MediaStore::from_records(BridgeMedia::fetch_folder_records(
                &library,
                folder_id,
                &folder_path,
                is_virtual_folder,
            ));
            if !list_loader::folder_load_is_current(job) {
                return;
            }
            let (mut request, query) = BridgeMedia::capture_request(&state);
            request.refine_from = None;
            let layout = lay_out(&library, &store, &request);

            let _ = weak.upgrade_in_event_loop(move |window| {
                if !list_loader::folder_load_is_current(job) {
                    return;
                }
                let pending = PendingList {
                    store: Some(store),
                    layout,
                    query,
                };
                BridgeMedia::commit_layout(
                    &window,
                    &library,
                    &thumbnails,
                    &settings,
                    &state,
                    pending,
                    MediaReloadOptions::new(false, false),
                );
                let ctx = TagActionContext {
                    window: &window,
                    library: &library,
                    thumbnails: &thumbnails,
                    tab_manager: &tab_manager,
                    settings: &settings,
                    state: &state,
                };
                TagTreeSync::sync_in_place(ctx.window, ctx.library, ctx.state);
                Self::finish_restore(&ctx, t_flow_start, t_restore);
            });
        });
    }
}
