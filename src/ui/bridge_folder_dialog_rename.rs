use super::bridge_folder_dialogs::FolderDialogContext;
use super::bridge_folders::BridgeFolders;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::{MainWindow, RenameFolderDialog};
use crate::library::models::FolderId;
use parking_lot::RwLock;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Specialized controller managing the Rename Folder modal dialog lifecycle and execution.
#[derive(Default, Clone)]
pub struct RenameFolderDialogController {
    dialog: Rc<RefCell<Option<RenameFolderDialog>>>,
}

impl RenameFolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the Rename Folder modal dialog with pre-populated name and clears any prior errors.
    pub fn show(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        current_name: &str,
        ctx: FolderDialogContext,
    ) {
        window.set_is_context_menu_open(false);
        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, folder_id, ctx);
            self.populate_dialog_fields(win, current_name);

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

    /// Executes atomic folder renaming on disk and updates LMDB path indices recursively.
    pub fn execute(
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
                let is_active = self.update_in_memory_state(folder_id, &updated.path, &ctx.state);

                self.close(window);
                window.set_status_text(format!("Renamed folder to '{}'", new_name).into());

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                if is_active {
                    BridgeMedia::reload(
                        window,
                        &ctx.library,
                        &ctx.thumbnails,
                        &ctx.settings,
                        &ctx.state,
                        true,
                        true,
                    );
                }
            }
            Err(e) => {
                tracing::warn!(folder_id = folder_id, error = %e, "Failed to rename folder");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = RenameFolderDialog::new().expect("Failed to instantiate RenameFolderDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &RenameFolderDialog,
        window: &MainWindow,
        folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        let ctx_confirm = ctx;

        win.on_confirmed(move |name| {
            ctrl_confirm.execute(folder_id, &name, &ctx_confirm, &win_confirm);
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

    fn populate_dialog_fields(&self, win: &RenameFolderDialog, current_name: &str) {
        win.set_initial_name(current_name.into());
        win.set_new_name(current_name.into());
        win.set_error_message("".into());
    }

    fn update_in_memory_state(
        &self,
        folder_id: FolderId,
        new_path_str: &str,
        state: &RwLock<UiState>,
    ) -> bool {
        let is_active = state.read().current_folder_id == Some(folder_id);
        if is_active {
            let mut st = state.write();
            st.current_folder_path = Some(PathBuf::from(new_path_str));
        }
        is_active
    }
}
