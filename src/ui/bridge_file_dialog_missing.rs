use super::{MainWindow, MissingFileDialog};
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Missing file item metadata for dialog display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFileItem {
    pub filename: String,
    pub path: PathBuf,
}

/// Specialized controller managing the Missing File(s) modal notification dialog.
#[derive(Default, Clone)]
pub struct MissingFileDialogController {
    dialog: Rc<RefCell<Option<MissingFileDialog>>>,
}

impl MissingFileDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the Missing File modal dialog for one or multiple missing files.
    pub fn show(
        &self,
        window: &MainWindow,
        operation_context: &str,
        missing_files: Vec<MissingFileItem>,
    ) {
        if missing_files.is_empty() {
            return;
        }

        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window);

            let count = missing_files.len();
            let has_multiple = count > 1;

            win.set_operation_context(operation_context.into());
            win.set_has_multiple(has_multiple);
            win.set_missing_files_count(count as i32);

            if has_multiple {
                let formatted_list = missing_files
                    .iter()
                    .map(|item| format!("• {} ({})", item.filename, item.path.display()))
                    .collect::<Vec<_>>()
                    .join("\n");
                win.set_missing_files_text(formatted_list.into());
                win.set_single_file_name("".into());
                win.set_single_file_path("".into());
            } else {
                let single = &missing_files[0];
                win.set_single_file_name(single.filename.clone().into());
                win.set_single_file_path(single.path.to_string_lossy().to_string().into());
                win.set_missing_files_text("".into());
            }

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

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = MissingFileDialog::new().expect("Failed to instantiate MissingFileDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(&self, win: &MissingFileDialog, window: &MainWindow) {
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
