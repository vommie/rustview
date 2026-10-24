//! Saving edited pictures (the edits themselves live in `viewer::edit` and the Multiview): over the original, through the
//! export dialog ("Save As"), and the question before closing views that hold unsaved pictures (`close_guard`).

mod choice_dialog;
pub mod close_guard;
mod export_dialog;
pub mod export_plan;
pub mod export_target;
mod library_update;
mod save_flow;
mod unsaved_dialog;

pub use choice_dialog::ChoiceDialog;
pub use export_dialog::ExportDialog;
pub use save_flow::{SaveIntent, save_tile};
pub use unsaved_dialog::UnsavedDialog;

use crate::ui::MainWindow;
use crate::ui::multiview::handle::{AppContext, MultiviewHandle};

/// Everything a save needs: the Multiview handle (to draw the views again), the application and the main window.
#[derive(Clone)]
pub struct EditContext {
    pub handle: MultiviewHandle,
    pub app: AppContext,
    pub window: slint::Weak<MainWindow>,
}

/// Learns whether the edits of a picture are done (saved, or dropped on purpose) or still open (cancelled, failed).
pub type AfterSave = Option<Box<dyn FnOnce(bool)>>;

/// Closes every dialog of the editor (the application exits); open questions are answered with "cancel".
pub fn close_windows(window: &MainWindow) {
    ChoiceDialog::close(window);
    ExportDialog::close(window);
    UnsavedDialog::close(window);
}
