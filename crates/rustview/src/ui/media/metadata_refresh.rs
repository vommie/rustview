//! Refresh of the metadata of selected files only (no hashing, no thumbnails).

use super::{BridgeMedia, RefreshedMetadataOnlyItem};
use crate::ui::MainWindow;
use crate::ui::browse_window::{apply_record, update_tile};
use crate::ui::file_dialogs::missing::{MissingFileDialogController, MissingFileItem};
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use parking_lot::RwLock;
use rayon::prelude::*;
use rustview_core::library::Library;
use rustview_core::library::models::{MediaId, MediaRecord};
use rustview_core::media::metadata::MetadataExtractor;
use rustview_core::settings::LogObscurer;
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::SaturatingAs;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

impl BridgeMedia {
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
        StatusBarBridge::info(window, format!("Refreshing metadata for {operation_desc}..."));

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
                            .map_or(old_record.modification_time, |d| d.as_secs().saturating_as::<i64>());

                        let ctime = fs_meta
                            .created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(old_record.creation_time, |d| d.as_secs().saturating_as::<i64>());

                        let is_zero_byte = file_size == 0;

                        let (raw_meta, meta_ok) = if is_zero_byte {
                            (
                                rustview_core::media::RawMetadata {
                                    file_size: 0,
                                    modification_time: mtime,
                                    creation_time: ctime,
                                    duration_ms: 0,
                                    width: 0,
                                    height: 0,
                                    media_type: old_record.media_type,
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
                                        rustview_core::media::RawMetadata {
                                            file_size,
                                            modification_time: mtime,
                                            creation_time: ctime,
                                            duration_ms: 0,
                                            width: 0,
                                            height: 0,
                                            media_type: old_record.media_type,
                                            format: old_record.format,
                                        },
                                        false,
                                    )
                                }
                            }
                        };

                        let assigned_tags_count = lib_ref.tags().get_tags_for_media(media_id).map_or(0, |t| t.len());

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
                            missing_files.push(MissingFileItem { filename: fname, path });
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
                            if let Some(target_indexed) =
                                st.all_folder_media.iter_mut().find(|m| m.is_at(&missing.path))
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
                            format!("Refreshed metadata for {refreshed_count} file(s). ({missing_count} missing)"),
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
            format!("{count} selected files")
        };

        Self::execute_metadata_only_refresh(targets, window, library, thumbnails, state, &desc);
    }
}
