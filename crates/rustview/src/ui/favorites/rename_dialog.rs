use crate::ui::favorites::BridgeFavorites;
use crate::ui::favorites::manager::FavoriteManager;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::{FavoriteRenameDialog, MainWindow};
use parking_lot::RwLock;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// Specialized controller managing the Rename Favorite modal dialog lifecycle and execution.
#[derive(Default, Clone)]
pub struct FavoriteRenameDialogController {
    dialog: Rc<RefCell<Option<FavoriteRenameDialog>>>,
}

impl FavoriteRenameDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the modal dialog to rename a favorite item's virtual display alias.
    pub fn show(
        &self,
        window: &MainWindow,
        current_alias: &str,
        target_path: &Path,
        favorites: Arc<RwLock<FavoriteManager>>,
    ) {
        window.set_is_context_menu_open(false);
        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            let ctrl_confirm = self.clone();
            let win_confirm = window.clone_strong();
            let path_confirm = target_path.to_path_buf();
            let favs_confirm = favorites;

            win.on_confirmed(move |new_alias| {
                ctrl_confirm.execute(&win_confirm, &favs_confirm, &path_confirm, &new_alias);
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

            win.set_initial_alias(current_alias.into());
            win.set_new_alias(current_alias.into());
            win.set_target_path(target_path.to_string_lossy().to_string().into());
            win.set_error_message("".into());

            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the dialog and releases the modal window lock.
    pub fn close(&self, window: &MainWindow) {
        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    /// Checks if the dialog is currently visible.
    pub fn is_open(&self) -> bool {
        self.dialog.borrow().as_ref().is_some_and(|w| w.window().is_visible())
    }

    fn execute(&self, window: &MainWindow, favorites: &Arc<RwLock<FavoriteManager>>, path: &Path, new_alias: &str) {
        let trimmed = new_alias.trim();
        if trimmed.is_empty() {
            if let Some(ref win) = *self.dialog.borrow() {
                win.set_error_message("Favorite alias cannot be empty.".into());
            }
            return;
        }

        match BridgeFavorites::rename_favorite(window, favorites, path, trimmed) {
            Ok(()) => {
                self.close(window);
                StatusBarBridge::success(window, format!("Renamed favorite to '{trimmed}'"));
            }
            Err(e) => {
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.into());
                }
            }
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = FavoriteRenameDialog::new().expect("Failed to instantiate FavoriteRenameDialog");
            *d_ref = Some(win);
        }
    }
}
