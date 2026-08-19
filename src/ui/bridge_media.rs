use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::bridge_inspector::BridgeInspector;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::models::{format_filesize, rgb8_to_shared_buffer};
use super::video_preview_animator::VideoPreviewAnimator;
use super::{MainWindow, MediaItemView};
use crate::library::models::{FolderId, MediaId, MediaRecord};
use crate::library::Library;
use crate::query::filters::IndexedMediaRecord;
use crate::query::{MediaSorter, SortColumn, SortOrder};
use crate::settings::{LogObscurer, SettingsManager};
use crate::thumbnails::{
    ScalingAlgorithm, ThumbnailConfig, ThumbnailFormat, ThumbnailManager, VideoThumbnailEngine,
};
use parking_lot::RwLock;
use rayon::prelude::*;
use slint::{ComponentHandle, Image, Model, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Parameter configuration for reloading and slicing the media dataset.
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

pub struct BridgeMedia;

impl BridgeMedia {
    pub fn register_query_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_search_query_changed(move |query| {
            st.write().filter.search_query = query.to_string();
            Self::reload(&win, &lib, &thumb, &set, &st, false, false);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_regex_mode_toggled(move |enabled| {
            st.write().filter.is_regex = enabled;
            Self::reload(&win, &lib, &thumb, &set, &st, false, false);
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

            Self::reload(&win, &lib, &thumb, &set, &st, false, false);
        });

        let win = window.clone_strong();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        let preview_anim_timer = Rc::new(RefCell::new(Option::<Timer>::None));

        let win_hover = win.clone_strong();
        let timer_clone = preview_anim_timer;
        window.on_media_hover_changed(move |id, is_hover| {
            let media_id = id as u64;
            if !set.get().appearance.enable_hover_previews {
                return;
            }

            let mut timer_guard = timer_clone.borrow_mut();
            *timer_guard = None;

            if is_hover {
                if let Some(images) = VideoPreviewAnimator::load_preview_images(&thumb, media_id) {
                    let win_w = win_hover.as_weak();
                    let st_c = st.clone();

                    *timer_guard = VideoPreviewAnimator::start_animation(images, move |img| {
                        if let Some(w) = win_w.upgrade() {
                            let media_model = w.get_media_items();
                            let start_window = w.get_visible_start_index() as usize;
                            let visible_count = media_model.row_count();
                            let st_r = st_c.read();

                            if let Some(&global_row_idx) = st_r.media_row_map.get(&media_id)
                                && global_row_idx >= start_window
                                && global_row_idx < start_window + visible_count
                            {
                                let local_idx = global_row_idx - start_window;
                                if let Some(mut item) = media_model.row_data(local_idx)
                                    && item.id == media_id as i32
                                {
                                    item.thumbnail = img;
                                    media_model.set_row_data(local_idx, item);
                                }
                            }
                        }
                    });
                }
            } else {
                let media_model = win_hover.get_media_items();
                let start_window = win_hover.get_visible_start_index() as usize;
                let visible_count = media_model.row_count();
                let st_r = st.read();

                if let Some(&global_row_idx) = st_r.media_row_map.get(&media_id)
                    && global_row_idx >= start_window
                    && global_row_idx < start_window + visible_count
                    && let Some(buf) = st_r.folder_thumbnails.get(&media_id)
                {
                    let local_idx = global_row_idx - start_window;
                    if let Some(mut item) = media_model.row_data(local_idx)
                        && item.id == media_id as i32
                    {
                        item.thumbnail = Image::from_rgb8(buf.clone());
                        media_model.set_row_data(local_idx, item);
                    }
                }
            }
        });

        // Viewport Scrolling Virtualization (Pure Consumer: Fast L1 / L2 PackStore cache lookups with Zero Generator Contention)
        let thumb_stream = thumbnails.clone();
        let set_stream = settings.clone();
        let st_stream = state.clone();
        let win_stream = window.clone_strong();
        window.on_visible_chunk_changed(move |first_idx, last_idx, total| {
            Self::handle_visible_chunk_changed(
                first_idx as usize,
                last_idx as usize,
                total as usize,
                &win_stream,
                &st_stream,
                &thumb_stream,
                &set_stream,
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
                1 => app_settings.view.col_dimensions_width = width_f32,
                2 => app_settings.view.col_size_width = width_f32,
                3 => app_settings.view.col_rating_width = width_f32,
                _ => {}
            }

            set_res.save_quiet(&app_settings);
        });

        // Instant View Mode Toggle Handler with 1:1 Scroll Synchronization
        let set_mode = settings;
        let win_mode = window.clone_strong();
        let lib_mode = library;
        let thumb_mode = thumbnails;
        let st_mode = state;

        window.on_view_mode_toggled_with_index(move |is_table, top_item_idx| {
            Self::handle_view_mode_toggled(
                is_table,
                top_item_idx as usize,
                &win_mode,
                &lib_mode,
                &thumb_mode,
                &set_mode,
                &st_mode,
            );
        });
    }

    pub fn is_system_busy(
        window: &MainWindow,
        thumbnails: &ThumbnailManager,
        state: &RwLock<UiState>,
    ) -> bool {
        let st = state.read();
        if st.is_refreshing_media || st.is_refreshing_folders {
            return true;
        }
        drop(st);

        if window.get_is_indexing_media() || window.get_is_refreshing_folders() {
            return true;
        }

        let progress = thumbnails.progress();
        progress.is_preloading || progress.is_generating || progress.active_workers > 0
    }

    /// Handles view mode toggle with seamless bidirectional scroll synchronization and centered slice extraction.
    pub fn handle_view_mode_toggled(
        is_table: bool,
        anchor_item_idx: usize,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t0 = Instant::now();

        // 1. Persist new view mode setting
        {
            let mut app_settings = settings.get();
            app_settings.view.is_table_view = is_table;
            settings.save_quiet(&app_settings);
        }

        // 2. Invalidate virtualization slice to force fresh slice calculation around the anchor index
        {
            let mut st = state.write();
            st.invalidate_loaded_slice();
            st.scroll_bench.is_table_view.store(is_table, Ordering::Relaxed);
        }

        // 3. Update Slint state and synchronize scroll coordinates in the destination view
        window.set_is_table_view(is_table);
        window.invoke_scroll_media_to_index(anchor_item_idx as i32, is_table);

        // 4. Reload the view model for the new view layout centered on anchor_item_idx
        Self::reload_with_options(
            window,
            library,
            thumbnails,
            settings,
            state,
            MediaReloadOptions::with_anchor(false, false, anchor_item_idx),
        );

        let elapsed_us = t0.elapsed().as_micros();
        tracing::info!(
            target_view = if is_table { "TableView" } else { "GridView" },
            anchor_index = anchor_item_idx,
            transition_time_us = elapsed_us,
            "[ViewTransition: Sync] View mode transition completed with scroll synchronization"
        );
    }

    /// Decoupled virtualization handler with Hysteresis Windowing.
    /// Pure consumer of cached thumbnails: checks L1 RAM cache first, then L2 PackStore directly, with ZERO generator task interruption.
    pub fn handle_visible_chunk_changed(
        first_idx: usize,
        last_idx: usize,
        total: usize,
        window: &MainWindow,
        state: &RwLock<UiState>,
        thumbnails: &ThumbnailManager,
        _settings: &SettingsManager,
    ) {
        if total == 0 {
            return;
        }

        let is_table = window.get_is_table_view();
        let scroll_bench = state.read().scroll_bench.clone();
        scroll_bench.record_event();

        // Optimal buffer sizes for instant rendering
        let (buffer_size, margin_threshold) = if is_table {
            (90usize, 20usize)
        } else {
            (60usize, 12usize)
        };

        // Hysteresis Check: If the currently loaded slice already comfortably covers the visible range, BYPASS completely
        {
            let st = state.read();
            if st.loaded_slice_valid
                && st.loaded_slice_len > 0
                && first_idx >= st.loaded_slice_start + margin_threshold
                && last_idx + margin_threshold <= st.loaded_slice_start + st.loaded_slice_len
            {
                scroll_bench.record_hysteresis_hit();
                return;
            }
        }

        let t_reload_start = Instant::now();

        // Compute Centered Window Slice
        let visible_center = (first_idx + last_idx) / 2;
        let half_buf = buffer_size / 2;
        let mut start_window = visible_center.saturating_sub(half_buf);
        let end_window = (start_window + buffer_size).min(total);
        start_window = end_window.saturating_sub(buffer_size);
        let window_len = end_window.saturating_sub(start_window);

        let mut slice_views = Vec::with_capacity(window_len);

        {
            let mut st = state.write();

            for global_idx in start_window..end_window {
                let item_info_opt = if !st.active_indices.is_empty() {
                    st.active_indices.get(global_idx).and_then(|&idx| st.all_folder_media.get(idx))
                } else {
                    st.all_folder_media.get(global_idx)
                }.map(|item| {
                    (
                        item.record.id,
                        item.filename_slint.clone(),
                        item.filesize_str.clone(),
                        item.dimensions_str.clone(),
                        item.record.rating as i32,
                        item.has_tags,
                        matches!(item.record.media_type, crate::media::MediaType::Video),
                    )
                });

                if let Some((media_id, filename_slint, filesize_str, dimensions_str, rating, has_tags, is_video)) = item_info_opt {
                    let is_selected = st.file_selection.is_selected(media_id);

                    // Pure Cached Consumer: Retrieve from L1 cache or L2 PackStore
                    let thumb_img = if is_table {
                        Image::default()
                    } else if let Some(buf) = st.folder_thumbnails.get(&media_id) {
                        Image::from_rgb8(buf.clone())
                    } else if let Some(buf) = thumbnails.try_load_existing(media_id) {
                        st.folder_thumbnails.insert(media_id, buf.clone());
                        Image::from_rgb8(buf)
                    } else {
                        Image::default()
                    };

                    slice_views.push(MediaItemView {
                        id: media_id as i32,
                        filename: filename_slint,
                        filesize_str,
                        dimensions_str,
                        rating,
                        has_tags,
                        thumbnail: thumb_img,
                        is_selected,
                        is_video,
                    });
                }
            }

            st.loaded_slice_start = start_window;
            st.loaded_slice_len = window_len;
            st.loaded_slice_valid = true;
        }

        window.set_visible_start_index(start_window as i32);
        window.set_media_items(ModelRc::new(VecModel::from(slice_views)));

        let slice_us = t_reload_start.elapsed().as_micros() as u64;
        scroll_bench.record_slice_reload(slice_us);

        tracing::debug!(
            is_table = is_table,
            first_idx = first_idx,
            last_idx = last_idx,
            start_window = start_window,
            window_len = window_len,
            slice_time_us = slice_us,
            "[Virtualization: HysteresisSlice] Reloaded buffer slice around visible range"
        );
    }

    /// Dedicated manual refresh for selected media files via context menu.
    /// Operates without blocking even if background folder-wide generation is running.
    pub fn refresh_selected_media(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let selected_ids = {
            let st = state.read();
            st.file_selection.selected_ids().clone()
        };

        if selected_ids.is_empty() {
            return;
        }

        let mut targets: Vec<(MediaId, PathBuf)> = Vec::with_capacity(selected_ids.len());
        {
            let st = state.read();
            let folder_path_opt = st.current_folder_path.clone();

            for &id in &selected_ids {
                if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id) {
                    targets.push((id, item.full_path.clone()));
                } else if let Some(ref folder_path) = folder_path_opt
                    && let Ok(Some(record)) = library.media().get(id)
                {
                    targets.push((id, folder_path.join(&record.filename)));
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

        window.set_is_indexing_media(true);
        window.set_media_index_current(0);
        window.set_media_index_total(total_items as i32);
        window.set_status_text(
            format!(
                "Refreshing {} selected file{}...",
                total_items,
                if total_items == 1 { "" } else { "s" }
            )
            .into(),
        );

        let perf = settings.get().performance;
        let config = ThumbnailConfig {
            max_bound: perf.thumbnail_db_size,
            format: ThumbnailFormat::from_str_name(&perf.thumbnail_format),
            quality: perf.thumbnail_quality,
            scaling_algorithm: ScalingAlgorithm::from_str_name(&perf.scaling_algorithm),
            batch_commit_size: perf.batch_commit_size,
            video_engine: VideoThumbnailEngine::from_str_name(&perf.video_engine),
            video_offset_percent: perf.video_offset_percent,
            generate_video_previews: perf.generate_video_previews,
            preview_frame_count: perf.preview_frame_count,
            custom_preview_settings: perf.custom_preview_settings,
            preview_format: ThumbnailFormat::from_str_name(&perf.preview_format),
            preview_quality: perf.preview_quality,
        };

        let w_weak = window.as_weak();
        let lib_arc = library.clone();
        let thumb_arc = thumbnails.clone();
        let st_arc = state.clone();

        std::thread::Builder::new()
            .name("media-item-refresher".into())
            .spawn(move || {
                let t_start = Instant::now();
                let mut refreshed_count = 0usize;
                let missing_files = Arc::new(parking_lot::Mutex::new(Vec::new()));
                let missing_files_ref = missing_files.clone();

                for (idx, (media_id, file_path)) in targets.into_iter().enumerate() {
                    let cur_step = idx + 1;

                    let updated_record = match lib_arc.file_operations().refresh_media(media_id) {
                        Ok(rec) => rec,
                        Err(e) => {
                            tracing::warn!(media_id = media_id, path = %LogObscurer::path(&file_path), error = %e, "Failed to refresh media record");
                            let file_name = file_path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| format!("Media #{}", media_id));

                            if !file_path.exists() {
                                missing_files_ref.lock().push(MissingFileItem {
                                    filename: file_name,
                                    path: file_path.clone(),
                                });
                            }

                            let _ = w_weak.upgrade_in_event_loop(move |w| {
                                w.set_media_index_current(cur_step as i32);
                            });
                            continue;
                        }
                    };

                    let cached_thumb = match thumb_arc.regenerate_sync(media_id, &file_path, &config) {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::warn!(media_id = media_id, path = %LogObscurer::path(&file_path), error = %e, "Failed to regenerate thumbnail");
                            let _ = w_weak.upgrade_in_event_loop(move |w| {
                                w.set_media_index_current(cur_step as i32);
                            });
                            continue;
                        }
                    };

                    refreshed_count += 1;

                    let filesize_str: slint::SharedString = format_filesize(updated_record.file_size).into();
                    let dimensions_str: slint::SharedString = format!("{} × {}", updated_record.width, updated_record.height).into();
                    let filename_slint: slint::SharedString = updated_record.filename.clone().into();
                    let filename_lower = updated_record.filename.to_lowercase();
                    let has_tags = !lib_arc
                        .tags()
                        .get_tags_for_media(media_id)
                        .unwrap_or_default()
                        .is_empty();

                    let shared_buf = rgb8_to_shared_buffer(
                        &cached_thumb.pixels,
                        cached_thumb.width,
                        cached_thumb.height,
                    );

                    let st_update = st_arc.clone();
                    let w_update = w_weak.clone();

                    let _ = w_update.upgrade_in_event_loop(move |w| {
                        {
                            let mut st = st_update.write();
                            st.folder_thumbnails.insert(media_id, shared_buf.clone());

                            let folder_path_opt = st.current_folder_path.clone();

                            if let Some(item) = st.all_folder_media.iter_mut().find(|m| m.record.id == media_id) {
                                item.record = updated_record.clone();
                                item.filesize_str = filesize_str.clone();
                                item.dimensions_str = dimensions_str.clone();
                                item.filename_slint = filename_slint.clone();
                                item.filename_lower = filename_lower;
                                item.has_tags = has_tags;
                                if let Some(ref folder_p) = folder_path_opt {
                                    item.full_path = folder_p.join(&updated_record.filename);
                                }
                            }
                        }

                        let media_model = w.get_media_items();
                        let start_window = w.get_visible_start_index() as usize;
                        let visible_count = media_model.row_count();
                        let st_read = st_update.read();

                        if let Some(&global_row_idx) = st_read.media_row_map.get(&media_id)
                            && global_row_idx >= start_window
                            && global_row_idx < start_window + visible_count
                        {
                            let local_idx = global_row_idx - start_window;
                            if let Some(mut view) = media_model.row_data(local_idx)
                                && view.id == media_id as i32
                            {
                                if !w.get_is_table_view() {
                                    view.thumbnail = Image::from_rgb8(shared_buf);
                                }
                                view.filesize_str = filesize_str;
                                view.dimensions_str = dimensions_str;
                                view.filename = filename_slint;
                                view.rating = updated_record.rating as i32;
                                view.has_tags = has_tags;
                                view.is_video = matches!(updated_record.media_type, crate::media::MediaType::Video);
                                media_model.set_row_data(local_idx, view);
                            }
                        }

                        w.set_media_index_current(cur_step as i32);
                    });
                }

                let st_done = st_arc.clone();
                let lib_done = lib_arc.clone();
                let missing_files_final = missing_files.clone();
                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    {
                        let mut st = st_done.write();
                        st.is_refreshing_media = false;
                    }

                    w.set_is_indexing_media(false);

                    let missing_list = missing_files_final.lock().drain(..).collect::<Vec<_>>();
                    let missing_count = missing_list.len();

                    if missing_count > 0 {
                        w.set_status_text(
                            format!(
                                "Refreshed {} file(s). {} file(s) missing from storage.",
                                refreshed_count, missing_count
                            )
                            .into(),
                        );
                        let missing_dialog = MissingFileDialogController::new();
                        missing_dialog.show(&w, "Refresh File(s)", missing_list);
                    } else {
                        w.set_status_text(
                            format!(
                                "Refreshed {} of {} selected file{} in {}ms.",
                                refreshed_count,
                                total_items,
                                if total_items == 1 { "" } else { "s" },
                                t_start.elapsed().as_millis()
                            )
                            .into(),
                        );
                    }

                    BridgeInspector::update(&w, &st_done);
                    BridgeTags::sync_tag_selection(&w, &lib_done, &st_done);
                });
            })
            .expect("Failed to spawn media item refresher thread");
    }

    /// Parallel multi-core string formatting and pre-indexing using Rayon and 1-txn tag lookup.
    pub fn preindex(
        library: &Library,
        raw_records: Vec<MediaRecord>,
        folder_path: &Path,
    ) -> Vec<IndexedMediaRecord> {
        let folder_path_buf = folder_path.to_path_buf();

        let tagged_media_ids: HashSet<MediaId> = if let Ok(rtxn) = library.raw_db().env.read_txn() {
            raw_records
                .iter()
                .filter_map(|r| {
                    if let Ok(Some(tags)) = library.raw_db().tags_by_media.get(&rtxn, &r.id) {
                        if !tags.is_empty() {
                            Some(r.id)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            HashSet::new()
        };

        raw_records
            .into_par_iter()
            .enumerate()
            .map(|(index, r)| {
                let filesize_str: slint::SharedString = format_filesize(r.file_size).into();
                let dimensions_str: slint::SharedString = format!("{} × {}", r.width, r.height).into();
                let filename_slint: slint::SharedString = r.filename.clone().into();
                let filename_lower = r.filename.to_lowercase();
                let full_path = folder_path_buf.join(&r.filename);
                let has_tags = tagged_media_ids.contains(&r.id);

                IndexedMediaRecord {
                    index,
                    record: r,
                    full_path,
                    filename_lower,
                    filename_slint,
                    filesize_str,
                    dimensions_str,
                    has_tags,
                }
            })
            .collect()
    }

    /// Preindexes media records across multiple folders (used for Global queries).
    pub fn preindex_records(
        library: &Library,
        raw_records: Vec<MediaRecord>,
    ) -> Vec<IndexedMediaRecord> {
        let mut folder_path_cache: HashMap<FolderId, PathBuf> = HashMap::new();

        let tagged_media_ids: HashSet<MediaId> = if let Ok(rtxn) = library.raw_db().env.read_txn() {
            raw_records
                .iter()
                .filter_map(|r| {
                    if let Ok(Some(tags)) = library.raw_db().tags_by_media.get(&rtxn, &r.id) {
                        if !tags.is_empty() {
                            Some(r.id)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            HashSet::new()
        };

        raw_records
            .into_iter()
            .enumerate()
            .map(|(index, r)| {
                let filesize_str: slint::SharedString = format_filesize(r.file_size).into();
                let dimensions_str: slint::SharedString = format!("{} × {}", r.width, r.height).into();
                let filename_slint: slint::SharedString = r.filename.clone().into();
                let filename_lower = r.filename.to_lowercase();

                let folder_p = folder_path_cache.entry(r.folder_id).or_insert_with(|| {
                    library
                        .folders()
                        .get_folder(r.folder_id)
                        .ok()
                        .flatten()
                        .map(|f| PathBuf::from(f.path))
                        .unwrap_or_default()
                });

                let full_path = folder_p.join(&r.filename);
                let has_tags = tagged_media_ids.contains(&r.id);

                IndexedMediaRecord {
                    index,
                    record: r,
                    full_path,
                    filename_lower,
                    filename_slint,
                    filesize_str,
                    dimensions_str,
                    has_tags,
                }
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

    pub fn reload_with_options(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        _settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        options: MediaReloadOptions,
    ) {
        let (folder_id, filter, sort_col, sort_ord, last_query, is_query_active, bench_opt) = {
            let s = state.read();
            (
                s.current_folder_id,
                s.filter.clone(),
                s.sort_column,
                s.sort_order,
                s.last_executed_query.clone(),
                s.is_query_active,
                s.active_folder_bench.clone(),
            )
        };

        let path_opt = state.read().current_folder_path.clone();
        if options.reload_lmdb
            && !is_query_active
            && let Some(f_id) = folder_id
            && let Some(ref p) = path_opt
        {
            let raw_media = library.media().get_by_folder(f_id).unwrap_or_default();
            let indexed = Self::preindex(library, raw_media, p);
            state.write().all_folder_media = indexed;
        }

        let current_query = filter.search_query.trim().to_string();
        let is_refinement = !current_query.is_empty()
            && !last_query.is_empty()
            && current_query.starts_with(&last_query);

        let st = state.read();
        let total_active_dataset_count = st.all_folder_media.len();

        let subset = if is_refinement && !st.active_indices.is_empty() {
            Some(st.active_indices.as_slice())
        } else {
            None
        };

        let t_filter_start = Instant::now();
        let filtered_refs = filter.execute_indexed_refs(library, &st.all_folder_media, subset);
        let filter_us = t_filter_start.elapsed().as_micros() as u64;

        if let Some(ref bench) = bench_opt {
            bench.filter_time_us.store(filter_us, Ordering::Relaxed);
            bench.items_matched.store(filtered_refs.len(), Ordering::Relaxed);
        }

        let t_sort_start = Instant::now();
        let mut sorted_refs = filtered_refs;
        MediaSorter::sort_indexed_refs(&mut sorted_refs, sort_col, sort_ord);
        let sort_us = t_sort_start.elapsed().as_micros() as u64;

        if let Some(ref bench) = bench_opt {
            bench.sort_time_us.store(sort_us, Ordering::Relaxed);
        }

        let t_map_start = Instant::now();
        let matched_len = sorted_refs.len();
        let mut row_map = HashMap::with_capacity(matched_len);
        let is_table = window.get_is_table_view();
        let p0_initial_window = if is_table { 90usize } else { 60usize };

        let mut matched_indices = Vec::with_capacity(matched_len);

        for (row_idx, &item) in sorted_refs.iter().enumerate() {
            matched_indices.push(item.index);
            row_map.insert(item.record.id, row_idx);
        }

        // Calculate slice centered around the anchor index (or index 0 on default load)
        let anchor = options.anchor_index.unwrap_or(0);
        let half_buf = p0_initial_window / 2;
        let mut start_window = anchor.saturating_sub(half_buf);
        let end_window = (start_window + p0_initial_window).min(matched_len);
        start_window = end_window.saturating_sub(p0_initial_window);
        let slice_len = end_window.saturating_sub(start_window);

        let mut slice_views = Vec::with_capacity(slice_len);

        for &item in &sorted_refs[start_window..end_window] {
            let is_selected = st.file_selection.is_selected(item.record.id);

            let thumbnail_img = if is_table {
                Image::default()
            } else if let Some(buf) = st.folder_thumbnails.get(&item.record.id) {
                Image::from_rgb8(buf.clone())
            } else if let Some(buf) = thumbnails.try_load_existing(item.record.id) {
                Image::from_rgb8(buf)
            } else {
                Image::default()
            };

            slice_views.push(MediaItemView {
                id: item.record.id as i32,
                filename: item.filename_slint.clone(),
                filesize_str: item.filesize_str.clone(),
                dimensions_str: item.dimensions_str.clone(),
                rating: item.record.rating as i32,
                has_tags: item.has_tags,
                thumbnail: thumbnail_img,
                is_selected,
                is_video: matches!(item.record.media_type, crate::media::MediaType::Video),
            });
        }

        drop(st);

        let map_us = t_map_start.elapsed().as_micros() as u64;
        if let Some(ref bench) = bench_opt {
            bench.view_model_map_time_us.store(map_us, Ordering::Relaxed);
        }

        {
            let mut st_write = state.write();
            st_write.media_row_map = row_map;
            st_write.active_indices = matched_indices;
            st_write.last_executed_query = current_query.clone();
            st_write.loaded_slice_start = start_window;
            st_write.loaded_slice_len = slice_len;
            st_write.loaded_slice_valid = true;
        }

        let t_slint_start = Instant::now();
        window.set_total_items_count(matched_len as i32);
        window.set_visible_start_index(start_window as i32);
        window.set_media_items(ModelRc::new(VecModel::from(slice_views)));

        let metric_text = if is_query_active {
            if matched_len == total_active_dataset_count {
                format!("Query Items: {}", matched_len)
            } else {
                format!("Query Filter: {} / {}", matched_len, total_active_dataset_count)
            }
        } else {
            format!("Items: {} / {}", matched_len, total_active_dataset_count)
        };
        window.set_items_metric_text(metric_text.into());

        let commit_us = t_slint_start.elapsed().as_micros() as u64;
        if let Some(ref bench) = bench_opt {
            bench.slint_commit_time_us.store(commit_us, Ordering::Relaxed);
        }
    }
}
