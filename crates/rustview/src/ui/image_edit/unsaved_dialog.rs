//! The question before something closes that holds pictures with unsaved edits: one checkbox per picture, Apply / Cancel
//! (`viewer::edit::unsaved` words it and turns the checkboxes into a decision).

use crate::ui::{ImageUnsavedDialog, MainWindow, UnsavedPictureView};
use crate::viewer::edit::unsaved::{UnsavedDecision, UnsavedEntry, UnsavedPrompt, apply_label, decide};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

type Answer = Box<dyn FnOnce(Option<UnsavedDecision>)>;

struct Pending {
    entries: Vec<UnsavedEntry>,
    answer: Answer,
}

thread_local! {
    static DIALOG: RefCell<Option<ImageUnsavedDialog>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

fn row(entry: &UnsavedEntry) -> UnsavedPictureView {
    UnsavedPictureView {
        filename: entry.filename.as_str().into(),
        folder: entry.folder.as_str().into(),
        description: entry.description.as_str().into(),
        save: entry.save,
        needs_export: !entry.overwritable,
    }
}

pub struct UnsavedDialog;

impl UnsavedDialog {
    /// Asks what to do with the pictures of `prompt`; `answer` gets the decision, or `None` when the user cancels (nothing
    /// closes then).
    pub fn ask(window: &MainWindow, prompt: UnsavedPrompt, answer: impl FnOnce(Option<UnsavedDecision>) + 'static) {
        Self::finish(false);
        let shown = DIALOG.with(|d| {
            let mut dialog = d.borrow_mut();
            if dialog.is_none() {
                match ImageUnsavedDialog::new() {
                    Ok(win) => {
                        Self::bind_events(&win, window);
                        *dialog = Some(win);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "[ImageEdit: Unsaved] The dialog could not be created");
                        return false;
                    }
                }
            }
            let Some(win) = dialog.as_ref() else { return false };
            win.set_heading(prompt.title.as_str().into());
            win.set_message(prompt.message.as_str().into());
            win.set_pictures(ModelRc::from(Rc::new(VecModel::from(
                prompt.entries.iter().map(row).collect::<Vec<_>>(),
            ))));
            win.set_apply_label(apply_label(&prompt.entries).into());
            window.set_is_context_menu_open(false);
            window.set_is_modal_locked(true);
            win.show().is_ok()
        });
        if shown {
            tracing::info!(
                pictures = prompt.entries.len(),
                "[ImageEdit: Unsaved] Asked before closing"
            );
            PENDING.with(|p| {
                *p.borrow_mut() = Some(Pending {
                    entries: prompt.entries,
                    answer: Box::new(answer),
                });
            });
        } else {
            // Without the dialog nothing may be lost: the close is called off.
            answer(None);
        }
    }

    pub fn close(window: &MainWindow) {
        Self::hide(window);
        Self::finish(false);
    }

    fn hide(window: &MainWindow) {
        DIALOG.with(|d| {
            if let Some(win) = d.borrow().as_ref() {
                let _ = win.hide();
            }
        });
        window.set_is_modal_locked(false);
    }

    fn finish(apply: bool) {
        let pending = PENDING.with(|p| p.borrow_mut().take());
        if let Some(pending) = pending {
            let decision = apply.then(|| decide(&pending.entries));
            tracing::info!(applied = apply, "[ImageEdit: Unsaved] Answered");
            (pending.answer)(decision);
        }
    }

    fn bind_events(win: &ImageUnsavedDialog, window: &MainWindow) {
        let dialog = win.as_weak();
        win.on_toggled(move |row_index, save| {
            let Ok(index) = usize::try_from(row_index) else { return };
            let label = PENDING.with(|p| {
                let mut pending = p.borrow_mut();
                let pending = pending.as_mut()?;
                pending.entries.get_mut(index)?.save = save;
                Some(apply_label(&pending.entries))
            });
            if let (Some(label), Some(win)) = (label, dialog.upgrade()) {
                let rows = win.get_pictures();
                if let Some(mut view) = rows.row_data(index) {
                    view.save = save;
                    rows.set_row_data(index, view);
                }
                win.set_apply_label(label.into());
            }
        });
        let weak = window.as_weak();
        win.on_apply(move || {
            if let Some(window) = weak.upgrade() {
                Self::hide(&window);
            }
            Self::finish(true);
        });
        let weak = window.as_weak();
        win.on_cancel(move || {
            if let Some(window) = weak.upgrade() {
                Self::hide(&window);
            }
            Self::finish(false);
        });
        let weak = window.as_weak();
        win.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                window.set_is_modal_locked(false);
            }
            Self::finish(false);
            CloseRequestResponse::HideWindow
        });
    }
}
