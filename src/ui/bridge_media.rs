use super::bridge_state::UiState;
use super::models::{format_filesize, rgb8_to_shared_buffer};
use super::{MainWindow, MediaItemView};
use crate::library::Library;
use crate::library::models::{MediaId, MediaRecord};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{MatchMode, MediaSorter, ScopeMode, SortColumn, SortOrder};
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
use std::path::Path;
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
            tracing::debug!(query = %query, "[QueryFilter] Search query updated by user");
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
                "[QueryFilter] Regex filter mode toggled"
            );
            st.write().filter.is_regex = enabled;
            Self::reload(&win, &lib, &thumb, &set, &st, false, false);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_match_mode_changed(move |idx| {
            let mode = if idx == 0 {
                MatchMode::All
            } else {
                MatchMode::Any
            };
            tracing::info!(?mode, "[QueryFilter] Match mode updated");
            st.write().filter.match_mode = mode;
            Self::reload(&win, &lib, &thumb, &set, &st, false, false);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_scope_mode_changed(move |idx| {
            let scope = if idx == 0 {
                ScopeMode::Folder
            } else {
                ScopeMode::Global
            };
            tracing::info!(?scope, "[QueryFilter] Scope mode updated");
            st.write().filter.scope_mode = scope;
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
            *timer_guard = None; // Stop any running preview

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
                // Restore default static thumbnail on mouse leave
                let model = win_hover.get_media_items();
                let st_r = st.read();
                if let Some(&row_idx) = st_r.media_row_map.get(&media_id)
                    && let Some(buf) = st_r.folder_thumbnails.get(&media_id)
                    && let Some(mut item) = model.row_data(row_idx)
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

        let (folder_id, filter, sort_col, sort_ord, last_query) = {
            let s = state.read();
            (
                s.current_folder_id,
                s.filter.clone(),
                s.sort_column,
                s.sort_order,
                s.last_executed_query.clone(),
            )
        };

        let Some(f_id) = folder_id else {
            window.set_media_items(ModelRc::new(VecModel::from(vec![])));
            window.set_items_metric_text("Items: 0".into());
            tracing::debug!("[MediaView] Reload bypassed: No active folder selected");
            return;
        };

        if reload_lmdb {
            let path_opt = state.read().current_folder_path.clone();
            if let Some(ref p) = path_opt {
                let raw_media = library.media().get_by_folder(f_id).unwrap_or_default();
                let indexed = Self::preindex(library, raw_media, p);
                state.write().all_folder_media = indexed;
            }
        }

        if start_pipeline {
            thumbnails.cancel_pipeline();
        }

        let current_query = filter.search_query.trim().to_string();
        let is_refinement = !current_query.is_empty()
            && !last_query.is_empty()
            && current_query.starts_with(&last_query);

        let st = state.read();
        let total_folder_count = st.all_folder_media.len();

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
        let mut new_thumbnails: Vec<(MediaId, SharedPixelBuffer<Rgb8Pixel>)> = Vec::new();

        for (row_idx, &item) in sorted_refs.iter().enumerate() {
            matched_indices.push(item.index);
            let is_selected = st.file_selection.is_selected(item.record.id);
            row_map.insert(item.record.id, row_idx);

            if start_pipeline {
                pipeline_targets.push(PreparedTarget {
                    media_id: item.record.id,
                    file_path: item.full_path.clone(),
                });
            }

            let thumbnail_img = if let Some(buf) = st.folder_thumbnails.get(&item.record.id) {
                Image::from_rgb8(buf.clone())
            } else if let Some(cached) = thumbnails.cache().get(item.record.id) {
                let buf = rgb8_to_shared_buffer(&cached.pixels, cached.width, cached.height);
                new_thumbnails.push((item.record.id, buf.clone()));
                Image::from_rgb8(buf)
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
            for (id, buf) in new_thumbnails {
                st_write.folder_thumbnails.insert(id, buf);
            }
            st_write.media_row_map = row_map;
            st_write.active_indices = matched_indices;
            st_write.last_executed_query = current_query.clone();
        }

        let t_slint_start = Instant::now();
        let matched_count = item_views.len();
        window.set_media_items(ModelRc::new(VecModel::from(item_views)));
        window.set_items_metric_text(
            format!("Items: {} / {}", matched_count, total_folder_count).into(),
        );
        let slint_us = t_slint_start.elapsed().as_micros();

        tracing::debug!(
            folder_id = f_id,
            sort_column = ?sort_col,
            sort_order = ?sort_ord,
            matched = matched_count,
            total_folder = total_folder_count,
            filter_us = filter_us,
            sort_us = sort_us,
            model_mapping_us = map_us,
            slint_commit_us = slint_us,
            total_pipeline_us = t_total_start.elapsed().as_micros(),
            "[MediaView: ViewPipeline] Rendered media items with synchronized global sorting"
        );

        if start_pipeline {
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
            thumbnails.start_pipeline(f_id, pipeline_targets, config);
        }
    }
}
