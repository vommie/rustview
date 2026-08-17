use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::bridge_inspector::BridgeInspector;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::models::{format_filesize, rgb8_to_shared_buffer};
use super::{MainWindow, MediaItemView};
use crate::library::Library;
use crate::library::models::{FolderId, MediaId, MediaRecord};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{MediaSorter, SortColumn, SortOrder};
use crate::settings::SettingsManager;
use crate::thumbnails::{
    PreparedTarget, ScalingAlgorithm, ThumbnailConfig, ThumbnailFormat, ThumbnailManager,
    VideoThumbnailEngine,
};
use parking_lot::RwLock;
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, SharedPixelBuffer, Timer, VecModel,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

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
            tracing::info!(
                query = %query,
                query_len = query.len(),
                "[SearchFilter: InputEvent] Media search query text changed"
            );
            st.write().filter.search_query = query.to_string();
            Self::reload(&win, &lib, &thumb, &set, &st, false, false);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_regex_mode_toggled(move |enabled| {
            tracing::info!(
                regex_enabled = enabled,
                "[SearchFilter: InputEvent] Media regex filter mode toggled"
            );
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

            tracing::info!(
                col = ?new_col,
                order = ?new_ord,
                "[SortEngine: Global] Updated global sort parameters"
            );

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

        // Hover Video Preview Animation Handler
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
                if let Ok(frames) = thumb.load_preview_frames_sync(media_id) {
                    let frame_buffers: Vec<SharedPixelBuffer<Rgb8Pixel>> = frames
                        .into_iter()
                        .map(|(w, h, rgb)| rgb8_to_shared_buffer(&rgb, w, h))
                        .collect();

                    if !frame_buffers.is_empty() {
                        let win_w = win_hover.as_weak();
                        let st_c = st.clone();
                        let frame_idx = Rc::new(RefCell::new(0usize));
                        let frame_idx_clone = frame_idx;

                        let timer = Timer::default();
                        timer.start(
                            slint::TimerMode::Repeated,
                            std::time::Duration::from_millis(250),
                            move || {
                                if let Some(w) = win_w.upgrade() {
                                    let mut idx_ref = frame_idx_clone.borrow_mut();
                                    let current_idx = *idx_ref % frame_buffers.len();
                                    *idx_ref += 1;
                                    let buf = &frame_buffers[current_idx];
                                    let model = w.get_media_items();
                                    let st_r = st_c.read();
                                    if let Some(&row_idx) = st_r.media_row_map.get(&media_id)
                                        && let Some(mut item) = model.row_data(row_idx)
                                        && item.id == media_id as i32
                                    {
                                        item.thumbnail = Image::from_rgb8(buf.clone());
                                        model.set_row_data(row_idx, item);
                                    }
                                }
                            },
                        );
                        *timer_guard = Some(timer);
                    }
                }
            } else {
                let model = win_hover.get_media_items();
                let st_r = st.read();
                if let Some(&row_idx) = st_r.media_row_map.get(&media_id)
                    && let Some(buf) = st_r.folder_thumbnails.get(&media_id)
                    && let Some(mut item) = model.row_data(row_idx)
                    && item.id == media_id as i32
                {
                    item.thumbnail = Image::from_rgb8(buf.clone());
                    model.set_row_data(row_idx, item);
                }
            }
        });

        let last_resize_time = Arc::new(parking_lot::Mutex::new(Instant::now()));
        let resize_events_count = Arc::new(AtomicUsize::new(0));

        let last_time_clone = last_resize_time;
        let count_clone = resize_events_count;

        window.on_column_resized(move |col_idx, new_width| {
            let t0 = Instant::now();
            let count = count_clone.fetch_add(1, Ordering::Relaxed);

            let mut last_log = last_time_clone.lock();
            let elapsed_since_last = last_log.elapsed();
            if elapsed_since_last.as_millis() >= 100 {
                let hz = if elapsed_since_last.as_millis() > 0 {
                    (count as f64) / (elapsed_since_last.as_secs_f64())
                } else {
                    0.0
                };
                let elapsed_us = t0.elapsed().as_micros();
                tracing::debug!(
                    col_idx = col_idx,
                    width = new_width,
                    event_rate_hz = format!("{:.1}", hz),
                    total_events = count,
                    processing_us = elapsed_us,
                    "[Benchmark: TableView Column Drag] Smooth high-FPS column layout update active"
                );
                *last_log = Instant::now();
                count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_res = settings.clone();
        window.on_column_resize_finished(move |col_idx, final_width| {
            let t0 = Instant::now();
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
            let elapsed_us = t0.elapsed().as_micros();

            tracing::info!(
                col_idx = col_idx,
                final_width = width_f32,
                save_time_us = elapsed_us,
                "[Benchmark: ColumnResizeCommit] Column width persisted to disk storage"
            );
        });

        let set_mode = settings;
        let win_mode = window.as_weak();
        window.on_view_mode_toggled(move |is_table| {
            if let Some(w) = win_mode.upgrade() {
                w.set_is_table_view(is_table);
            }
            let mut app_settings = set_mode.get();
            app_settings.view.is_table_view = is_table;
            set_mode.save_quiet(&app_settings);
            tracing::info!(
                is_table_view = is_table,
                "[ViewMode: Toggle] Persisted active view mode"
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

    pub fn refresh_selected_media(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (selected_ids, is_busy) = {
            let st = state.read();
            let sel = st.file_selection.selected_ids().clone();
            let busy = st.is_refreshing_media || st.is_refreshing_folders;
            (sel, busy)
        };

        if selected_ids.is_empty() || is_busy {
            tracing::warn!("[MediaRefresh] Aborted: No files selected or system is currently busy");
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
                            tracing::warn!(media_id = media_id, path = ?file_path, error = %e, "Failed to refresh media record");
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
                            tracing::warn!(media_id = media_id, path = ?file_path, error = %e, "Failed to regenerate thumbnail");
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
                        let st_read = st_update.read();
                        if let Some(&row_idx) = st_read.media_row_map.get(&media_id)
                            && let Some(mut view) = media_model.row_data(row_idx)
                            && view.id == media_id as i32
                        {
                            view.thumbnail = Image::from_rgb8(shared_buf);
                            view.filesize_str = filesize_str;
                            view.dimensions_str = dimensions_str;
                            view.filename = filename_slint;
                            view.rating = updated_record.rating as i32;
                            view.has_tags = has_tags;
                            view.is_video = matches!(updated_record.media_type, crate::media::MediaType::Video);
                            media_model.set_row_data(row_idx, view);
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

    pub fn preindex(
        library: &Library,
        raw_records: Vec<MediaRecord>,
        folder_path: &Path,
    ) -> Vec<IndexedMediaRecord> {
        let t0 = Instant::now();
        let total_count = raw_records.len();

        let indexed = raw_records
            .into_iter()
            .enumerate()
            .map(|(index, r)| {
                let filesize_str = format_filesize(r.file_size).into();
                let dimensions_str = format!("{} × {}", r.width, r.height).into();
                let filename_slint = r.filename.clone().into();
                let filename_lower = r.filename.to_lowercase();
                let full_path = folder_path.join(&r.filename);
                let has_tags = !library
                    .tags()
                    .get_tags_for_media(r.id)
                    .unwrap_or_default()
                    .is_empty();

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
            .collect();

        tracing::debug!(
            indexed_items = total_count,
            elapsed_us = t0.elapsed().as_micros(),
            "[MediaIndex] In-memory pre-indexing and string formatting complete"
        );

        indexed
    }

    /// Preindexes media records across multiple folders (used for Global queries).
    pub fn preindex_records(
        library: &Library,
        raw_records: Vec<MediaRecord>,
    ) -> Vec<IndexedMediaRecord> {
        let t0 = Instant::now();
        let total_count = raw_records.len();

        let mut folder_path_cache: HashMap<FolderId, PathBuf> = HashMap::new();

        let indexed = raw_records
            .into_iter()
            .enumerate()
            .map(|(index, r)| {
                let filesize_str = format_filesize(r.file_size).into();
                let dimensions_str = format!("{} × {}", r.width, r.height).into();
                let filename_slint = r.filename.clone().into();
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
                let has_tags = !library
                    .tags()
                    .get_tags_for_media(r.id)
                    .unwrap_or_default()
                    .is_empty();

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
            .collect();

        tracing::debug!(
            indexed_items = total_count,
            elapsed_us = t0.elapsed().as_micros(),
            "[MediaIndex] In-memory multi-folder pre-indexing complete"
        );

        indexed
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
        let t_total_start = Instant::now();

        let (folder_id, filter, sort_col, sort_ord, last_query, is_query_active) = {
            let s = state.read();
            (
                s.current_folder_id,
                s.filter.clone(),
                s.sort_column,
                s.sort_order,
                s.last_executed_query.clone(),
                s.is_query_active,
            )
        };

        let path_opt = state.read().current_folder_path.clone();
        if reload_lmdb
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
        let filter_us = t_filter_start.elapsed().as_micros();

        let t_sort_start = Instant::now();
        let mut sorted_refs = filtered_refs;
        MediaSorter::sort_indexed_refs(&mut sorted_refs, sort_col, sort_ord);
        let sort_us = t_sort_start.elapsed().as_micros();

        let t_map_start = Instant::now();
        let matched_len = sorted_refs.len();
        let mut item_views = Vec::with_capacity(matched_len);
        let mut row_map = HashMap::with_capacity(matched_len);
        let mut pipeline_targets = if start_pipeline {
            Vec::with_capacity(matched_len)
        } else {
            Vec::new()
        };

        let mut matched_indices = Vec::with_capacity(matched_len);

        for (row_idx, &item) in sorted_refs.iter().enumerate() {
            matched_indices.push(item.index);
            let is_selected = st.file_selection.is_selected(item.record.id);
            row_map.insert(item.record.id, row_idx);

            if start_pipeline {
                pipeline_targets.push(PreparedTarget {
                    pipeline_id: 0,
                    folder_id: item.record.folder_id,
                    media_id: item.record.id,
                    file_path: item.full_path.clone(),
                });
            }

            let thumbnail_img = if let Some(buf) = st.folder_thumbnails.get(&item.record.id) {
                Image::from_rgb8(buf.clone())
            } else {
                Image::default()
            };

            item_views.push(MediaItemView {
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

        let map_us = t_map_start.elapsed().as_micros();

        {
            let mut st_write = state.write();
            st_write.media_row_map = row_map;
            st_write.active_indices = matched_indices;
            st_write.last_executed_query = current_query.clone();
        }

        let t_slint_start = Instant::now();
        let matched_count = item_views.len();
        window.set_media_items(ModelRc::new(VecModel::from(item_views)));

        let metric_text = if is_query_active {
            if matched_count == total_active_dataset_count {
                format!("Query Items: {}", matched_count)
            } else {
                format!("Query Filter: {} / {}", matched_count, total_active_dataset_count)
            }
        } else {
            format!("Items: {} / {}", matched_count, total_active_dataset_count)
        };
        window.set_items_metric_text(metric_text.into());
        let slint_us = t_slint_start.elapsed().as_micros();

        tracing::debug!(
            is_query = is_query_active,
            search_query = %current_query,
            is_regex = filter.is_regex,
            sort_column = ?sort_col,
            sort_order = ?sort_ord,
            matched = matched_count,
            total = total_active_dataset_count,
            filter_us = filter_us,
            sort_us = sort_us,
            model_mapping_us = map_us,
            slint_commit_us = slint_us,
            total_pipeline_us = t_total_start.elapsed().as_micros(),
            "[MediaView: ViewPipeline] Rendered media items with synchronized query filtering"
        );

        if start_pipeline && !pipeline_targets.is_empty() {
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
            let pipeline_folder_id = folder_id.unwrap_or(0);
            thumbnails.start_pipeline(pipeline_folder_id, pipeline_targets, config);
        }
    }
}
