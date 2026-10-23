//! Full refresh of selected files: metadata, hash, thumbnails and library records.

use super::{BridgeMedia, ProcessedRefreshedItem};
use crate::ui::MainWindow;
use crate::ui::browse_window::{apply_record, update_tile};
use crate::ui::file_dialogs::missing::{MissingFileDialogController, MissingFileItem};
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::models::rgb8_to_shared_buffer;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tags::BridgeTags;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rayon::prelude::*;
use rustview_core::library::Library;
use rustview_core::library::models::{MediaId, MediaRecord};
use rustview_core::media::{MediaFormat, MediaHasher, MediaType};
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::{ThumbnailConfig, ThumbnailGenerator, ThumbnailManager, VideoPreviewRecord};
use slint::{ComponentHandle, Image};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

impl BridgeMedia {
    pub fn refresh_selected_media(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        Self::execute_media_refresh(window, library, thumbnails, tab_manager, settings, state);
    }

    pub(super) fn execute_media_refresh(
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
            format!("Refreshing {total_items} selected files (Full)...")
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
                            .map_or(old_record.modification_time, |d| d.as_secs() as i64);

                        let ctime = fs_meta
                            .created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(old_record.creation_time, |d| d.as_secs() as i64);

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
                        let is_video = rustview_core::media::MetadataExtractor::resolve_media_type(&file_path, format.media_type().unwrap_or(MediaType::Image)) == MediaType::Video;

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
                            .map_or(0, |t| t.len());

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
                            let is_pending_stub = rec_opt.as_ref().is_some_and(|r| r.source.is_some() && (r.file_size == 0 || r.width == 0));

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
                            tab_mgr_arc.reload_media(item.media_id);
                        }

                        for missing in &missing_files {
                            if let Some(target_indexed) = st.all_folder_media.iter_mut().find(|m| m.is_at(&missing.path))
                                && !target_indexed.is_pending
                            {
                                target_indexed.mark_deleted();
                            }
                        }
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
                                "Refreshed {refreshed_count} file(s). ({missing_count} missing / pending)"
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
}
