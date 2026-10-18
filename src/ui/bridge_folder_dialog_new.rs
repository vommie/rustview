use super::bridge_folder_dialogs::FolderDialogContext;
use super::bridge_media::BridgeMedia;
use super::bridge_status_bar::StatusBarBridge;
use super::browse_folders::BrowseFolders;
use super::{MainWindow, NewFolderDialog};
use crate::library::error::LibraryError;
use crate::library::folder_create::FolderPathPlanner;
use crate::library::models::FolderId;
use crate::settings::LogObscurer;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Controller of the "New Folder" modal dialog: asks for a folder name or a relative path and creates the missing folders below the
/// folder it was opened for.
#[derive(Default, Clone)]
pub struct NewFolderDialogController {
    dialog: Rc<RefCell<Option<NewFolderDialog>>>,
}

/// The text of an error for the dialog (the library errors carry a prefix that only makes sense in logs).
fn dialog_message(error: &LibraryError) -> String {
    match error {
        LibraryError::FileOperationInvalidFilename(message) => message.clone(),
        LibraryError::FileOperationAlreadyExists(message) => message.clone(),
        other => other.to_string(),
    }
}

impl NewFolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the dialog for creating folders below `parent_folder_id`; the cursor is in the input.
    pub fn show(&self, window: &MainWindow, parent_folder_id: FolderId, ctx: FolderDialogContext) {
        window.set_is_context_menu_open(false);
        if BridgeMedia::is_system_busy(window, &ctx.thumbnails, &ctx.state) {
            StatusBarBridge::warn(window, "Cannot create a folder: another operation is already in progress.");
            return;
        }
        let Some(parent) = ctx.library.folders().get_folder(parent_folder_id).ok().flatten() else {
            StatusBarBridge::error(window, "Cannot create a folder: the parent folder is not in the library.");
            return;
        };

        self.ensure_dialog_instantiated();
        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, parent_folder_id, ctx);
            win.set_parent_path(PathBuf::from(&parent.path).to_string_lossy().as_ref().into());
            win.set_folder_path("".into());
            win.set_error_message("".into());

            window.set_is_modal_locked(true);
            let _ = win.show();
            win.invoke_focus_input();
        }
    }

    /// Hides the dialog and releases the modal lock on the main window.
    pub fn close(&self, window: &MainWindow) {
        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    /// Creates the folders. Folders that exist already are skipped without a message; an error keeps the dialog open.
    pub fn execute(&self, parent_folder_id: FolderId, text: &str, ctx: &FolderDialogContext, window: &MainWindow) {
        if BridgeMedia::is_system_busy(window, &ctx.thumbnails, &ctx.state) {
            self.show_error("Another operation is in progress. Try again when it has finished.");
            return;
        }

        match ctx.library.folder_creator().create_path(parent_folder_id, text) {
            Ok(report) => {
                tracing::info!(
                    parent_folder_id,
                    components = report.components.len(),
                    created = report.created_count(),
                    path = %LogObscurer::query(text),
                    "[NewFolderDialog: Execute] Folder path processed"
                );
                self.close(window);
                if report.created_count() > 0 {
                    StatusBarBridge::success(window, format!("Created folder '{}'.", text.trim().trim_matches('/')));
                }
                BrowseFolders::folders_created(window, ctx, parent_folder_id, &report);
            }
            Err(e) => {
                tracing::warn!(parent_folder_id, error = %e, "[NewFolderDialog: Execute] Failed to create the folder");
                self.show_error(&dialog_message(&e));
            }
        }
    }

    fn show_error(&self, message: &str) {
        if let Some(ref win) = *self.dialog.borrow() {
            win.set_error_message(message.into());
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = NewFolderDialog::new().expect("Failed to instantiate NewFolderDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(&self, win: &NewFolderDialog, window: &MainWindow, parent_folder_id: FolderId, ctx: FolderDialogContext) {
        let win_weak = win.as_weak();
        win.on_text_edited(move |typed| {
            let Some(w) = win_weak.upgrade() else { return };
            if typed.trim().is_empty() {
                w.set_error_message("".into());
                return;
            }
            match FolderPathPlanner::parse(&typed) {
                Ok(_) => w.set_error_message("".into()),
                Err(e) => w.set_error_message(dialog_message(&e).into()),
            }
        });

        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        win.on_confirmed(move |text| {
            ctrl_confirm.execute(parent_folder_id, &text, &ctx, &win_confirm);
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
