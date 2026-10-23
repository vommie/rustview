use crate::ui::folder_dialogs::reborn_warn::RebornFolderWarnDialogController;
use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::{MainWindow, RemoveOrphanedFolderDialog};
use rustview_core::library::folder_operations::FolderSubtreeMetrics;
use rustview_core::library::models::FolderId;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// Specialized controller managing the Remove Missing/Orphaned Folder from Database modal dialog.
#[derive(Default, Clone)]
pub struct RemoveOrphanedFolderDialogController {
    dialog: Rc<RefCell<Option<RemoveOrphanedFolderDialog>>>,
}

impl RemoveOrphanedFolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the confirmation dialog to remove an orphaned folder from database after verifying physical non-existence.
    pub fn show(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        folder_name: &str,
        ctx: FolderDialogContext,
        reborn_warn: &RebornFolderWarnDialogController,
    ) {
        window.set_is_context_menu_open(false);

        match ctx
            .library
            .folder_operations()
            .check_and_restore_if_exists(folder_id)
        {
            Ok(true) => {
                let path_str = ctx
                    .library
                    .folders()
                    .get_folder(folder_id)
                    .ok()
                    .flatten()
                    .map(|f| f.path)
                    .unwrap_or_default();

                BridgeFolders::reload_tree_after_restore(window, &ctx.library, &ctx.settings, &ctx.state);
                StatusBarBridge::info(window, "Folder re-detected on disk and restored.");
                reborn_warn.show(window, folder_name, &path_str);
                return;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!(
                    folder_id = folder_id,
                    error = %e,
                    "Failed to verify physical folder existence"
                );
                return;
            }
        }

        let metrics = match ctx.library.folder_operations().get_metrics(folder_id) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(
                    folder_id = folder_id,
                    error = %e,
                    "Failed to compute folder metrics for orphaned removal"
                );
                return;
            }
        };

        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, folder_id, ctx, reborn_warn);
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

    /// Executes database purge of the orphaned folder subtree after final safety verification.
    pub fn execute(
        &self,
        folder_id: FolderId,
        ctx: &FolderDialogContext,
        window: &MainWindow,
        reborn_warn: &RebornFolderWarnDialogController,
    ) {
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

                BridgeFolders::reload_tree_after_restore(window, &ctx.library, &ctx.settings, &ctx.state);
                StatusBarBridge::info(window, "Folder re-detected on disk and restored.");
                reborn_warn.show(window, &f_name, &path_str);
                return;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!(error = %e, "Failed to verify folder existence during purge");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
                return;
            }
        }

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
            .purge_folder_from_db(folder_id)
        {
            Ok(report) => {
                for &mid in &report.deleted_media_ids {
                    ctx.tab_manager.notify_media_deleted(mid);
                }

                BridgeFolders::prune_expanded_folders(
                    &report.deleted_folder_ids,
                    &ctx.settings,
                    &ctx.state,
                );

                self.close(window);

                let msg = format!(
                    "Removed {} missing folder(s) and {} cached media file(s) from database",
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
                BridgeTags::sync_tag_selection(window, &ctx.library, &ctx.state);
                BridgeViewerTabs::sync_ui(
                    window,
                    &ctx.tab_manager,
                    &ctx.library,
                    &ctx.settings,
                    &ctx.state,
                );
                StatusBarBridge::update_metrics(window, &ctx.state);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to purge orphaned folder from database");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = RemoveOrphanedFolderDialog::new()
                .expect("Failed to instantiate RemoveOrphanedFolderDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &RemoveOrphanedFolderDialog,
        window: &MainWindow,
        folder_id: FolderId,
        ctx: FolderDialogContext,
        reborn_warn: &RebornFolderWarnDialogController,
    ) {
        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        let ctx_confirm = ctx;
        let warn_confirm = reborn_warn.clone();

        win.on_confirmed(move || {
            ctrl_confirm.execute(folder_id, &ctx_confirm, &win_confirm, &warn_confirm);
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
        win: &RemoveOrphanedFolderDialog,
        folder_name: &str,
        metrics: &FolderSubtreeMetrics,
    ) {
        let path_str = metrics.path.to_string_lossy().to_string();
        win.set_folder_name(folder_name.into());
        win.set_folder_path(path_str.into());
        win.set_subfolders_count(metrics.subfolders_count as i32);
        win.set_files_count(metrics.media_files_count as i32);
        win.set_error_message("".into());
    }
}
