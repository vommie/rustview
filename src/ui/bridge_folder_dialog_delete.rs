use super::bridge_folder_dialog_missing_delete::MissingFolderDeleteDialogController;
use super::bridge_folder_dialogs::FolderDialogContext;
use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::bridge_status_bar::StatusBarBridge;
use super::{DeleteFolderDialog, MainWindow};
use crate::library::folder_operations::FolderSubtreeMetrics;
use crate::library::models::FolderId;
use crate::settings::LogObscurer;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Specialized controller managing active folder deletion (Trash / Permanent Delete) dialog and execution.
#[derive(Default, Clone)]
pub struct DeleteFolderDialogController {
    dialog: Rc<RefCell<Option<DeleteFolderDialog>>>,
}

impl DeleteFolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the Delete Folder modal dialog with computed subtree metrics.
    pub fn show(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        folder_name: &str,
        ctx: FolderDialogContext,
        missing_dialog: &MissingFolderDeleteDialogController,
    ) {
        window.set_is_context_menu_open(false);

        let Ok(Some(folder_rec)) = ctx.library.folders().get_folder(folder_id) else {
            return;
        };

        let folder_path = PathBuf::from(&folder_rec.path);

        if !folder_path.exists() {
            tracing::warn!(
                folder_id = folder_id,
                path = %LogObscurer::path(&folder_path),
                "[FolderDialog: Delete] Folder path does not exist on disk. Redirecting to MissingFolderDeleteDialog."
            );

            let _ = ctx.library.folders().set_deleted(folder_id, true);
            BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);

            missing_dialog.show(window, folder_id, folder_name, &folder_rec.path, ctx);
            return;
        }

        let metrics = match ctx.library.folder_operations().get_metrics(folder_id) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(folder_id = folder_id, error = %e, "Failed to compute folder metrics for deletion");
                return;
            }
        };

        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, folder_id, ctx);
            self.populate_dialog_fields(win, folder_name, &metrics);

            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the dialog and releases modal lock on the main application window.
    pub fn close(&self, window: &MainWindow) {
        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    /// Executes folder deletion (Trash or Permanent) and synchronizes storage and UI trees.
    pub fn execute(
        &self,
        folder_id: FolderId,
        to_trash: bool,
        ctx: &FolderDialogContext,
        window: &MainWindow,
    ) {
        let metrics_opt = ctx.library.folder_operations().get_metrics(folder_id).ok();
        let fallback = [folder_id];
        let affected_ids = metrics_opt
            .as_ref()
            .map(|m| m.affected_folder_ids.as_slice())
            .unwrap_or(&fallback);

        let successor_target = BridgeFolders::find_visible_successor_folder(
            folder_id,
            affected_ids,
            &ctx.library,
            &ctx.settings,
            &ctx.state,
        );

        match ctx
            .library
            .folder_operations()
            .delete_folder(folder_id, to_trash)
        {
            Ok(report) => {
                BridgeFolders::prune_expanded_folders(
                    &report.deleted_folder_ids,
                    &ctx.settings,
                    &ctx.state,
                );

                self.close(window);

                let action_str = if to_trash {
                    "Moved to Trash"
                } else {
                    "Permanently deleted"
                };
                let msg = format!(
                    "{} {} folder(s) and {} media file(s)",
                    action_str,
                    report.deleted_folder_ids.len(),
                    report.deleted_media_count
                );
                StatusBarBridge::success(window, msg);

                BridgeFolders::navigate_to_successor_if_affected(
                    successor_target,
                    &report.deleted_folder_ids,
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.settings,
                    &ctx.state,
                );

                BridgeInspector::update(window, &ctx.state);
                StatusBarBridge::update_metrics(window, &ctx.state);
            }
            Err(e) => {
                tracing::error!(folder_id = folder_id, error = %e, "Failed to delete folder");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = DeleteFolderDialog::new().expect("Failed to instantiate DeleteFolderDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &DeleteFolderDialog,
        window: &MainWindow,
        folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        let ctrl_trash = self.clone();
        let win_trash = window.clone_strong();
        let ctx_trash = ctx.clone();

        win.on_move_to_trash_confirmed(move || {
            ctrl_trash.execute(folder_id, true, &ctx_trash, &win_trash);
        });

        let ctrl_perm = self.clone();
        let win_perm = window.clone_strong();
        let ctx_perm = ctx;

        win.on_permanent_delete_confirmed(move || {
            ctrl_perm.execute(folder_id, false, &ctx_perm, &win_perm);
        });

        let ctrl_cancel = self.clone();
        let win_cancel = window.clone_strong();
        win.on_cancelled(move || {
            ctrl_cancel.close(&win_cancel);
        });

        let ctrl_close = self.clone();
        let win_close = window.clone_strong();
        win.window().on_close_requested(move || {
            ctrl_close.close(&win_close);
            CloseRequestResponse::HideWindow
        });
    }

    fn populate_dialog_fields(
        &self,
        win: &DeleteFolderDialog,
        folder_name: &str,
        metrics: &FolderSubtreeMetrics,
    ) {
        win.set_folder_name(folder_name.into());
        win.set_subfolders_count(metrics.subfolders_count as i32);
        win.set_files_count(metrics.media_files_count as i32);
        win.set_error_message("".into());
    }
}
