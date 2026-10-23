//! The step-by-step runner of a batch copy or move started by a drop (conflicts, progress, final report).

use crate::ui::MainWindow;
use crate::ui::file_dialogs::conflict::{ConflictDialogParams, FileOpsConflictDialogController};
use crate::ui::file_dialogs::missing::{MissingFileDialogController, MissingFileItem};
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::error::LibraryError;
use rustview_core::library::file_operations::FileOperations;
use rustview_core::library::models::{
    BatchMoveSummary, FileConflictReport, FileConflictResolution, FileTransferKind, FolderId, MediaId,
};
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{ComponentHandle, Timer};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

pub(super) struct BatchTransferRunner {
    pub(super) window: MainWindow,
    pub(super) library: Arc<Library>,
    pub(super) thumbnails: Arc<ThumbnailManager>,
    pub(super) settings: Arc<SettingsManager>,
    pub(super) state: Arc<RwLock<UiState>>,
    pub(super) conflict_dialog: FileOpsConflictDialogController,
    pub(super) missing_dialog: MissingFileDialogController,
    pub(super) target_folder_id: FolderId,
    pub(super) target_folder_path: PathBuf,
    pub(super) target_folder_name: String,
    pub(super) items: Vec<MediaId>,
    pub(super) current_index: usize,
    pub(super) transfer_kind: FileTransferKind,
    pub(super) summary: BatchMoveSummary,
    pub(super) moved_media_ids: HashSet<MediaId>,
    pub(super) remembered_resolution: Option<FileConflictResolution>,
    pub(super) start_time: Instant,
    /// Why the first item failed, for the final message.
    pub(super) first_failure: Option<String>,
}

impl BatchTransferRunner {
    pub(super) fn step(mut self) {
        if self.current_index >= self.items.len() {
            self.finish();
            return;
        }

        let media_id = self.items[self.current_index];

        if self.library.media().get(media_id).ok().flatten().is_none() {
            self.current_index += 1;
            Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            return;
        }

        self.summary.total_requested += 1;
        let t_step = Instant::now();

        match self.library.file_operations().check_transfer_conflict(
            media_id,
            self.target_folder_id,
            self.transfer_kind,
        ) {
            Ok(Some(conflict_report)) => {
                if let Some(res) = self.remembered_resolution {
                    self.apply_resolution(media_id, &conflict_report, res);
                } else {
                    let conflict_for_show = conflict_report.clone();
                    let conflict_for_closure = conflict_report.clone();
                    let runner_rc = Rc::new(RefCell::new(Some(self)));
                    let runner_for_resolve = runner_rc.clone();

                    let win = runner_rc.borrow().as_ref().unwrap().window.clone_strong();
                    let thumb = runner_rc.borrow().as_ref().unwrap().thumbnails.clone();
                    let set = runner_rc.borrow().as_ref().unwrap().settings.clone();
                    let dlg = runner_rc.borrow().as_ref().unwrap().conflict_dialog.clone();
                    let kind = runner_rc.borrow().as_ref().unwrap().transfer_kind;
                    let total_items = runner_rc.borrow().as_ref().unwrap().items.len();
                    let current_idx = runner_rc.borrow().as_ref().unwrap().current_index;

                    dlg.show(
                        ConflictDialogParams {
                            window: &win,
                            thumbnails: &thumb,
                            settings: &set,
                            report: &conflict_for_show,
                            conflict_index: current_idx + 1,
                            total_conflicts: total_items,
                            transfer_kind: kind,
                        },
                        move |resolution, apply_to_all| {
                            if let Some(mut runner) = runner_for_resolve.borrow_mut().take() {
                                if apply_to_all {
                                    runner.remembered_resolution = Some(resolution);
                                }
                                runner.apply_resolution(media_id, &conflict_for_closure, resolution);
                            }
                        },
                    );
                }
            }
            Ok(None) => {
                let exec_res = match self.transfer_kind {
                    FileTransferKind::Move => {
                        self.library
                            .file_operations()
                            .move_single_media(media_id, self.target_folder_id, None, false)
                    }
                    FileTransferKind::Copy => {
                        self.library
                            .file_operations()
                            .copy_single_media(media_id, self.target_folder_id, None, false)
                    }
                };

                match exec_res {
                    Ok(rec) => {
                        tracing::debug!(
                            media = %LogObscurer::media(Some(media_id), &rec.filename),
                            transfer_kind = ?self.transfer_kind,
                            step_time_us = t_step.elapsed().as_micros(),
                            "[BatchTransfer: Direct] Successfully processed media item without collision"
                        );
                        self.summary.moved_count += 1;
                        if self.transfer_kind == FileTransferKind::Move {
                            self.moved_media_ids.insert(media_id);
                        }
                    }
                    Err(LibraryError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                        self.note_failure(&e);
                        let missing_path = self.resolve_media_path(media_id);
                        self.missing_dialog.show(
                            &self.window,
                            if self.transfer_kind == FileTransferKind::Move {
                                "Move File"
                            } else {
                                "Copy File"
                            },
                            vec![MissingFileItem {
                                filename: format!("Media #{}", media_id),
                                path: missing_path,
                            }],
                        );
                    }
                    Err(e) => {
                        tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: Error] Direct transfer failed");
                        self.note_failure(&e);
                    }
                }
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            Err(e) => {
                tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: Error] Failed to check conflict");
                self.note_failure(&e);
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
        }
    }

    fn apply_resolution(
        mut self,
        media_id: MediaId,
        conflict: &FileConflictReport,
        resolution: FileConflictResolution,
    ) {
        let t_res = Instant::now();
        match resolution {
            FileConflictResolution::Skip => {
                tracing::info!(
                    media_id = media_id,
                    file = %LogObscurer::filename(&conflict.source.filename),
                    elapsed_us = t_res.elapsed().as_micros(),
                    "[BatchTransfer: Resolution] Action: SKIP. File left untouched in source directory."
                );
                self.summary.skipped_count += 1;
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            FileConflictResolution::AutoRename => {
                let auto_name =
                    FileOperations::generate_unique_filename(&self.target_folder_path, &conflict.source.filename);
                let exec_res = match self.transfer_kind {
                    FileTransferKind::Move => self.library.file_operations().move_single_media(
                        media_id,
                        self.target_folder_id,
                        Some(&auto_name),
                        false,
                    ),
                    FileTransferKind::Copy => self.library.file_operations().copy_single_media(
                        media_id,
                        self.target_folder_id,
                        Some(&auto_name),
                        false,
                    ),
                };

                match exec_res {
                    Ok(_) => {
                        self.summary.auto_renamed_count += 1;
                        self.summary.moved_count += 1;
                        if self.transfer_kind == FileTransferKind::Move {
                            self.moved_media_ids.insert(media_id);
                        }
                    }
                    Err(e) => {
                        tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: AutoRename Error] Failed to transfer auto-renamed file");
                        self.note_failure(&e);
                    }
                }
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            FileConflictResolution::Overwrite => {
                let exec_res = match self.transfer_kind {
                    FileTransferKind::Move => {
                        self.library
                            .file_operations()
                            .move_single_media(media_id, self.target_folder_id, None, true)
                    }
                    FileTransferKind::Copy => {
                        self.library
                            .file_operations()
                            .copy_single_media(media_id, self.target_folder_id, None, true)
                    }
                };

                match exec_res {
                    Ok(_) => {
                        self.summary.overwritten_count += 1;
                        self.summary.moved_count += 1;
                        if self.transfer_kind == FileTransferKind::Move {
                            self.moved_media_ids.insert(media_id);
                        }
                    }
                    Err(e) => {
                        tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: Overwrite Error] Failed to overwrite destination file");
                        self.note_failure(&e);
                    }
                }
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            FileConflictResolution::Cancel => {
                self.finish();
            }
        }
    }

    fn note_failure(&mut self, error: &dyn std::fmt::Display) {
        self.summary.failed_count += 1;
        self.first_failure.get_or_insert_with(|| error.to_string());
    }

    fn resolve_media_path(&self, media_id: MediaId) -> PathBuf {
        let st = self.state.read();
        if let Some(item) = st.all_folder_media.find(media_id) {
            return item.full_path();
        }
        drop(st);

        self.library
            .resolve_media_path(media_id)
            .unwrap_or_else(|_| PathBuf::from(format!("Media_{}", media_id)))
    }

    fn finish(self) {
        let elapsed_ms = self.start_time.elapsed().as_millis();

        {
            let mut st = self.state.write();
            st.invalidate_folder_hierarchy();
            if self.transfer_kind == FileTransferKind::Move {
                st.file_selection.deselect_many(&self.moved_media_ids);
            }
        }

        BridgeFolders::reload_tree(&self.window, &self.library, &self.settings, &self.state);
        BridgeMedia::reload(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
            false,
            true,
        );
        BridgeInspector::update(&self.window, &self.state);
        BridgeTags::sync_tag_selection(&self.window, &self.library, &self.state);

        let verb_past = if self.transfer_kind == FileTransferKind::Move {
            "Moved"
        } else {
            "Copied"
        };

        let mut details = Vec::new();
        if self.summary.auto_renamed_count > 0 {
            details.push(format!("{} auto-renamed", self.summary.auto_renamed_count));
        }
        if self.summary.overwritten_count > 0 {
            details.push(format!("{} overwritten", self.summary.overwritten_count));
        }
        if self.summary.skipped_count > 0 {
            details.push(format!("{} skipped", self.summary.skipped_count));
        }
        if self.summary.failed_count > 0 {
            let reason = self
                .first_failure
                .as_deref()
                .map(|r| format!(": {r}"))
                .unwrap_or_default();
            details.push(format!("{} failed{reason}", self.summary.failed_count));
        }

        let detail_str = if details.is_empty() {
            String::new()
        } else {
            format!(" ({})", details.join(", "))
        };

        let msg = format!(
            "{} {} file(s) to '{}' in {}ms{}",
            verb_past, self.summary.moved_count, self.target_folder_name, elapsed_ms, detail_str
        );

        if self.summary.failed_count > 0 {
            StatusBarBridge::warn(&self.window, msg);
        } else {
            StatusBarBridge::success(&self.window, msg);
        }
        StatusBarBridge::update_metrics(&self.window, &self.state);
    }
}
