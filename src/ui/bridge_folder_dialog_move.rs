use super::bridge_folder_dialogs::FolderDialogContext;
use super::bridge_folders::BridgeFolders;
use super::bridge_media::BridgeMedia;
use super::bridge_status_bar::StatusBarBridge;
use super::{MainWindow, MoveFolderToDialog};
use crate::library::models::FolderId;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// Specialized controller managing the Move/Reparent Folder modal dialog and execution.
#[derive(Default, Clone)]
pub struct MoveFolderDialogController {
    dialog: Rc<RefCell<Option<MoveFolderToDialog>>>,
}

impl MoveFolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the Move Folder dialog confirming relocation under destination folder.
    pub fn show(
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

        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, src_folder_id, dst_folder_id, ctx);

            win.set_source_folder_name(src_name.into());
            win.set_target_folder_name(dst_name.into());
            win.set_subfolders_count(metrics.subfolders_count as i32);
            win.set_files_count(metrics.media_files_count as i32);
            win.set_error_message("".into());

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

    /// Executes physical relocation on disk and rewrites LMDB folder path keys recursively.
    pub fn execute(
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

                self.close(window);
                StatusBarBridge::success(window, "Folder moved successfully.");

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                BridgeMedia::reload(
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.settings,
                    &ctx.state,
                    false,
                    true,
                );
                StatusBarBridge::update_metrics(window, &ctx.state);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to move folder");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = MoveFolderToDialog::new().expect("Failed to instantiate MoveFolderToDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &MoveFolderToDialog,
        window: &MainWindow,
        src_folder_id: FolderId,
        dst_folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        let ctx_confirm = ctx;

        win.on_confirmed(move || {
            ctrl_confirm.execute(src_folder_id, dst_folder_id, &ctx_confirm, &win_confirm);
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
}
