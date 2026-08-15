use super::bridge_state::UiState;
use super::models::{format_filesize, rgb8_to_shared_buffer};
use super::{MainWindow, MediaItemView};
use crate::library::Library;
use crate::library::models::{MediaId, MediaRecord};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{MatchMode, MediaSorter, ScopeMode};
use crate::thumbnails::{PreparedTarget, THUMBNAIL_SIZE, ThumbnailManager};
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// Subsystem managing media item pre-indexing, filtering, sorting, and Slint model generation.
pub struct BridgeMedia;

impl BridgeMedia {
    /// Registers search query, regex, match mode, and scope mode callbacks onto MainWindow.
    pub fn register_query_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let st = state.clone();
        window.on_search_query_changed(move |query| {
            st.write().filter.search_query = query.to_string();
            Self::reload(&win, &lib, &thumb, &st, false, false);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let st = state.clone();
        window.on_regex_mode_toggled(move |enabled| {
            st.write().filter.is_regex = enabled;
            Self::reload(&win, &lib, &thumb, &st, false, false);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let st = state.clone();
        window.on_match_mode_changed(move |idx| {
            let mode = if idx == 0 {
                MatchMode::All
            } else {
                MatchMode::Any
            };
            st.write().filter.match_mode = mode;
            Self::reload(&win, &lib, &thumb, &st, false, false);
        });

        let win = window.clone_strong();
        let lib = library;
        let thumb = thumbnails;
        let st = state;
        window.on_scope_mode_changed(move |idx| {
            let scope = if idx == 0 {
                ScopeMode::Folder
            } else {
                ScopeMode::Global
            };
            st.write().filter.scope_mode = scope;
            Self::reload(&win, &lib, &thumb, &st, false, false);
        });
    }

    /// Pre-indexes media records and formats display strings into zero-allocation RAM cache.
    pub fn preindex(
        library: &Library,
        raw_records: Vec<MediaRecord>,
        folder_path: &Path,
    ) -> Vec<IndexedMediaRecord> {
        raw_records
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
            .collect()
    }

    /// Full or filtered reload of the media grid/table view.
    pub fn reload(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
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
        let is_full_reset = filter.is_empty();

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

        if !is_full_reset || reload_lmdb {
            MediaSorter::sort_indexed_refs(&mut sorted_refs, sort_col, sort_ord);
        }
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

        let arena = thumbnails.cache().arena();
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
            } else if let Some(handle) = thumbnails.cache().get(item.record.id)
                && let Some(rgb_data) = arena.read_slot(handle)
            {
                let buf = rgb8_to_shared_buffer(rgb_data, THUMBNAIL_SIZE, THUMBNAIL_SIZE);
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
            query = %current_query,
            is_reset = is_full_reset,
            matched = matched_count,
            filter_us = filter_us,
            sort_us = sort_us,
            model_mapping_us = map_us,
            slint_commit_us = slint_us,
            total_pipeline_us = t_total_start.elapsed().as_micros(),
            "[ViewBenchmark: Zero-Copy] View update pipeline completed"
        );

        if start_pipeline {
            thumbnails.start_pipeline(f_id, pipeline_targets);
        }
    }
}
