//! The question when an edited picture cannot simply be written (the file changed on disk, it is gone, it is read-only, writing
//! failed, a file of the chosen name exists): a heading, a message and the answers. The main window is locked while it is open.

use crate::ui::{ImageChoiceView, ImageSaveConflictDialog, MainWindow};
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

/// One answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub code: i32,
    pub label: String,
    pub primary: bool,
    pub destructive: bool,
}

impl Choice {
    pub fn new(code: i32, label: impl Into<String>) -> Self {
        Self {
            code,
            label: label.into(),
            primary: false,
            destructive: false,
        }
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
}

/// What the dialog asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub heading: String,
    pub message: String,
    pub detail: String,
    pub choices: Vec<Choice>,
    /// The answer of Esc and of closing the window.
    pub cancel: i32,
}

type Answer = Box<dyn FnOnce(i32)>;

thread_local! {
    static DIALOG: RefCell<Option<ImageSaveConflictDialog>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<(Answer, i32)>> = const { RefCell::new(None) };
}

pub struct ChoiceDialog;

impl ChoiceDialog {
    /// Asks `question`; `answer` gets the code of the chosen answer (the cancel code when the window is closed). Returns
    /// `false` when the dialog could not be shown (then `answer` already got the cancel code).
    pub fn ask(window: &MainWindow, question: &Question, answer: impl FnOnce(i32) + 'static) -> bool {
        Self::finish(None);
        let shown = DIALOG.with(|d| {
            let mut dialog = d.borrow_mut();
            if dialog.is_none() {
                match ImageSaveConflictDialog::new() {
                    Ok(win) => {
                        crate::ui::dialog_fit::install(&win);
                        crate::ui::dialog_parent::install(&win);
                        Self::bind_events(&win, window);
                        *dialog = Some(win);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "[ImageEdit: Dialog] The question could not be created");
                        return false;
                    }
                }
            }
            let Some(win) = dialog.as_ref() else { return false };
            win.set_heading(question.heading.as_str().into());
            win.set_message(question.message.as_str().into());
            win.set_detail(question.detail.as_str().into());
            win.set_cancel_code(question.cancel);
            win.set_primary_code(
                question
                    .choices
                    .iter()
                    .find(|c| c.primary)
                    .map_or(question.cancel, |c| c.code),
            );
            let choices: Vec<ImageChoiceView> = question
                .choices
                .iter()
                .map(|c| ImageChoiceView {
                    code: c.code,
                    label: c.label.as_str().into(),
                    primary: c.primary,
                    destructive: c.destructive,
                })
                .collect();
            win.set_choices(ModelRc::from(Rc::new(VecModel::from(choices))));
            window.set_is_context_menu_open(false);
            window.set_is_modal_locked(true);
            crate::ui::dialog_parent::show(win).is_ok()
        });
        if shown {
            PENDING.with(|p| *p.borrow_mut() = Some((Box::new(answer), question.cancel)));
            tracing::info!(heading = %question.heading, "[ImageEdit: Dialog] Question shown");
        } else {
            answer(question.cancel);
        }
        shown
    }

    /// Closes the dialog; a pending question is answered with its cancel code (shutdown, another dialog replaces it).
    pub fn close(window: &MainWindow) {
        Self::hide(window);
        Self::finish(None);
    }

    fn hide(window: &MainWindow) {
        DIALOG.with(|d| {
            if let Some(win) = d.borrow().as_ref() {
                let _ = win.hide();
            }
        });
        window.set_is_modal_locked(false);
    }

    /// Hands `code` (or the cancel code) to whoever asked.
    fn finish(code: Option<i32>) {
        let pending = PENDING.with(|p| p.borrow_mut().take());
        if let Some((answer, cancel)) = pending {
            answer(code.unwrap_or(cancel));
        }
    }

    fn bind_events(win: &ImageSaveConflictDialog, window: &MainWindow) {
        let weak = window.as_weak();
        win.on_chosen(move |code| {
            if let Some(window) = weak.upgrade() {
                Self::hide(&window);
            }
            tracing::info!(code, "[ImageEdit: Dialog] Answered");
            Self::finish(Some(code));
        });
        let weak = window.as_weak();
        win.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                window.set_is_modal_locked(false);
            }
            Self::finish(None);
            CloseRequestResponse::HideWindow
        });
    }
}
