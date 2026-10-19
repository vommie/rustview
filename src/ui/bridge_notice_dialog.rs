use super::{MainWindow, NoticeDialog};
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::rc::Rc;

/// What a notice says: a heading, the message, optional lines that belong to it and a closing remark.
#[derive(Debug, Clone, Default)]
pub struct NoticeContent {
    pub window_title: String,
    pub heading: String,
    pub message: String,
    pub details: String,
    pub footnote: String,
}

/// Controller of the notice dialog: tells the user why something was refused or only partly done.
#[derive(Default, Clone)]
pub struct NoticeDialogController {
    dialog: Rc<RefCell<Option<NoticeDialog>>>,
}

impl NoticeDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(&self, window: &MainWindow, content: NoticeContent) {
        self.ensure_dialog_instantiated();
        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window);
            win.set_window_title(if content.window_title.is_empty() { "RustView".into() } else { content.window_title.into() });
            win.set_heading(content.heading.into());
            win.set_message(content.message.into());
            win.set_details(content.details.into());
            win.set_footnote(content.footnote.into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the dialog and releases the modal lock of the main window.
    pub fn close(&self, window: &MainWindow) {
        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    fn ensure_dialog_instantiated(&self) {
        let mut slot = self.dialog.borrow_mut();
        if slot.is_none() {
            match NoticeDialog::new() {
                Ok(win) => *slot = Some(win),
                Err(e) => tracing::error!(error = %e, "[NoticeDialog] The dialog could not be created"),
            }
        }
    }

    fn bind_dialog_events(&self, win: &NoticeDialog, window: &MainWindow) {
        let controller = self.clone();
        let main = window.clone_strong();
        win.on_acknowledged(move || controller.close(&main));

        let controller = self.clone();
        let main = window.clone_strong();
        win.window().on_close_requested(move || {
            controller.close(&main);
            CloseRequestResponse::HideWindow
        });
    }
}
