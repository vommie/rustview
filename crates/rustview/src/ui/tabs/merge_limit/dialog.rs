use crate::ui::{MainWindow, TabMergeLimitDialog};
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;

thread_local! {
    static DIALOG: RefCell<Option<TabMergeLimitDialog>> = const { RefCell::new(None) };
}

/// The window that explains why selected tabs cannot be merged. It has one button; the main window is locked while it is open.
pub struct MergeLimitDialog;

impl MergeLimitDialog {
    /// Shows the dialog with `message` and `breakdown`. Returns `false` when it could not be created (the caller says it in the
    /// status line instead).
    pub fn show(window: &MainWindow, message: &str, breakdown: &str) -> bool {
        window.set_is_context_menu_open(false);
        DIALOG.with(|d| {
            let mut dialog = d.borrow_mut();
            if dialog.is_none() {
                match TabMergeLimitDialog::new() {
                    Ok(win) => {
                        Self::bind_events(&win, window);
                        *dialog = Some(win);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "[MergeLimitDialog] The dialog could not be created");
                        return false;
                    }
                }
            }
            let Some(win) = dialog.as_ref() else { return false };
            win.set_message(message.into());
            win.set_breakdown(breakdown.into());
            window.set_is_modal_locked(true);
            let _ = win.show();
            true
        })
    }

    /// Hides the dialog and releases the modal lock.
    pub fn close(window: &MainWindow) {
        DIALOG.with(|d| {
            if let Some(win) = d.borrow().as_ref() {
                let _ = win.hide();
            }
        });
        window.set_is_modal_locked(false);
    }

    fn bind_events(win: &TabMergeLimitDialog, window: &MainWindow) {
        let weak = window.as_weak();
        win.on_closed(move || {
            if let Some(window) = weak.upgrade() {
                Self::close(&window);
            }
        });

        let weak = window.as_weak();
        win.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                window.set_is_modal_locked(false);
            }
            CloseRequestResponse::HideWindow
        });
    }
}
