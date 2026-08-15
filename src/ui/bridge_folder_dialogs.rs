use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::{DeleteFolderDialog, MainWindow, MoveFolderToDialog, RenameFolderDialog};
use crate::library::Library;
use crate::library::models::FolderId;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

/// Context bundle grouping shared application subsystems for folder dialog operations.
#[derive(Clone)]
pub struct FolderDialogContext {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub settings: Arc<SettingsManager>,
    pub state: Arc<RwLock<UiState>>,
}

/// Dedicated controller managing modal folder rename, delete (trash/permanent), and move dialogs.
#[derive(Default, Clone)]
pub struct FolderDialogController {
    rename_dialog: Rc<RefCell<Option<RenameFolderDialog>>>,
    delete_dialog: Rc<RefCell<Option<DeleteFolderDialog>>>,
    move_dialog: Rc<RefCell<Option<MoveFolderToDialog>>>,
}

impl FolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    // =========================================================================
    // 1. Rename Folder Dialog
    // =========================================================================

    pub fn show_rename(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        current_name: &str,
        ctx: FolderDialogContext,
    ) {
        window.set_is_context_menu_open(false);
        let mut d_ref = self.rename_dialog.borrow_mut();
        if d_ref.is_none() {
            let win = RenameFolderDialog::new().expect("Failed to instantiate RenameFolderDialog");
            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            let ctrl_confirm = self.clone();
            let win_confirm = window.clone_strong();
            let ctx_confirm = ctx;

            win.on_confirmed(move |name| {
                ctrl_confirm.execute_rename(folder_id, &name, &ctx_confirm, &win_confirm);
            });

            let ctrl_cancel = self.clone();
            let win_cancel = window.clone_strong();
            win.on_cancelled(move || {
                ctrl_cancel.close_rename(&win_cancel);
            });

            let ctrl_close = self.clone();
            let win_close = window.clone_strong();
            win.window().on_close_requested(move || {
                ctrl_close.close_rename(&win_close);
                CloseRequestResponse::HideWindow
            });

            win.set_initial_name(current_name.into());
            win.set_new_name(current_name.into());
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    pub fn close_rename(&self, window: &MainWindow) {
        if let Some(ref win) = *self.rename_dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    pub fn execute_rename(
        &self,
        folder_id: FolderId,
        new_name: &str,
        ctx: &FolderDialogContext,
        window: &MainWindow,
    ) {
        match ctx
            .library
            .folder_operations()
            .rename_folder(folder_id, new_name)
        {
            Ok(updated) => {
                let is_active = ctx.state.read().current_folder_id == Some(folder_id);
                if is_active {
                    let mut st = ctx.state.write();
                    st.current_folder_path = Some(PathBuf::from(&updated.path));
                }

                self.close_rename(window);
                window.set_status_text(format!("Renamed folder to '{}'", new_name).into());

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                if is_active {
                    BridgeMedia::reload(
                        window,
                        &ctx.library,
                        &ctx.thumbnails,
                        &ctx.state,
                        true,
                        true,
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to rename folder");
                if let Some(ref win) = *self.rename_dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    // =========================================================================
    // 2. Delete Folder Dialog (Trash vs. Permanent)
    // =========================================================================

    pub fn show_delete(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        folder_name: &str,
        ctx: FolderDialogContext,
    ) {
        window.set_is_context_menu_open(false);
        let metrics = match ctx.library.folder_operations().get_metrics(folder_id) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(error = %e, "Failed to compute folder metrics for deletion");
                return;
            }
        };

        let mut d_ref = self.delete_dialog.borrow_mut();
        if d_ref.is_none() {
            let win = DeleteFolderDialog::new().expect("Failed to instantiate DeleteFolderDialog");
            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            let ctrl_trash = self.clone();
            let win_trash = window.clone_strong();
            let ctx_trash = ctx.clone();

            win.on_move_to_trash_confirmed(move || {
                ctrl_trash.execute_delete(folder_id, true, &ctx_trash, &win_trash);
            });

            let ctrl_perm = self.clone();
            let win_perm = window.clone_strong();
            let ctx_perm = ctx;

            win.on_permanent_delete_confirmed(move || {
                ctrl_perm.execute_delete(folder_id, false, &ctx_perm, &win_perm);
            });

            let ctrl_cancel = self.clone();
            let win_cancel = window.clone_strong();
            win.on_cancelled(move || {
                ctrl_cancel.close_delete(&win_cancel);
            });

            let ctrl_close = self.clone();
            let win_close = window.clone_strong();
            win.window().on_close_requested(move || {
                ctrl_close.close_delete(&win_close);
                CloseRequestResponse::HideWindow
            });

            win.set_folder_name(folder_name.into());
            win.set_subfolders_count(metrics.subfolders_count as i32);
            win.set_files_count(metrics.media_files_count as i32);
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    pub fn close_delete(&self, window: &MainWindow) {
        if let Some(ref win) = *self.delete_dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    pub fn execute_delete(
        &self,
        folder_id: FolderId,
        to_trash: bool,
        ctx: &FolderDialogContext,
        window: &MainWindow,
    ) {
        match ctx
            .library
            .folder_operations()
            .delete_folder(folder_id, to_trash)
        {
            Ok(report) => {
                let is_current = ctx
                    .state
                    .read()
                    .current_folder_id
                    .map(|id| report.deleted_folder_ids.contains(&id))
                    .unwrap_or(false);

                // Remove exclusively deleted folder IDs from expansion set (preserving root & sibling branches)
                {
                    let mut st = ctx.state.write();
                    for id in &report.deleted_folder_ids {
                        st.expanded_folder_ids.remove(id);
                    }
                    BridgeFolders::persist_expanded_folders(&ctx.settings, &st.expanded_folder_ids);
                }

                // If currently active folder was deleted, gracefully switch back to root
                if is_current {
                    let root_setting = ctx.settings.get().general.root_folder;
                    if let Some(ref root_p) = root_setting
                        && let Ok(Some(root_rec)) = ctx.library.folders().get_folder_by_path(root_p)
                    {
                        let raw_media = ctx
                            .library
                            .media()
                            .get_by_folder(root_rec.id)
                            .unwrap_or_default();
                        let indexed = BridgeMedia::preindex(&ctx.library, raw_media, root_p);

                        {
                            let mut st = ctx.state.write();
                            st.current_folder_id = Some(root_rec.id);
                            st.current_folder_path = Some(root_p.clone());
                            st.all_folder_media = indexed;
                            st.folder_thumbnails.clear();
                            st.active_indices.clear();
                            st.file_selection.clear();
                            st.last_executed_query.clear();
                        }

                        {
                            let mut s = ctx.settings.get();
                            if s.general.remember_last_folder {
                                s.general.last_folder_path = Some(root_p.clone());
                                ctx.settings.save_quiet(&s);
                            }
                        }

                        BridgeMedia::reload(
                            window,
                            &ctx.library,
                            &ctx.thumbnails,
                            &ctx.state,
                            true,
                            false,
                        );
                    }
                }

                self.close_delete(window);

                let action_str = if to_trash {
                    "Moved to Trash"
                } else {
                    "Permanently deleted"
                };
                window.set_status_text(
                    format!(
                        "{} {} folder(s) and {} media file(s)",
                        action_str,
                        report.deleted_folder_ids.len(),
                        report.deleted_media_count
                    )
                    .into(),
                );

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                BridgeInspector::update(window, &ctx.state);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to delete folder");
                if let Some(ref win) = *self.delete_dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    // =========================================================================
    // 3. Move Folder Dialog
    // =========================================================================

    pub fn show_move(
        &self,
        window: &MainWindow,
        src_folder_id: FolderId,
        dst_folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        window.set_is_context_menu_open(false);
        let (src_name, metrics) = match (
            ctx.library.folders().get_folder(src_folder_id),
            ctx.library.folder_operations().get_metrics(src_folder_id),
        ) {
            (Ok(Some(src)), Ok(m)) => {
                let name = Path::new(&src.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or(src.path);
                (name, m)
            }
            _ => return,
        };

        let dst_name = match ctx.library.folders().get_folder(dst_folder_id) {
            Ok(Some(dst)) => Path::new(&dst.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(dst.path),
            _ => return,
        };

        let mut d_ref = self.move_dialog.borrow_mut();
        if d_ref.is_none() {
            let win = MoveFolderToDialog::new().expect("Failed to instantiate MoveFolderToDialog");
            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            let ctrl_confirm = self.clone();
            let win_confirm = window.clone_strong();
            let ctx_confirm = ctx;

            win.on_confirmed(move || {
                ctrl_confirm.execute_move(src_folder_id, dst_folder_id, &ctx_confirm, &win_confirm);
            });

            let ctrl_cancel = self.clone();
            let win_cancel = window.clone_strong();
            win.on_cancelled(move || {
                ctrl_cancel.close_move(&win_cancel);
            });

            let ctrl_close = self.clone();
            let win_close = window.clone_strong();
            win.window().on_close_requested(move || {
                ctrl_close.close_move(&win_close);
                CloseRequestResponse::HideWindow
            });

            win.set_source_folder_name(src_name.into());
            win.set_target_folder_name(dst_name.into());
            win.set_subfolders_count(metrics.subfolders_count as i32);
            win.set_files_count(metrics.media_files_count as i32);
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    pub fn close_move(&self, window: &MainWindow) {
        if let Some(ref win) = *self.move_dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    pub fn execute_move(
        &self,
        src_folder_id: FolderId,
        dst_folder_id: FolderId,
        ctx: &FolderDialogContext,
        window: &MainWindow,
    ) {
        match ctx
            .library
            .folder_operations()
            .move_folder(src_folder_id, dst_folder_id)
        {
            Ok(_report) => {
                {
                    let mut st = ctx.state.write();
                    st.expanded_folder_ids.insert(dst_folder_id);
                    BridgeFolders::persist_expanded_folders(&ctx.settings, &st.expanded_folder_ids);
                }

                self.close_move(window);
                window.set_status_text("Folder moved successfully.".into());

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                BridgeMedia::reload(
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.state,
                    false,
                    true,
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to move folder");
                if let Some(ref win) = *self.move_dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }
}
