//! Selecting a folder: loading its media list, immediately or on the worker for large folders, and the work after the list is ready.

use super::{BridgeFolders, FolderNavigationContext, FolderSelection, LoadedFolder};
use crate::ui::MainWindow;
use crate::ui::browse_folders::BrowseFolders;
use crate::ui::favorites::BridgeFavorites;
use crate::ui::list_loader;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::media::{BridgeMedia, MediaReloadOptions, PendingList};
use crate::ui::models::rgb8_to_shared_buffer;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::folders::normalize_path;
use rustview_core::library::models::{FolderId, MediaId, MediaRecord};
use rustview_core::library::refresh::RefreshReport;
use rustview_core::query::{MediaStore, lay_out};
use rustview_core::settings::SettingsManager;
use rustview_core::telemetry::FolderLoadBenchmark;
use rustview_core::thumbnails::{ThumbnailConfig, ThumbnailManager};
use rustview_core::util::cast::SaturatingAs;
use slint::ComponentHandle;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

impl BridgeFolders {
    pub fn select_folder(
        folder_id: FolderId,
        path: &PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        {
            let mut st = state.write();
            let from = st.current_folder_id;
            st.nav_history.record(from, folder_id);
        }
        Self::select_folder_internal(
            folder_id,
            path,
            &FolderNavigationContext {
                window,
                library,
                thumbnails,
                settings,
                state,
            },
            false,
        );
    }

    pub(super) fn select_folder_internal(
        folder_id: FolderId,
        path: &PathBuf,
        ctx: &FolderNavigationContext<'_>,
        is_explicit_refresh: bool,
    ) {
        let normalized_path = normalize_path(path);
        let folder_name = normalized_path.file_name().map_or_else(
            || normalized_path.to_string_lossy().to_string(),
            |n| n.to_string_lossy().to_string(),
        );

        ctx.thumbnails.cancel_pipeline();
        if !is_explicit_refresh {
            ctx.window.invoke_scroll_media_to_top();
        }

        let bench = Arc::new(FolderLoadBenchmark::new(
            folder_id,
            normalized_path.to_string_lossy().to_string(),
        ));

        let (request_token, cancel_flag, is_virtual_folder) = {
            let mut st = ctx.state.write();

            if let Some(prev_flag) = st.active_folder_cancel_flag.take() {
                prev_flag.store(true, Ordering::Release);
            }
            let new_flag = Arc::new(AtomicBool::new(false));
            st.active_folder_cancel_flag = Some(new_flag.clone());

            st.active_folder_bench = Some(bench.clone());
            st.active_folder_token = st.active_folder_token.wrapping_add(1);
            st.current_folder_id = Some(folder_id);
            st.current_folder_path = Some(normalized_path.clone());
            st.active_indices.clear();
            st.mark_list_bytes_dirty();
            st.all_folder_media.clear();
            // Opening a folder again puts its subfolders in name order and drops the highlight of a folder row.
            st.folder_rows.reset_tail();
            st.folder_rows.clear_selection();

            if !is_explicit_refresh {
                st.file_selection.clear();
                st.last_executed_query.clear();
                st.is_query_active = false;
                st.selected_rating = 0;
                st.filter.rating = None;
                st.filter.search_query.clear();
                st.filter.tag_ids.clear();
                st.tag_filter_query.clear();
                st.is_virtual_folder_mode = false;
                st.virtual_folder_count = 0;
            }

            (st.active_folder_token, new_flag, st.is_virtual_folder_mode)
        };

        {
            let mut s = ctx.settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(normalized_path.clone());
                ctx.settings.save_quiet_async(&s);
            }
        }

        if !is_explicit_refresh {
            ctx.window.set_search_query("".into());
            ctx.window.set_selected_rating(0);
            ctx.window.set_tag_filter_text("".into());
        }

        Self::reload_tree(ctx.window, ctx.library, ctx.settings, ctx.state);
        Self::scroll_to_selected(ctx.window);

        BridgeFavorites::set_selected_path(Some(&normalized_path));

        let target_folder_ids = if is_virtual_folder {
            ctx.library
                .folders()
                .get_descendant_folder_ids(folder_id)
                .unwrap_or_else(|_| vec![folder_id])
        } else {
            vec![folder_id]
        };

        let virtual_count = target_folder_ids.len();
        {
            let mut st = ctx.state.write();
            st.virtual_folder_count = virtual_count;
        }
        StatusBarBridge::update_virtual_folder(ctx.window, is_virtual_folder, virtual_count);

        let selection = FolderSelection {
            folder_id,
            normalized_path,
            folder_name,
            is_virtual_folder,
            virtual_count,
            target_folder_ids,
            cancel_flag,
            bench,
            is_explicit_refresh,
            request_token,
        };

        let media_ids = Self::fetch_media_ids(ctx.library, &selection);
        if media_ids.len() > list_loader::ASYNC_ABOVE_RECORDS {
            Self::load_folder_deferred(ctx, selection, media_ids);
            return;
        }

        let loaded = Self::load_folder_records(ctx.library, &selection, media_ids);
        let (is_empty_in_db, stub_candidates) = (loaded.is_empty_in_db, loaded.stub_candidates);
        {
            let mut s = ctx.state.write();
            s.all_folder_media.replace(loaded.indexed);
        }

        BridgeMedia::reload(
            ctx.window,
            ctx.library,
            ctx.thumbnails,
            ctx.settings,
            ctx.state,
            false,
            false,
        );
        Self::after_folder_list_ready(ctx, selection, is_empty_in_db, &stub_candidates);
    }

    /// The ids of the media of the selected folder(s); a cheap index read, it decides whether the list is built on the worker.
    pub(super) fn fetch_media_ids(library: &Library, selection: &FolderSelection) -> Vec<MediaId> {
        let t_id_start = Instant::now();
        let mut media_ids = Vec::new();
        if let Ok(rtxn) = library.raw_db().env.read_txn() {
            for fid in &selection.target_folder_ids {
                if let Ok(Some(ids)) = library.raw_db().media_by_folder.get(&rtxn, fid) {
                    media_ids.extend(ids);
                }
            }
        }
        selection.bench.lmdb_id_fetch_time_us.store(
            t_id_start.elapsed().as_micros().saturating_as::<u64>(),
            Ordering::Relaxed,
        );
        media_ids
    }

    /// Reads the records of `media_ids` and indexes them for the view (no UI access, safe on any thread).
    pub(super) fn load_folder_records(
        library: &Library,
        selection: &FolderSelection,
        media_ids: Vec<MediaId>,
    ) -> LoadedFolder {
        let bench = &selection.bench;
        let t_rec_start = Instant::now();
        let mut known_media = Vec::with_capacity(media_ids.len());
        let mut seen_ids = HashSet::with_capacity(media_ids.len());

        if let Ok(rtxn) = library.raw_db().env.read_txn() {
            for id in media_ids {
                if seen_ids.insert(id)
                    && let Ok(Some(rec)) = library.raw_db().media.get(&rtxn, &id)
                {
                    known_media.push(rec);
                }
            }
        }
        bench.lmdb_record_fetch_time_us.store(
            t_rec_start.elapsed().as_micros().saturating_as::<u64>(),
            Ordering::Relaxed,
        );
        bench.total_records_fetched.store(known_media.len(), Ordering::Relaxed);

        let is_empty_in_db = known_media.is_empty();
        let stub_candidates: Vec<MediaRecord> = known_media.iter().filter(|r| r.source.is_some()).cloned().collect();
        let t_pre = Instant::now();
        let indexed = if selection.is_virtual_folder {
            BridgeMedia::preindex_records(library, known_media)
        } else {
            BridgeMedia::preindex(library, known_media, &selection.normalized_path)
        };
        bench
            .preindex_time_us
            .store(t_pre.elapsed().as_micros().saturating_as::<u64>(), Ordering::Relaxed);
        LoadedFolder {
            indexed,
            is_empty_in_db,
            stub_candidates,
        }
    }

    /// A large folder: the view shows an empty list while a worker thread loads, filters and sorts the records; the UI thread only
    /// swaps the finished list in. A newer folder selection supersedes this one.
    pub(super) fn load_folder_deferred(
        ctx: &FolderNavigationContext<'_>,
        selection: FolderSelection,
        media_ids: Vec<MediaId>,
    ) {
        let job = list_loader::begin_folder_load();
        tracing::info!(
            job,
            folder_id = selection.folder_id,
            records = media_ids.len(),
            "[ListLoader: FolderLoad] Large folder; loading it on the worker thread"
        );
        BridgeMedia::show_empty_list(ctx.window, ctx.thumbnails, ctx.state);
        StatusBarBridge::info(ctx.window, format!("Loading {} items...", media_ids.len()));
        let started = Instant::now();

        let weak = ctx.window.as_weak();
        let (library, thumbnails, settings, state) = (
            ctx.library.clone(),
            ctx.thumbnails.clone(),
            ctx.settings.clone(),
            ctx.state.clone(),
        );
        list_loader::worker().submit(move || {
            if !list_loader::folder_load_is_current(job) || selection.cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let loaded = Self::load_folder_records(&library, &selection, media_ids);
            let (is_empty_in_db, stub_candidates) = (loaded.is_empty_in_db, loaded.stub_candidates);
            let store = MediaStore::from_records(loaded.indexed);
            if !list_loader::folder_load_is_current(job) || selection.cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let (mut request, query) = BridgeMedia::capture_request(&state);
            request.refine_from = None;
            let layout = lay_out(&library, &store, &request);
            let worker_ms = started.elapsed().as_secs_f64() * 1000.0;

            let _ = weak.upgrade_in_event_loop(move |window| {
                let still_selected = list_loader::folder_load_is_current(job)
                    && state.read().active_folder_token == selection.request_token;
                if !still_selected {
                    tracing::debug!(
                        job,
                        "[ListLoader: FolderLoad] Superseded by a newer selection; the result is dropped"
                    );
                    return;
                }
                let records = store.len();
                let t_commit = Instant::now();
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
                tracing::info!(
                    job,
                    records,
                    worker_ms = format!("{worker_ms:.1}"),
                    commit_ms = format!("{:.1}", t_commit.elapsed().as_secs_f64() * 1000.0),
                    total_ms = format!("{:.1}", started.elapsed().as_secs_f64() * 1000.0),
                    "[ListLoader: FolderLoad] Folder list ready"
                );
                let ctx = FolderNavigationContext {
                    window: &window,
                    library: &library,
                    thumbnails: &thumbnails,
                    settings: &settings,
                    state: &state,
                };
                Self::after_folder_list_ready(&ctx, selection, is_empty_in_db, &stub_candidates);
            });
        });
    }

    /// What follows once the list of the selected folder is shown: inspector and status, then, when the folder is new to the library
    /// or the user asked for a refresh, the scan of the drive (a refresh is the only thing that reads the folder from disk).
    pub(super) fn after_folder_list_ready(
        ctx: &FolderNavigationContext<'_>,
        selection: FolderSelection,
        is_empty_in_db: bool,
        stub_candidates: &[MediaRecord],
    ) {
        let FolderSelection {
            folder_id,
            normalized_path,
            folder_name,
            is_virtual_folder,
            virtual_count,
            cancel_flag,
            bench,
            is_explicit_refresh,
            request_token,
            ..
        } = selection;

        BrowseFolders::apply_pending_first_select(ctx.window, ctx.state, folder_id);
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeTags::sync_tag_selection(ctx.window, ctx.library, ctx.state);
        BridgeMedia::update_virtual_folder_button_state(ctx.window, ctx.library, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        Self::scroll_to_selected(ctx.window);

        let should_scan_and_generate = is_explicit_refresh || is_empty_in_db;
        if !should_scan_and_generate {
            if ctx.settings.get().api.auto_complete_external_stubs {
                let (ready_stubs, still_pending, sources) = ctx
                    .library
                    .external_media_completer()
                    .find_pending_stubs_in_records(stub_candidates);

                if !ready_stubs.is_empty() {
                    let stubs_count = ready_stubs.len();
                    tracing::info!(
                        folder_id = folder_id,
                        stubs_count,
                        sources = ?sources,
                        still_pending,
                        "[FolderNavigation: StubsDetected] Auto-completing external stubs on folder access..."
                    );

                    {
                        let mut st = ctx.state.write();
                        st.is_refreshing_media = true;
                    }
                    ctx.window.set_is_system_busy(true);

                    ctx.thumbnails.tracker().set_generate_total(stubs_count);
                    StatusBarBridge::info(ctx.window, format!("Auto-completing {stubs_count} external file(s)..."));

                    let w_weak = ctx.window.as_weak();
                    let lib_arc = ctx.library.clone();
                    let thumb_arc = ctx.thumbnails.clone();
                    let set_arc = ctx.settings.clone();
                    let st_arc = ctx.state.clone();
                    let config = ThumbnailConfig::from_performance_settings(&ctx.settings.get().performance);

                    std::thread::Builder::new()
                        .name(format!("folder-stubs-completer-{folder_id}"))
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
                    return;
                }
            }

            StatusBarBridge::update_media_index_progress(ctx.window, false, 0, 0);
            StatusBarBridge::info(ctx.window, "Ready.");
            return;
        }

        StatusBarBridge::update_media_index_progress(ctx.window, true, 0, 0);
        StatusBarBridge::info(
            ctx.window,
            if is_virtual_folder {
                format!("Scanning media files in '{folder_name}' (Recursive: {virtual_count} folders)...")
            } else {
                format!("Scanning media files in '{folder_name}'...")
            },
        );

        let w_weak_scanner = ctx.window.as_weak();
        let lib_scanner = ctx.library.clone();
        let thumb_scanner = ctx.thumbnails.clone();
        let set_scanner = ctx.settings.clone();
        let st_scanner = ctx.state.clone();
        let target_p_scanner = normalized_path.clone();
        let cancel_ref = cancel_flag.clone();
        let bench_scanner = bench.clone();

        std::thread::Builder::new()
            .name(format!("media-scanner-fld-{folder_id}"))
            .spawn(move || {
                let w_weak_progress = w_weak_scanner.clone();
                let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));

                let t_scan = Instant::now();
                let auto_detect = set_scanner.get().formats.auto_detect_extensionless_media;

                let scan_folder_ids = if is_virtual_folder {
                    lib_scanner
                        .folders()
                        .get_descendant_folder_ids(folder_id)
                        .unwrap_or_else(|_| vec![folder_id])
                } else {
                    vec![folder_id]
                };

                let mut combined_report = RefreshReport::default();

                for fid in &scan_folder_ids {
                    if cancel_ref.load(Ordering::Acquire) {
                        break;
                    }

                    let w_weak_p = w_weak_progress.clone();
                    let last_rep = last_progress_report.clone();

                    let rep_res = lib_scanner.refresher().refresh_folder_with_options(
                        *fid,
                        auto_detect,
                        Some(cancel_ref.as_ref()),
                        Some(move |cur, tot| {
                            let mut last = last_rep.lock();
                            if last.elapsed().as_millis() >= 35 || cur == tot {
                                *last = Instant::now();
                                let _ = w_weak_p.upgrade_in_event_loop(move |w| {
                                    StatusBarBridge::update_media_index_progress(&w, true, cur, tot);
                                });
                            }
                        }),
                    );

                    if let Ok(rep) = rep_res {
                        combined_report.total_scanned += rep.total_scanned;
                        combined_report.new_media += rep.new_media;
                        combined_report.updated_media += rep.updated_media;
                        combined_report.unchanged_media += rep.unchanged_media;
                        combined_report.deleted_media += rep.deleted_media;
                        combined_report.restored_media += rep.restored_media;
                        combined_report.corrupted_media += rep.corrupted_media;
                        combined_report.hidden_format_media += rep.hidden_format_media;
                        combined_report.ignored.merge(rep.ignored);
                    }
                }

                let scan_elapsed = t_scan.elapsed().as_micros().saturating_as::<u64>();
                bench_scanner.scan_executed.store(true, Ordering::Relaxed);
                bench_scanner.fs_scan_time_us.store(scan_elapsed, Ordering::Relaxed);

                if cancel_ref.load(Ordering::Acquire) {
                    return;
                }

                bench_scanner
                    .files_scanned
                    .store(combined_report.total_scanned, Ordering::Relaxed);
                bench_scanner
                    .new_files
                    .store(combined_report.new_media, Ordering::Relaxed);
                bench_scanner
                    .updated_files
                    .store(combined_report.updated_media, Ordering::Relaxed);
                bench_scanner
                    .unchanged_files
                    .store(combined_report.unchanged_media, Ordering::Relaxed);
                bench_scanner
                    .deleted_files
                    .store(combined_report.deleted_media, Ordering::Relaxed);
                bench_scanner
                    .restored_files
                    .store(combined_report.restored_media, Ordering::Relaxed);

                let mut media_ids = Vec::new();
                if let Ok(rtxn) = lib_scanner.raw_db().env.read_txn() {
                    for fid in &scan_folder_ids {
                        if let Ok(Some(ids)) = lib_scanner.raw_db().media_by_folder.get(&rtxn, fid) {
                            media_ids.extend(ids);
                        }
                    }
                }

                let mut known = Vec::with_capacity(media_ids.len());
                let mut seen = HashSet::new();
                if let Ok(rtxn) = lib_scanner.raw_db().env.read_txn() {
                    for id in media_ids {
                        if seen.insert(id)
                            && let Ok(Some(rec)) = lib_scanner.raw_db().media.get(&rtxn, &id)
                        {
                            known.push(rec);
                        }
                    }
                }

                let indexed_media = if is_virtual_folder {
                    BridgeMedia::preindex_records(&lib_scanner, known)
                } else {
                    BridgeMedia::preindex(&lib_scanner, known, &target_p_scanner)
                };

                let ready_message = match combined_report.ignored.describe() {
                    Some(skipped) => format!("Ready. {skipped}"),
                    None => "Ready.".to_string(),
                };
                if !combined_report.ignored.is_empty() || combined_report.hidden_format_media > 0 {
                    tracing::info!(
                        disabled_format_files = combined_report.ignored.disabled_total(),
                        hidden_format_records = combined_report.hidden_format_media,
                        unsupported_extensions = combined_report.ignored.unsupported.len(),
                        header_mismatch = combined_report.ignored.header_mismatch,
                        "[FolderRefresh: Skipped] Files left out of the refresh"
                    );
                }
                let pipeline_items = BridgeMedia::pipeline_items(&indexed_media, is_explicit_refresh);
                let store = MediaStore::from_records(indexed_media);
                let (mut request, query) = BridgeMedia::capture_request(&st_scanner);
                request.refine_from = None;
                let layout = lay_out(&lib_scanner, &store, &request);

                let _ = w_weak_scanner.upgrade_in_event_loop(move |w| {
                    let is_still_current = {
                        let s = st_scanner.read();
                        s.current_folder_id == Some(folder_id) && s.active_folder_token == request_token
                    };

                    if is_still_current {
                        let pending = PendingList {
                            store: Some(store),
                            layout,
                            query,
                        };
                        BridgeMedia::commit_layout(
                            &w,
                            &lib_scanner,
                            &thumb_scanner,
                            &set_scanner,
                            &st_scanner,
                            pending,
                            MediaReloadOptions::new(false, false),
                        );
                        BridgeInspector::update(&w, &st_scanner);
                        BridgeTags::sync_tag_selection(&w, &lib_scanner, &st_scanner);
                        BridgeMedia::update_virtual_folder_button_state(&w, &lib_scanner, &st_scanner);
                        Self::reload_tree(&w, &lib_scanner, &set_scanner, &st_scanner);

                        StatusBarBridge::update_media_index_progress(&w, false, 0, 0);
                        StatusBarBridge::info(&w, ready_message);
                        StatusBarBridge::update_metrics(&w, &st_scanner);

                        if !pipeline_items.is_empty() {
                            let config = ThumbnailConfig::from_performance_settings(&set_scanner.get().performance);
                            thumb_scanner.start_pipeline(folder_id, pipeline_items, config);
                        }
                    }
                });
            })
            .expect("Failed to spawn media scanner thread");
    }
}
