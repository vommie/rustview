use super::bridge_folder_dialog_reborn_warn::RebornFolderWarnDialogController;
use super::bridge_folder_dialogs::FolderDialogContext;
use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::{MainWindow, MissingFolderDeleteDialog};
use crate::library::folder_operations::FolderSubtreeMetrics;
use crate::library::models::FolderId;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// Specialized controller managing the dialog when attempting to delete a folder that is already missing from disk.
#[derive(Default, Clone)]
pub struct MissingFolderDeleteDialogController {
    dialog: Rc<RefCell<Option<MissingFolderDeleteDialog>>>,
}

impl MissingFolderDeleteDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the missing folder delete dialog informing the user that the folder does not exist on disk and offering database purge.
    pub fn show(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        folder_name: &str,
        folder_path: &str,
        ctx: FolderDialogContext,
    ) {
        window.set_is_context_menu_open(false);

        let metrics = match ctx.library.folder_operations().get_metrics(folder_id) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(
                    folder_id = folder_id,
                    error = %e,
                    "Failed to compute folder metrics for missing folder delete"
                );
                return;
            }
        };

        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, folder_id, ctx);
            self.populate_dialog_fields(win, folder_name, folder_path, &metrics);

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

    /// Executes database cleanup for the missing folder and its descendants.
    pub fn execute(
        &self,
        folder_id: FolderId,
        ctx: &FolderDialogContext,
        window: &MainWindow,
    ) {
        // Safety verification: Check if the folder physically reappeared before purging
        match ctx
            .library
            .folder_operations()
            .check_and_restore_if_exists(folder_id)
        {
            Ok(true) => {
                self.close(window);
                let (f_name, path_str) = ctx
                    .library
                    .folders()
                    .get_folder(folder_id)
                    .ok()
                    .flatten()
                    .map(|f| {
                        let name = Path::new(&f.path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| f.path.clone());
                        (name, f.path)
                    })
                    .unwrap_or_else(|| ("Folder".to_string(), "".to_string()));

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                window.set_status_text("Folder re-detected on disk and restored.".into());

                let reborn_ctrl = RebornFolderWarnDialogController::new();
                reborn_ctrl.show(window, &f_name, &path_str);
                return;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!(error = %e, "Failed to verify folder existence during missing folder purge");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
                return;
            }
        }

        match ctx
            .library
            .folder_operations()
            .purge_folder_from_db(folder_id)
        {
            Ok(report) => {
                BridgeFolders::prune_expanded_folders(
                    &report.deleted_folder_ids,
                    &ctx.settings,
                    &ctx.state,
                );

                BridgeFolders::fallback_to_root_if_affected(
                    &report.deleted_folder_ids,
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.settings,
                    &ctx.state,
                );

                self.close(window);

                window.set_status_text(
                    format!(
                        "Cleaned {} missing folder(s) and {} cached media file(s) from database",
                        report.deleted_folder_ids.len(),
                        report.deleted_media_count
                    )
                    .into(),
                );

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                BridgeInspector::update(window, &ctx.state);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to purge missing folder from database");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = MissingFolderDeleteDialog::new()
                .expect("Failed to instantiate MissingFolderDeleteDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &MissingFolderDeleteDialog,
        window: &MainWindow,
        folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        let ctx_confirm = ctx;

        win.on_clean_confirmed(move || {
            ctrl_confirm.execute(folder_id, &ctx_confirm, &win_confirm);
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
        win: &MissingFolderDeleteDialog,
        folder_name: &str,
        folder_path: &str,
        metrics: &FolderSubtreeMetrics,
    ) {
        win.set_folder_name(folder_name.into());
        win.set_folder_path(folder_path.into());
        win.set_subfolders_count(metrics.subfolders_count as i32);
        win.set_files_count(metrics.media_files_count as i32);
        win.set_error_message("".into());
    }
}
