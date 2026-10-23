use super::policy::{self, OpenKind};
use crate::ui::{MainWindow, OpenManyFilesDialog};
use rustview_core::settings::SettingsManager;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;

type Pending = Box<dyn FnOnce()>;

thread_local! {
    static DIALOG: RefCell<Option<OpenManyFilesDialog>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

pub struct OpenConfirmation;

impl OpenConfirmation {
    /// Runs `proceed` now when `count` is below the limit of the settings, otherwise after the user confirmed the dialog.
    pub fn run_or_ask(
        window: &MainWindow,
        settings: &SettingsManager,
        kind: OpenKind,
        count: usize,
        proceed: impl FnOnce() + 'static,
    ) {
        let limit = settings.get().general.confirm_open_file_count;
        if !policy::needs_confirmation(count, limit) {
            proceed();
            return;
        }

        tracing::info!(
            kind = kind.log_name(),
            count,
            limit,
            "[OpenConfirmation] Opening many files at once, asking the user first"
        );
        if !Self::show(window, kind, count) {
            proceed();
            return;
        }
        PENDING.with(|p| *p.borrow_mut() = Some(Box::new(proceed)));
    }

    /// Hides the dialog and releases the modal lock; the pending action is dropped.
    pub fn close(window: &MainWindow) {
        PENDING.with(|p| p.borrow_mut().take());
        DIALOG.with(|d| {
            if let Some(win) = d.borrow().as_ref() {
                let _ = win.hide();
            }
        });
        window.set_is_modal_locked(false);
    }

    fn show(window: &MainWindow, kind: OpenKind, count: usize) -> bool {
        window.set_is_context_menu_open(false);
        DIALOG.with(|d| {
            let mut dialog = d.borrow_mut();
            if dialog.is_none() {
                match OpenManyFilesDialog::new() {
                    Ok(win) => {
                        Self::bind_events(&win, window);
                        *dialog = Some(win);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "[OpenConfirmation] The dialog could not be created, opening without asking");
                        return false;
                    }
                }
            }
            let Some(win) = dialog.as_ref() else { return false };
            win.set_message(policy::message(kind, count).into());
            win.set_hint(policy::HINT.into());
            window.set_is_modal_locked(true);
            let _ = win.show();
            true
        })
    }

    fn bind_events(win: &OpenManyFilesDialog, window: &MainWindow) {
        let weak = window.as_weak();
        win.on_confirmed(move || {
            tracing::info!("[OpenConfirmation] Confirmed by the user");
            let proceed = Self::finish(&weak);
            if let Some(proceed) = proceed {
                proceed();
            }
        });

        let weak = window.as_weak();
        win.on_cancelled(move || {
            tracing::info!("[OpenConfirmation] Cancelled by the user");
            Self::finish(&weak);
        });

        let weak = window.as_weak();
        win.window().on_close_requested(move || {
            Self::finish(&weak);
            CloseRequestResponse::HideWindow
        });
    }

    /// Closes the dialog and hands back the action that waited for the answer.
    fn finish(window: &slint::Weak<MainWindow>) -> Option<Pending> {
        let proceed = PENDING.with(|p| p.borrow_mut().take());
        DIALOG.with(|d| {
            if let Some(win) = d.borrow().as_ref() {
                let _ = win.hide();
            }
        });
        if let Some(window) = window.upgrade() {
            window.set_is_modal_locked(false);
        }
        proceed
    }
}
