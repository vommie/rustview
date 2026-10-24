use crate::ui::{MainWindow, RebornFolderDeleteWarnDialog};
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::rc::Rc;

/// Specialized controller managing the Reborn Folder Warning dialog.
#[derive(Default, Clone)]
pub struct RebornFolderWarnDialogController {
    dialog: Rc<RefCell<Option<RebornFolderDeleteWarnDialog>>>,
}

impl RebornFolderWarnDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the warning dialog notifying user that a folder was re-detected on disk.
    pub fn show(&self, window: &MainWindow, folder_name: &str, folder_path: &str) {
        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window);
            win.set_folder_name(folder_name.into());
            win.set_folder_path(folder_path.into());

            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the dialog and releases modal lock on the main window.
    pub fn close(&self, window: &MainWindow) {
        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = RebornFolderDeleteWarnDialog::new().expect("Failed to instantiate RebornFolderDeleteWarnDialog");
            crate::ui::dialog_fit::install(&win);
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(&self, win: &RebornFolderDeleteWarnDialog, window: &MainWindow) {
        let ctrl_ack = self.clone();
        let win_ack = window.clone_strong();
        win.on_acknowledged(move || {
            ctrl_ack.close(&win_ack);
        });

        let ctrl_close = self.clone();
        let win_close = window.clone_strong();
        win.window().on_close_requested(move || {
            ctrl_close.close(&win_close);
            CloseRequestResponse::HideWindow
        });
    }
}
