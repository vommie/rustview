use crate::ui::models::{
    format_filesize, format_timestamp, rgb8_to_slint_image,
};
use crate::ui::media::video_preview::VideoPreviewAnimator;
use crate::ui::{ConflictFileView, FileOpsConflictDialog, MainWindow};
use rustview_core::library::models::{
    FileConflictItemDetails, FileConflictReport, FileConflictResolution, FileTransferKind, MediaId,
};
use rustview_core::media::{MediaFormat, MediaHasher, MediaType};
use rustview_core::settings::{LogObscurer, SettingsManager};
use crate::system::SystemFileHandler;
use rustview_core::thumbnails::{ThumbnailConfig, ThumbnailGenerator, ThumbnailManager};
use slint::{CloseRequestResponse, ComponentHandle, Image, Timer};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

thread_local! {
    static CONFLICT_PREVIEW_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
    static CONFLICT_HOVER_TARGET: RefCell<Option<&'static str>> = const { RefCell::new(None) };
}

type ConflictResolutionCallback = Box<dyn Fn(FileConflictResolution, bool)>;

/// Encapsulated metadata and cover thumbnail container for conflict cards.
#[derive(Clone)]
pub struct ConflictThumbInfo {
    pub media_id: Option<MediaId>,
    pub thumbnail: Image,
    pub format: MediaFormat,
    pub full_path: PathBuf,
}

/// Structured parameter bundle for presenting the file conflict modal dialog.
pub struct ConflictDialogParams<'a> {
    pub window: &'a MainWindow,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub report: &'a FileConflictReport,
    pub conflict_index: usize,
    pub total_conflicts: usize,
    pub transfer_kind: FileTransferKind,
}

/// Specialized controller managing the File Name Conflict split-view comparison dialog with interactive hover video/GIF previews.
#[derive(Default, Clone)]
pub struct FileOpsConflictDialogController {
    dialog: Rc<RefCell<Option<FileOpsConflictDialog>>>,
    active_callback: Rc<RefCell<Option<ConflictResolutionCallback>>>,
    active_paths: Rc<RefCell<Option<(PathBuf, PathBuf)>>>,
    active_source_info: Rc<RefCell<Option<ConflictThumbInfo>>>,
    active_dest_info: Rc<RefCell<Option<ConflictThumbInfo>>>,
    thumbnails: Rc<RefCell<Option<Arc<ThumbnailManager>>>>,
    settings: Rc<RefCell<Option<Arc<SettingsManager>>>>,
}

impl FileOpsConflictDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the File Conflict modal dialog with rich visual comparison, animated GIF/video hover previews, and resolution buttons.
    pub fn show<F>(
        &self,
        params: ConflictDialogParams<'_>,
        on_resolve: F,
    ) where
        F: Fn(FileConflictResolution, bool) + 'static,
    {
        let t0 = Instant::now();
        self.ensure_dialog_instantiated(params.window);

        *self.thumbnails.borrow_mut() = Some(params.thumbnails.clone());
        *self.settings.borrow_mut() = Some(params.settings.clone());

        if let Some(ref win) = *self.dialog.borrow() {
            let t_thumb = Instant::now();
            let thumb_config = ThumbnailConfig::from_performance_settings(&params.settings.get().performance);
            let src_thumb = load_conflict_thumbnail(params.thumbnails, &thumb_config, &params.report.source.full_path, params.report.source.media_id);
            let dst_thumb = load_conflict_thumbnail(params.thumbnails, &thumb_config, &params.report.destination.full_path, params.report.destination.media_id);
            let thumb_load_us = t_thumb.elapsed().as_micros();

            let src_view = convert_details_to_view(&params.report.source, src_thumb.clone());
            let dst_view = convert_details_to_view(&params.report.destination, dst_thumb.clone());

            tracing::info!(
                conflict_index = params.conflict_index,
                total_conflicts = params.total_conflicts,
                transfer_kind = ?params.transfer_kind,
                src_filename = %LogObscurer::filename(&params.report.source.filename),
                src_size = params.report.source.file_size,
                src_hash = %MediaHasher::to_hex(&params.report.source.file_hash),
                dst_folder = %LogObscurer::folder(Some(params.report.destination.folder_id), &params.report.destination.folder_path),
                dst_size = params.report.destination.file_size,
                dst_hash = %MediaHasher::to_hex(&params.report.destination.file_hash),
                thumb_load_us = thumb_load_us,
                prep_total_us = t0.elapsed().as_micros(),
                "[ConflictDialog: Show] Displaying side-by-side comparison modal dialog"
            );

            win.set_source_file(src_view);
            win.set_destination_file(dst_view);
            win.set_conflict_index(params.conflict_index as i32);
            win.set_total_conflicts(params.total_conflicts as i32);
            win.set_is_copy(params.transfer_kind == FileTransferKind::Copy);
            win.set_apply_to_all(false);

            *self.active_callback.borrow_mut() = Some(Box::new(on_resolve));
            *self.active_paths.borrow_mut() = Some((
                params.report.source.full_path.clone(),
                params.report.destination.full_path.clone(),
            ));
            *self.active_source_info.borrow_mut() = Some(ConflictThumbInfo {
                media_id: params.report.source.media_id,
                thumbnail: src_thumb,
                format: params.report.source.format,
                full_path: params.report.source.full_path.clone(),
            });
            *self.active_dest_info.borrow_mut() = Some(ConflictThumbInfo {
                media_id: params.report.destination.media_id,
                thumbnail: dst_thumb,
                format: params.report.destination.format,
                full_path: params.report.destination.full_path.clone(),
            });

            params.window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the dialog and releases the modal lock on the main application window.
    pub fn close(&self, window: &MainWindow) {
        CONFLICT_HOVER_TARGET.with(|t| *t.borrow_mut() = None);
        CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        *self.active_paths.borrow_mut() = None;
        *self.active_source_info.borrow_mut() = None;
        *self.active_dest_info.borrow_mut() = None;
        window.set_is_modal_locked(false);
        tracing::debug!("[ConflictDialog: Close] Closed conflict dialog and released modal window lock");
    }

    fn ensure_dialog_instantiated(&self, window: &MainWindow) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = FileOpsConflictDialog::new().expect("Failed to instantiate FileOpsConflictDialog");

            let win_dlg_skip = win.as_weak();
            let cb_slot_skip = self.active_callback.clone();
            let ctrl_skip = self.clone();
            let win_main_skip = window.clone_strong();
            win.on_skip_clicked(move || {
                let apply_all = win_dlg_skip.upgrade().map(|w| w.get_apply_to_all()).unwrap_or(false);
                tracing::info!(
                    apply_to_all = apply_all,
                    "[ConflictDialog: UserAction] User clicked SKIP"
                );
                let cb = cb_slot_skip.borrow_mut().take();
                ctrl_skip.close(&win_main_skip);
                if let Some(f) = cb {
                    f(FileConflictResolution::Skip, apply_all);
                }
            });

            let win_dlg_ren = win.as_weak();
            let cb_slot_ren = self.active_callback.clone();
            let ctrl_ren = self.clone();
            let win_main_ren = window.clone_strong();
            win.on_auto_rename_clicked(move || {
                let apply_all = win_dlg_ren.upgrade().map(|w| w.get_apply_to_all()).unwrap_or(false);
                tracing::info!(
                    apply_to_all = apply_all,
                    "[ConflictDialog: UserAction] User clicked AUTO-RENAME"
                );
                let cb = cb_slot_ren.borrow_mut().take();
                ctrl_ren.close(&win_main_ren);
                if let Some(f) = cb {
                    f(FileConflictResolution::AutoRename, apply_all);
                }
            });

            let win_dlg_over = win.as_weak();
            let cb_slot_over = self.active_callback.clone();
            let ctrl_over = self.clone();
            let win_main_over = window.clone_strong();
            win.on_overwrite_clicked(move || {
                let apply_all = win_dlg_over.upgrade().map(|w| w.get_apply_to_all()).unwrap_or(false);
                tracing::info!(
                    apply_to_all = apply_all,
                    "[ConflictDialog: UserAction] User clicked OVERWRITE"
                );
                let cb = cb_slot_over.borrow_mut().take();
                ctrl_over.close(&win_main_over);
                if let Some(f) = cb {
                    f(FileConflictResolution::Overwrite, apply_all);
                }
            });

            let cb_slot_cancel = self.active_callback.clone();
            let ctrl_cancel = self.clone();
            let win_main_cancel = window.clone_strong();
            win.on_cancel_clicked(move || {
                tracing::info!("[ConflictDialog: UserAction] User clicked CANCEL. Aborting conflict resolution.");
                let cb = cb_slot_cancel.borrow_mut().take();
                ctrl_cancel.close(&win_main_cancel);
                if let Some(f) = cb {
                    f(FileConflictResolution::Cancel, false);
                }
            });

            let cb_slot_close = self.active_callback.clone();
            let ctrl_close = self.clone();
            let win_main_close = window.clone_strong();
            win.window().on_close_requested(move || {
                tracing::info!("[ConflictDialog: UserAction] Dialog window close requested. Treating as Cancel.");
                let cb = cb_slot_close.borrow_mut().take();
                ctrl_close.close(&win_main_close);
                if let Some(f) = cb {
                    f(FileConflictResolution::Cancel, false);
                }
                CloseRequestResponse::HideWindow
            });

            let paths_slot_src = self.active_paths.clone();
            win.on_open_source_file(move || {
                let paths_guard = paths_slot_src.borrow();
                if let Some((src_p, _)) = paths_guard.as_ref() {
                    tracing::info!(
                        path = %LogObscurer::path(src_p),
                        "[ConflictDialog: ThumbnailClick] Opening source file with default application"
                    );
                    if let Err(e) = SystemFileHandler::open_file(src_p) {
                        tracing::error!(
                            error = %e,
                            "[ConflictDialog] Failed to open source file"
                        );
                    }
                }
            });

            let paths_slot_dst = self.active_paths.clone();
            win.on_open_destination_file(move || {
                let paths_guard = paths_slot_dst.borrow();
                if let Some((_, dst_p)) = paths_guard.as_ref() {
                    tracing::info!(
                        path = %LogObscurer::path(dst_p),
                        "[ConflictDialog: ThumbnailClick] Opening destination file with default application"
                    );
                    if let Err(e) = SystemFileHandler::open_file(dst_p) {
                        tracing::error!(
                            error = %e,
                            "[ConflictDialog] Failed to open destination file"
                        );
                    }
                }
            });

            // Hover Video & GIF Previews for Source File Thumbnail
            let win_weak_src = win.as_weak();
            let src_info_slot = self.active_source_info.clone();
            let thumb_slot = self.thumbnails.clone();
            let set_slot = self.settings.clone();

            win.on_source_hover_changed(move |is_hover| {
                if !set_slot
                    .borrow()
                    .as_ref()
                    .map(|s| s.get().appearance.enable_hover_previews)
                    .unwrap_or(true)
                {
                    return;
                }

                let src_info_guard = src_info_slot.borrow();
                let Some(ref info) = *src_info_guard else {
                    return;
                };

                if is_hover {
                    CONFLICT_HOVER_TARGET.with(|t| *t.borrow_mut() = Some("source"));
                    CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                    let thumb_guard = thumb_slot.borrow();
                    let Some(thumb_mgr) = thumb_guard.as_ref() else {
                        return;
                    };

                    let win_w = win_weak_src.clone();

                    if info.format == MediaFormat::Gif {
                        if let Some(media_id) = info.media_id
                            && let Some(preview) = thumb_mgr.try_get_animated_preview(media_id)
                        {
                            let timed_frames = VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                            let timer = VideoPreviewAnimator::start_variable_delay_animation(timed_frames, move |img| {
                                if let Some(w) = win_w.upgrade() {
                                    let mut src_file = w.get_source_file();
                                    src_file.thumbnail = img;
                                    w.set_source_file(src_file);
                                }
                            });
                            CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                        } else {
                            let config = ThumbnailConfig::default();
                            let win_w_async = win_weak_src.clone();

                            thumb_mgr.get_or_load_animated_preview_async(
                                info.media_id.unwrap_or(0),
                                info.full_path.clone(),
                                config,
                                move |res| {
                                    if let Ok(preview) = res {
                                        let _ = win_w_async.upgrade_in_event_loop(move |w| {
                                            let is_still_hovered = CONFLICT_HOVER_TARGET.with(|t| *t.borrow() == Some("source"));
                                            if is_still_hovered {
                                                let timed_frames = VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                                                let timer = VideoPreviewAnimator::start_variable_delay_animation(timed_frames, move |img| {
                                                    let mut src_file = w.get_source_file();
                                                    src_file.thumbnail = img;
                                                    w.set_source_file(src_file);
                                                });
                                                CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                                            }
                                        });
                                    }
                                },
                            );
                        }
                    } else if let Some(media_id) = info.media_id
                        && let Some(images) = VideoPreviewAnimator::load_preview_images(thumb_mgr, media_id)
                    {
                        let timer = VideoPreviewAnimator::start_animation(images, move |img| {
                            if let Some(w) = win_w.upgrade() {
                                let mut src_file = w.get_source_file();
                                src_file.thumbnail = img;
                                w.set_source_file(src_file);
                            }
                        });
                        CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                    }
                } else {
                    CONFLICT_HOVER_TARGET.with(|t| *t.borrow_mut() = None);
                    CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                    if let Some(w) = win_weak_src.upgrade() {
                        let mut src_file = w.get_source_file();
                        src_file.thumbnail = info.thumbnail.clone();
                        w.set_source_file(src_file);
                    }
                }
            });

            // Hover Video & GIF Previews for Destination File Thumbnail
            let win_weak_dst = win.as_weak();
            let dst_info_slot = self.active_dest_info.clone();
            let thumb_slot_dst = self.thumbnails.clone();
            let set_slot_dst = self.settings.clone();

            win.on_destination_hover_changed(move |is_hover| {
                if !set_slot_dst
                    .borrow()
                    .as_ref()
                    .map(|s| s.get().appearance.enable_hover_previews)
                    .unwrap_or(true)
                {
                    return;
                }

                let dst_info_guard = dst_info_slot.borrow();
                let Some(ref info) = *dst_info_guard else {
                    return;
                };

                if is_hover {
                    CONFLICT_HOVER_TARGET.with(|t| *t.borrow_mut() = Some("dest"));
                    CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                    let thumb_guard = thumb_slot_dst.borrow();
                    let Some(thumb_mgr) = thumb_guard.as_ref() else {
                        return;
                    };

                    let win_w = win_weak_dst.clone();

                    if info.format == MediaFormat::Gif {
                        if let Some(media_id) = info.media_id
                            && let Some(preview) = thumb_mgr.try_get_animated_preview(media_id)
                        {
                            let timed_frames = VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                            let timer = VideoPreviewAnimator::start_variable_delay_animation(timed_frames, move |img| {
                                if let Some(w) = win_w.upgrade() {
                                    let mut dst_file = w.get_destination_file();
                                    dst_file.thumbnail = img;
                                    w.set_destination_file(dst_file);
                                }
                            });
                            CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                        } else {
                            let config = ThumbnailConfig::default();
                            let win_w_async = win_weak_dst.clone();

                            thumb_mgr.get_or_load_animated_preview_async(
                                info.media_id.unwrap_or(0),
                                info.full_path.clone(),
                                config,
                                move |res| {
                                    if let Ok(preview) = res {
                                        let _ = win_w_async.upgrade_in_event_loop(move |w| {
                                            let is_still_hovered = CONFLICT_HOVER_TARGET.with(|t| *t.borrow() == Some("dest"));
                                            if is_still_hovered {
                                                let timed_frames = VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                                                let timer = VideoPreviewAnimator::start_variable_delay_animation(timed_frames, move |img| {
                                                    let mut dst_file = w.get_destination_file();
                                                    dst_file.thumbnail = img;
                                                    w.set_destination_file(dst_file);
                                                });
                                                CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                                            }
                                        });
                                    }
                                },
                            );
                        }
                    } else if let Some(media_id) = info.media_id
                        && let Some(images) = VideoPreviewAnimator::load_preview_images(thumb_mgr, media_id)
                    {
                        let timer = VideoPreviewAnimator::start_animation(images, move |img| {
                            if let Some(w) = win_w.upgrade() {
                                let mut dst_file = w.get_destination_file();
                                dst_file.thumbnail = img;
                                w.set_destination_file(dst_file);
                            }
                        });
                        CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                    }
                } else {
                    CONFLICT_HOVER_TARGET.with(|t| *t.borrow_mut() = None);
                    CONFLICT_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                    if let Some(w) = win_weak_dst.upgrade() {
                        let mut dst_file = w.get_destination_file();
                        dst_file.thumbnail = info.thumbnail.clone();
                        w.set_destination_file(dst_file);
                    }
                }
            });

            *d_ref = Some(win);
        }
    }
}

fn convert_details_to_view(details: &FileConflictItemDetails, thumb: Image) -> ConflictFileView {
    let dim_str = if details.width > 0 && details.height > 0 {
        format!("{} x {}", details.width, details.height)
    } else {
        "-".to_string()
    };

    ConflictFileView {
        filename: details.filename.clone().into(),
        folder_path: details.folder_path.clone().into(),
        filesize_str: format_filesize(details.file_size).into(),
        dimensions_str: dim_str.into(),
        date_str: format_timestamp(details.modification_time).into(),
        hash_str: MediaHasher::to_hex(&details.file_hash).into(),
        thumbnail: thumb,
    }
}

/// Cover for a conflict card: the stored thumbnail (L1 cache, then database) whenever one exists. Only if there is
/// none, it is generated with the user's thumbnail settings (including the video position option). Files that are
/// not in the library get a transient thumbnail that is never stored.
fn load_conflict_thumbnail(
    thumbnails: &ThumbnailManager,
    config: &ThumbnailConfig,
    path: &Path,
    media_id: Option<rustview_core::library::models::MediaId>,
) -> Image {
    if let Some(id) = media_id {
        match thumbnails.get_or_load_sync(id, path, config.clone()) {
            Ok(cached) => return slint::Image::from_rgb8(cached.pixels),
            Err(e) => tracing::warn!(media_id = id, error = %e, "[ConflictDialog: Thumbnail] Stored/generated thumbnail unavailable, using transient fallback"),
        }
    }
    generate_transient_thumbnail(config, path)
}

fn generate_transient_thumbnail(config: &ThumbnailConfig, path: &Path) -> Image {
    let generator = ThumbnailGenerator::new();
    let extension_type = MediaFormat::from_path(path).media_type().unwrap_or(MediaType::Image);
    let is_video = rustview_core::media::MetadataExtractor::resolve_media_type(path, extension_type) == MediaType::Video;
    let frame = if is_video {
        generator.generate_video_thumbnails(path, config).map(|r| r.primary_frame)
    } else {
        generator.generate_image_thumbnail(path, config.max_bound, config.scaling_algorithm)
    };

    match frame {
        Ok(f) => rgb8_to_slint_image(&f.rgb_pixels, f.width, f.height),
        Err(e) => {
            tracing::warn!(error = %e, "[ConflictDialog: Thumbnail] Transient thumbnail generation failed");
            Image::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_thumbnail_is_generated_for_images_and_empty_for_missing_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("c.png");
        image::RgbImage::from_pixel(64, 48, image::Rgb([10, 120, 200])).save(&file).expect("write png");
        let config = ThumbnailConfig::default();

        assert!(generate_transient_thumbnail(&config, &file).size().width > 0);
        assert_eq!(generate_transient_thumbnail(&config, &dir.path().join("none.png")).size().width, 0);
    }
}
