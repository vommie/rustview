use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::bridge_inspector::BridgeInspector;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::models::{format_timestamp, rgb8_to_shared_buffer};
use super::video_preview_animator::VideoPreviewAnimator;
use super::{MainWindow, MediaItemView};
use crate::library::models::{FolderId, MediaId, MediaRecord};
use crate::library::Library;
use crate::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use crate::media::metadata::MetadataExtractor;
use crate::media::{MediaFormat, MediaHasher, MediaType};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{MediaSorter, SortColumn, SortOrder};
use crate::settings::SettingsManager;
use crate::thumbnails::{
    PreparedTarget, ThumbnailConfig, ThumbnailGenerator, ThumbnailLocation, ThumbnailManager,
    VideoPreviewRecord,
};
use parking_lot::RwLock;
use rayon::prelude::*;
use slint::{ComponentHandle, Image, Model, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
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

/// Container holding processed results from parallel multi-threaded media item full refreshes.
struct ProcessedRefreshedItem {
    media_id: MediaId,
    old_hash: crate::media::FileHash,
    new_hash: crate::media::FileHash,
    updated_record: MediaRecord,
    primary_loc: ThumbnailLocation,
    preview_rec: Option<VideoPreviewRecord>,
    primary_pixels: Vec<u8>,
    primary_width: u32,
    primary_height: u32,
    tags_count: usize,
}

/// Container holding processed results from parallel lightweight metadata-only refreshes.
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

        // High-FPS Virtualization with Optimized Hysteresis Windowing
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
                1 => app_settings.view.col_modified_date_width = width_f32,
                2 => app_settings.view.col_created_date_width = width_f32,
                3 => app_settings.view.col_size_width = width_f32,
                4 => app_settings.view.col_rating_width = width_f32,
                5 => app_settings.view.col_tags_width = width_f32,
                6 => app_settings.view.col_width_width = width_f32,
                7 => app_settings.view.col_height_width = width_f32,
                8 => app_settings.view.col_ar_width = width_f32,
                9 => app_settings.view.col_duration_width = width_f32,
                10 => app_settings.view.col_format_width = width_f32,
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

    /// Decoupled virtualization handler with High-FPS Hysteresis Windowing across all 11 columns.
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

        let (buffer_size, margin_threshold) = if is_table {
            (160usize, 32usize)
        } else {
            (120usize, 24usize)
        };

        // Hysteresis Check: If current loaded slice comfortably covers the visible range, bypass immediately (0 µs cost)
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
                        item.modified_date_str.clone(),
                        item.created_date_str.clone(),
                        item.filesize_mb_str.clone(),
                        item.record.rating as i32,
                        item.rating_str.clone(),
                        item.tags_count as i32,
                        item.tags_count_str.clone(),
                        item.width_str.clone(),
                        item.height_str.clone(),
                        item.ar_str.clone(),
                        item.duration_str.clone(),
                        item.format_str.clone(),
                        item.has_tags,
                        matches!(item.record.media_type, MediaType::Video),
                    )
                });

                if let Some((
                    media_id,
                    filename_slint,
                    modified_date_str,
                    created_date_str,
                    filesize_str,
                    rating,
                    rating_str,
                    tags_count,
                    tags_str,
                    width_str,
                    height_str,
                    ar_str,
                    duration_str,
                    format_str,
                    has_tags,
                    is_video,
                )) = item_info_opt {
                    let is_selected = st.file_selection.is_selected(media_id);

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
                        modified_date_str,
                        created_date_str,
                        filesize_str,
                        rating,
                        rating_str,
                        tags_count,
                        tags_str,
                        width_str,
                        height_str,
                        ar_str,
                        duration_str,
                        format_str,
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

    /// Shared, high-performance multi-threaded metadata-only refresh engine via Rayon.
    /// Re-reads container headers (width, height, AR, duration, format) and filesystem attributes
    /// WITHOUT regenerating thumbnails or video preview frames, with atomic batched LMDB commit.
    pub fn execute_metadata_only_refresh(
        targets: Vec<(MediaId, PathBuf, MediaRecord)>,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        operation_desc: &str,
    ) {
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
                "Refreshing metadata for {}...",
                operation_desc
            )
            .into(),
        );

        let w_weak = window.as_weak();
        let lib_arc = library.clone();
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
                            w.set_media_index_current(cur as i32);
                            w.set_media_index_total(tot as i32);
                        });
                    }
                });

                // =========================================================================
                // 1. Parallel Header Inspection via Rayon (Zero Thumbnail / Preview Overhead)
                // =========================================================================
                let progress_counter_ref = progress_counter.clone();
                let progress_cb_ref = progress_cb.clone();
                let lib_ref = lib_arc.clone();

                let processed_results: Vec<Result<RefreshedMetadataOnlyItem, (MediaId, PathBuf, String)>> = targets
                    .into_par_iter()
                    .map(|(media_id, file_path, old_record)| {
                        if !file_path.exists() {
                            return Err((media_id, file_path, "File not found on disk".to_string()));
                        }

                        let fs_meta = match std::fs::metadata(&file_path) {
                            Ok(m) => m,
                            Err(e) => return Err((media_id, file_path, e.to_string())),
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

                        let raw_meta = match MetadataExtractor::extract(&file_path) {
                            Ok(m) => m,
                            Err(e) => return Err((media_id, file_path, e.to_string())),
                        };

                        let assigned_tags_count = lib_ref
                            .tags()
                            .get_tags_for_media(media_id)
                            .map(|t| t.len())
                            .unwrap_or(0);

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
                        Err((_id, path, _err)) => {
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

                // =========================================================================
                // 2. Single Atomic Batched LMDB Write Transaction
                // =========================================================================
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

                // =========================================================================
                // 3. In-Place UI State & Slint View Updates
                // =========================================================================
                let refreshed_count = successful_items.len();
                let missing_count = missing_files.len();

                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    {
                        let mut st = st_arc.write();
                        st.is_refreshing_media = false;

                        for item in &successful_items {
                            let is_video = matches!(item.updated_record.media_type, MediaType::Video);
                            let width_str: slint::SharedString = if item.updated_record.width > 0 {
                                item.updated_record.width.to_string().into()
                            } else {
                                "-".into()
                            };
                            let height_str: slint::SharedString = if item.updated_record.height > 0 {
                                item.updated_record.height.to_string().into()
                            } else {
                                "-".into()
                            };
                            let ar_str: slint::SharedString = approximate_aspect_ratio(
                                item.updated_record.width,
                                item.updated_record.height,
                            ).into();
                            let ar_value = if item.updated_record.height > 0 {
                                item.updated_record.width as f32 / item.updated_record.height as f32
                            } else {
                                0.0
                            };
                            let duration_str: slint::SharedString = format_duration_hms_ms(
                                item.updated_record.duration_ms,
                                is_video,
                            ).into();
                            let filesize_mb_str: slint::SharedString = format_filesize_mb(item.updated_record.file_size).into();
                            let modified_date_str: slint::SharedString = format_timestamp(item.updated_record.modification_time).into();
                            let created_date_str: slint::SharedString = format_timestamp(item.updated_record.creation_time).into();
                            let format_str: slint::SharedString = item.updated_record.format.default_extension().to_ascii_lowercase().into();

                            if let Some(target_indexed) = st.all_folder_media.iter_mut().find(|m| m.record.id == item.media_id) {
                                target_indexed.record = item.updated_record.clone();
                                target_indexed.width_str = width_str;
                                target_indexed.height_str = height_str;
                                target_indexed.ar_str = ar_str;
                                target_indexed.ar_value = ar_value;
                                target_indexed.duration_str = duration_str;
                                target_indexed.filesize_mb_str = filesize_mb_str;
                                target_indexed.modified_date_str = modified_date_str;
                                target_indexed.created_date_str = created_date_str;
                                target_indexed.format_str = format_str;
                                target_indexed.tags_count = item.tags_count;
                                target_indexed.tags_count_str = item.tags_count.to_string().into();
                                target_indexed.has_tags = item.tags_count > 0;
                            }
                        }
                    }

                    // Refresh visible rows in Slint in-place
                    let media_model = w.get_media_items();
                    let start_window = w.get_visible_start_index() as usize;
                    let visible_count = media_model.row_count();
                    let st_read = st_arc.read();

                    for item in &successful_items {
                        if let Some(&global_row_idx) = st_read.media_row_map.get(&item.media_id)
                            && global_row_idx >= start_window
                            && global_row_idx < start_window + visible_count
                        {
                            let local_idx = global_row_idx - start_window;
                            if let Some(mut view) = media_model.row_data(local_idx)
                                && view.id == item.media_id as i32
                            {
                                if let Some(indexed) = st_read.all_folder_media.iter().find(|m| m.record.id == item.media_id) {
                                    view.width_str = indexed.width_str.clone();
                                    view.height_str = indexed.height_str.clone();
                                    view.ar_str = indexed.ar_str.clone();
                                    view.duration_str = indexed.duration_str.clone();
                                    view.filesize_str = indexed.filesize_mb_str.clone();
                                    view.modified_date_str = indexed.modified_date_str.clone();
                                    view.created_date_str = indexed.created_date_str.clone();
                                    view.format_str = indexed.format_str.clone();
                                    view.tags_count = indexed.tags_count as i32;
                                    view.tags_str = indexed.tags_count_str.clone();
                                    view.has_tags = indexed.has_tags;
                                }

                                media_model.set_row_data(local_idx, view);
                            }
                        }
                    }

                    w.set_is_indexing_media(false);

                    if missing_count > 0 {
                        w.set_status_text(
                            format!(
                                "Refreshed metadata for {} file(s). {} file(s) missing from storage.",
                                refreshed_count, missing_count
                            )
                            .into(),
                        );
                        let missing_dialog = MissingFileDialogController::new();
                        missing_dialog.show(&w, "Refresh Metadata", missing_files);
                    } else {
                        w.set_status_text(
                            format!(
                                "Refreshed metadata for {} in {}ms.",
                                op_desc_owned,
                                t_start.elapsed().as_millis()
                            )
                            .into(),
                        );
                    }

                    BridgeInspector::update(&w, &st_arc);
                    BridgeTags::sync_tag_selection(&w, &lib_arc, &st_arc);

                    tracing::info!(
                        operation = %op_desc_owned,
                        total_requested = total_items,
                        refreshed = refreshed_count,
                        missing = missing_count,
                        elapsed_ms = t_start.elapsed().as_millis(),
                        "[Benchmark: RefreshMetadataOnly] High-speed parallel metadata refresh completed without thumbnail generation"
                    );
                });
            })
            .expect("Failed to spawn parallel media metadata refresher thread");
    }

    /// Ultra-fast multi-threaded metadata-only refresh for selected media files via Rayon.
    pub fn refresh_selected_metadata_only(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
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
                if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id) {
                    targets.push((id, item.full_path.clone(), item.record.clone()));
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
            state,
            &desc,
        );
    }

    /// High-performance multi-threaded manual refresh for selected media files via Rayon.
    /// Combines file metadata, container headers, and thumbnail regeneration in a single pass with batched LMDB commit.
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

        let mut targets: Vec<(MediaId, PathBuf, MediaRecord)> = Vec::with_capacity(selected_ids.len());
        {
            let st = state.read();
            let folder_path_opt = st.current_folder_path.clone();

            for &id in &selected_ids {
                if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id) {
                    targets.push((id, item.full_path.clone(), item.record.clone()));
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

        window.set_is_indexing_media(true);
        window.set_media_index_current(0);
        window.set_media_index_total(total_items as i32);
        window.set_status_text(
            format!(
                "Refreshing {} selected file{} (Full)...",
                total_items,
                if total_items == 1 { "" } else { "s" }
            )
            .into(),
        );

        let config = ThumbnailConfig::from_performance_settings(&settings.get().performance);

        let w_weak = window.as_weak();
        let lib_arc = library.clone();
        let thumb_arc = thumbnails.clone();
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
                            w.set_media_index_current(cur as i32);
                            w.set_media_index_total(tot as i32);
                        });
                    }
                });

                // =========================================================================
                // 1. Parallel Multi-Core Extraction & Generation via Rayon
                // =========================================================================
                let progress_counter_ref = progress_counter.clone();
                let progress_cb_ref = progress_cb.clone();
                let lib_ref = lib_arc.clone();
                let gen_ref = generator.clone();
                let cfg_ref = config.clone();

                let processed_results: Vec<Result<ProcessedRefreshedItem, (MediaId, PathBuf, String)>> = targets
                    .into_par_iter()
                    .map(|(media_id, file_path, old_record)| {
                        if !file_path.exists() {
                            return Err((media_id, file_path, "File not found on disk".to_string()));
                        }

                        let fs_meta = match std::fs::metadata(&file_path) {
                            Ok(m) => m,
                            Err(e) => return Err((media_id, file_path, e.to_string())),
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

                        let new_hash = match MediaHasher::hash_file(&file_path) {
                            Ok(h) => h,
                            Err(e) => return Err((media_id, file_path, e.to_string())),
                        };

                        let format = MediaFormat::from_path(&file_path);
                        let is_video = format.media_type() == Some(MediaType::Video);

                        let (primary_frame, preview_rec, orig_w, orig_h, duration_ms) = if is_video {
                            let video_res = match gen_ref.generate_video_thumbnails(&file_path, &cfg_ref) {
                                Ok(res) => res,
                                Err(e) => return Err((media_id, file_path, e.to_string())),
                            };

                            let primary = video_res.primary_frame;
                            let mut preview_rec = None;

                            if !video_res.preview_frames.is_empty() {
                                let (p_format, p_quality) = if cfg_ref.custom_preview_settings {
                                    (cfg_ref.preview_format, cfg_ref.preview_quality)
                                } else {
                                    (cfg_ref.format, cfg_ref.quality)
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

                            (primary, preview_rec, video_res.orig_width, video_res.orig_height, video_res.duration_ms)
                        } else {
                            let frame = match gen_ref.generate_image_thumbnail(
                                &file_path,
                                cfg_ref.max_bound,
                                cfg_ref.scaling_algorithm,
                            ) {
                                Ok(f) => f,
                                Err(e) => return Err((media_id, file_path, e.to_string())),
                            };
                            (frame.clone(), None, frame.orig_width, frame.orig_height, 0u64)
                        };

                        let compressed_primary = match gen_ref.compress(
                            &primary_frame.rgb_pixels,
                            primary_frame.width,
                            primary_frame.height,
                            cfg_ref.format,
                            cfg_ref.quality,
                        ) {
                            Ok(b) => b,
                            Err(e) => return Err((media_id, file_path, e.to_string())),
                        };

                        let primary_loc = match store.append(&compressed_primary) {
                            Ok(loc) => loc,
                            Err(e) => return Err((media_id, file_path, e.to_string())),
                        };

                        let assigned_tags_count = lib_ref
                            .tags()
                            .get_tags_for_media(media_id)
                            .map(|t| t.len())
                            .unwrap_or(0);

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
                        };

                        let cur_count = progress_counter_ref.fetch_add(1, Ordering::Relaxed) + 1;
                        progress_cb_ref(cur_count, total_items);

                        Ok(ProcessedRefreshedItem {
                            media_id,
                            old_hash: old_record.file_hash,
                            new_hash,
                            updated_record,
                            primary_loc,
                            preview_rec,
                            primary_pixels: primary_frame.rgb_pixels,
                            primary_width: primary_frame.width,
                            primary_height: primary_frame.height,
                            tags_count: assigned_tags_count,
                        })
                    })
                    .collect();

                let mut successful_items = Vec::with_capacity(processed_results.len());
                let mut missing_files = Vec::new();

                for res in processed_results {
                    match res {
                        Ok(item) => successful_items.push(item),
                        Err((_id, path, _err)) => {
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

                // =========================================================================
                // 2. Single Atomic Batched LMDB Write Transaction
                // =========================================================================
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
                        let _ = lib_arc.raw_db().thumbnails.put(&mut wtxn, &item.media_id, &item.primary_loc);

                        if let Some(ref p_rec) = item.preview_rec {
                            let _ = lib_arc.raw_db().video_previews.put(&mut wtxn, &item.media_id, p_rec);
                        } else {
                            let _ = lib_arc.raw_db().video_previews.delete(&mut wtxn, &item.media_id);
                        }
                    }
                    let _ = wtxn.commit();
                }

                // =========================================================================
                // 3. Batch Update UI State, In-Memory Index & Slint Models
                // =========================================================================
                let refreshed_count = successful_items.len();
                let missing_count = missing_files.len();

                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    {
                        let mut st = st_arc.write();
                        st.is_refreshing_media = false;

                        for item in &successful_items {
                            let shared_buf = rgb8_to_shared_buffer(
                                &item.primary_pixels,
                                item.primary_width,
                                item.primary_height,
                            );

                            st.folder_thumbnails.insert(item.media_id, shared_buf.clone());
                            let _ = thumb_arc.cache().insert(
                                item.media_id,
                                item.primary_width,
                                item.primary_height,
                                &item.primary_pixels,
                            );

                            let is_video = matches!(item.updated_record.media_type, MediaType::Video);
                            let width_str: slint::SharedString = if item.updated_record.width > 0 {
                                item.updated_record.width.to_string().into()
                            } else {
                                "-".into()
                            };
                            let height_str: slint::SharedString = if item.updated_record.height > 0 {
                                item.updated_record.height.to_string().into()
                            } else {
                                "-".into()
                            };
                            let ar_str: slint::SharedString = approximate_aspect_ratio(
                                item.updated_record.width,
                                item.updated_record.height,
                            ).into();
                            let ar_value = if item.updated_record.height > 0 {
                                item.updated_record.width as f32 / item.updated_record.height as f32
                            } else {
                                0.0
                            };
                            let duration_str: slint::SharedString = format_duration_hms_ms(
                                item.updated_record.duration_ms,
                                is_video,
                            ).into();
                            let filesize_mb_str: slint::SharedString = format_filesize_mb(item.updated_record.file_size).into();
                            let modified_date_str: slint::SharedString = format_timestamp(item.updated_record.modification_time).into();
                            let created_date_str: slint::SharedString = format_timestamp(item.updated_record.creation_time).into();
                            let format_str: slint::SharedString = item.updated_record.format.default_extension().to_ascii_lowercase().into();

                            if let Some(target_indexed) = st.all_folder_media.iter_mut().find(|m| m.record.id == item.media_id) {
                                target_indexed.record = item.updated_record.clone();
                                target_indexed.width_str = width_str;
                                target_indexed.height_str = height_str;
                                target_indexed.ar_str = ar_str;
                                target_indexed.ar_value = ar_value;
                                target_indexed.duration_str = duration_str;
                                target_indexed.filesize_mb_str = filesize_mb_str;
                                target_indexed.modified_date_str = modified_date_str;
                                target_indexed.created_date_str = created_date_str;
                                target_indexed.format_str = format_str;
                                target_indexed.tags_count = item.tags_count;
                                target_indexed.tags_count_str = item.tags_count.to_string().into();
                                target_indexed.has_tags = item.tags_count > 0;
                            }
                        }
                    }

                    // Refresh visible rows in Slint
                    let media_model = w.get_media_items();
                    let start_window = w.get_visible_start_index() as usize;
                    let visible_count = media_model.row_count();
                    let st_read = st_arc.read();
                    let is_table = w.get_is_table_view();

                    for item in &successful_items {
                        if let Some(&global_row_idx) = st_read.media_row_map.get(&item.media_id)
                            && global_row_idx >= start_window
                            && global_row_idx < start_window + visible_count
                        {
                            let local_idx = global_row_idx - start_window;
                            if let Some(mut view) = media_model.row_data(local_idx)
                                && view.id == item.media_id as i32
                            {
                                if !is_table
                                    && let Some(buf) = st_read.folder_thumbnails.get(&item.media_id)
                                {
                                    view.thumbnail = Image::from_rgb8(buf.clone());
                                }

                                if let Some(indexed) = st_read.all_folder_media.iter().find(|m| m.record.id == item.media_id) {
                                    view.width_str = indexed.width_str.clone();
                                    view.height_str = indexed.height_str.clone();
                                    view.ar_str = indexed.ar_str.clone();
                                    view.duration_str = indexed.duration_str.clone();
                                    view.filesize_str = indexed.filesize_mb_str.clone();
                                    view.modified_date_str = indexed.modified_date_str.clone();
                                    view.created_date_str = indexed.created_date_str.clone();
                                    view.format_str = indexed.format_str.clone();
                                    view.tags_count = indexed.tags_count as i32;
                                    view.tags_str = indexed.tags_count_str.clone();
                                    view.has_tags = indexed.has_tags;
                                }

                                media_model.set_row_data(local_idx, view);
                            }
                        }
                    }

                    w.set_is_indexing_media(false);

                    if missing_count > 0 {
                        w.set_status_text(
                            format!(
                                "Refreshed {} file(s). {} file(s) missing from storage.",
                                refreshed_count, missing_count
                            )
                            .into(),
                        );
                        let missing_dialog = MissingFileDialogController::new();
                        missing_dialog.show(&w, "Refresh File(s)", missing_files);
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

                    BridgeInspector::update(&w, &st_arc);
                    BridgeTags::sync_tag_selection(&w, &lib_arc, &st_arc);

                    tracing::info!(
                        total_requested = total_items,
                        refreshed = refreshed_count,
                        missing = missing_count,
                        elapsed_ms = t_start.elapsed().as_millis(),
                        "[Benchmark: RefreshSelectedMedia] High-performance parallel refresh completed with Rayon multi-threading and batched LMDB commit"
                    );
                });
            })
            .expect("Failed to spawn parallel media item refresher thread");
    }

    /// Parallel multi-core string formatting and pre-indexing for all 11 columns in Rayon.
    pub fn preindex(
        library: &Library,
        raw_records: Vec<MediaRecord>,
        folder_path: &Path,
    ) -> Vec<IndexedMediaRecord> {
        let folder_path_buf = folder_path.to_path_buf();

        let tag_counts_map: HashMap<MediaId, usize> = if let Ok(rtxn) = library.raw_db().env.read_txn() {
            raw_records
                .iter()
                .filter_map(|r| {
                    if let Ok(Some(tags)) = library.raw_db().tags_by_media.get(&rtxn, &r.id) {
                        Some((r.id, tags.len()))
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            HashMap::new()
        };

        raw_records
            .into_par_iter()
            .enumerate()
            .map(|(index, r)| {
                let modified_date_str: slint::SharedString = format_timestamp(r.modification_time).into();
                let created_date_str: slint::SharedString = format_timestamp(r.creation_time).into();
                let filesize_mb_str: slint::SharedString = format_filesize_mb(r.file_size).into();
                let rating_str: slint::SharedString = r.rating.to_string().into();

                let tags_count = tag_counts_map.get(&r.id).copied().unwrap_or(0);
                let tags_count_str: slint::SharedString = tags_count.to_string().into();
                let has_tags = tags_count > 0;

                let width_str: slint::SharedString = if r.width > 0 { r.width.to_string().into() } else { "-".into() };
                let height_str: slint::SharedString = if r.height > 0 { r.height.to_string().into() } else { "-".into() };
                let ar_str: slint::SharedString = approximate_aspect_ratio(r.width, r.height).into();
                let ar_value = if r.height > 0 { r.width as f32 / r.height as f32 } else { 0.0 };

                let is_video = matches!(r.media_type, MediaType::Video);
                let duration_str: slint::SharedString = format_duration_hms_ms(r.duration_ms, is_video).into();
                let format_str: slint::SharedString = r.format.default_extension().to_ascii_lowercase().into();

                let filename_slint: slint::SharedString = r.filename.clone().into();
                let filename_lower = r.filename.to_lowercase();
                let full_path = folder_path_buf.join(&r.filename);

                IndexedMediaRecord {
                    index,
                    record: r,
                    full_path,
                    filename_lower,
                    filename_slint,
                    modified_date_str,
                    created_date_str,
                    filesize_mb_str,
                    rating_str,
                    tags_count,
                    tags_count_str,
                    width_str,
                    height_str,
                    ar_str,
                    ar_value,
                    duration_str,
                    format_str,
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

        let tag_counts_map: HashMap<MediaId, usize> = if let Ok(rtxn) = library.raw_db().env.read_txn() {
            raw_records
                .iter()
                .filter_map(|r| {
                    if let Ok(Some(tags)) = library.raw_db().tags_by_media.get(&rtxn, &r.id) {
                        Some((r.id, tags.len()))
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            HashMap::new()
        };

        raw_records
            .into_iter()
            .enumerate()
            .map(|(index, r)| {
                let modified_date_str: slint::SharedString = format_timestamp(r.modification_time).into();
                let created_date_str: slint::SharedString = format_timestamp(r.creation_time).into();
                let filesize_mb_str: slint::SharedString = format_filesize_mb(r.file_size).into();
                let rating_str: slint::SharedString = r.rating.to_string().into();

                let tags_count = tag_counts_map.get(&r.id).copied().unwrap_or(0);
                let tags_count_str: slint::SharedString = tags_count.to_string().into();
                let has_tags = tags_count > 0;

                let width_str: slint::SharedString = if r.width > 0 { r.width.to_string().into() } else { "-".into() };
                let height_str: slint::SharedString = if r.height > 0 { r.height.to_string().into() } else { "-".into() };
                let ar_str: slint::SharedString = approximate_aspect_ratio(r.width, r.height).into();
                let ar_value = if r.height > 0 { r.width as f32 / r.height as f32 } else { 0.0 };

                let is_video = matches!(r.media_type, MediaType::Video);
                let duration_str: slint::SharedString = format_duration_hms_ms(r.duration_ms, is_video).into();
                let format_str: slint::SharedString = r.format.default_extension().to_ascii_lowercase().into();

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

                IndexedMediaRecord {
                    index,
                    record: r,
                    full_path,
                    filename_lower,
                    filename_slint,
                    modified_date_str,
                    created_date_str,
                    filesize_mb_str,
                    rating_str,
                    tags_count,
                    tags_count_str,
                    width_str,
                    height_str,
                    ar_str,
                    ar_value,
                    duration_str,
                    format_str,
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
        settings: &Arc<SettingsManager>,
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

        // Calculate slice centered around the anchor index
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
                modified_date_str: item.modified_date_str.clone(),
                created_date_str: item.created_date_str.clone(),
                filesize_str: item.filesize_mb_str.clone(),
                rating: item.record.rating as i32,
                rating_str: item.rating_str.clone(),
                tags_count: item.tags_count as i32,
                tags_str: item.tags_count_str.clone(),
                width_str: item.width_str.clone(),
                height_str: item.height_str.clone(),
                ar_str: item.ar_str.clone(),
                duration_str: item.duration_str.clone(),
                format_str: item.format_str.clone(),
                has_tags: item.has_tags,
                thumbnail: thumbnail_img,
                is_selected,
                is_video: matches!(item.record.media_type, MediaType::Video),
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

        if options.start_pipeline {
            let st_read = state.read();
            if let Some(f_id) = st_read.current_folder_id {
                let all_targets: Vec<PreparedTarget> = st_read
                    .all_folder_media
                    .iter()
                    .map(|item| PreparedTarget {
                        pipeline_id: 0,
                        folder_id: item.record.folder_id,
                        media_id: item.record.id,
                        file_path: item.full_path.clone(),
                    })
                    .collect();

                if !all_targets.is_empty() {
                    let config = ThumbnailConfig::from_performance_settings(&settings.get().performance);
                    thumbnails.start_pipeline(f_id, all_targets, config);
                }
            }
        }
    }
}
