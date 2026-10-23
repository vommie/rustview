use crate::ui::{MainWindow, MissingFileDialog};
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::SaturatingAs;
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

    pub fn format_missing_paths_list(missing_files: &[MissingFileItem]) -> String {
        missing_files
            .iter()
            .map(|item| item.path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Displays the Missing File modal dialog for one or multiple missing files.
    pub fn show(&self, window: &MainWindow, operation_context: &str, missing_files: Vec<MissingFileItem>) {
        if missing_files.is_empty() {
            return;
        }

        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window);

            let count = missing_files.len();
            let has_multiple = count > 1;

            tracing::warn!(
                operation_context = operation_context,
                missing_count = count,
                first_missing = %missing_files.first().map(|f| LogObscurer::path(&f.path)).unwrap_or_default(),
                "[MissingFileDialog] Displaying missing files notification dialog with focusable read-only text area"
            );

            win.set_operation_context(operation_context.into());
            win.set_has_multiple(has_multiple);
            win.set_missing_files_count(count.saturating_as::<i32>());

            if has_multiple {
                let formatted_list = Self::format_missing_paths_list(&missing_files);
                win.set_missing_files_text(formatted_list.into());
                win.set_single_file_name("".into());
                win.set_single_file_path("".into());
            } else {
                let single = &missing_files[0];
                let path_str = single.path.to_string_lossy().to_string();
                win.set_single_file_name(single.filename.clone().into());
                win.set_single_file_path(path_str.clone().into());
                win.set_missing_files_text(path_str.into());
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
        tracing::debug!("[MissingFileDialog] Dialog dismissed by user, releasing modal lock");
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
