//! The question before something closes that holds pictures with unsaved edits: one checkbox per picture, Apply / Don't Save /
//! Cancel (`viewer::edit::unsaved` words it and turns the checkboxes into a decision).

use crate::ui::{ImageUnsavedDialog, MainWindow, UnsavedPictureView};
use crate::viewer::edit::unsaved::{UnsavedDecision, UnsavedEntry, UnsavedPrompt, apply_label, decide, discard_all};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::image_decode::downscale;
use rustview_core::util::cast::FloatToInt;
use slint::{CloseRequestResponse, ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

type Answer = Box<dyn FnOnce(Option<UnsavedDecision>)>;

/// The button the user pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    /// Save the checked pictures, discard the others, go on.
    Apply,
    /// Discard every edit, go on.
    DontSave,
    /// Nothing closes.
    Cancel,
}

struct Pending {
    entries: Vec<UnsavedEntry>,
    answer: Answer,
}

thread_local! {
    static DIALOG: RefCell<Option<ImageUnsavedDialog>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

/// The edge of the square a thumbnail is drawn in, logical pixels (the same as in `image_unsaved_dialog.slint`).
const THUMBNAIL_EDGE: f32 = 64.0;

/// The row of `entry`; `picture` is what its tile shows (with the edits), made small enough for the thumbnail square at the
/// window's `scale`. Without it the square shows the picture icon.
fn row(entry: &UnsavedEntry, picture: Option<&SharedPixelBuffer<Rgb8Pixel>>, scale: f32) -> UnsavedPictureView {
    let edge = (THUMBNAIL_EDGE * scale.max(1.0)).ceil().to_int::<u32>();
    let thumbnail = picture.and_then(|pixels| match downscale(pixels.clone(), edge, edge) {
        Ok(small) => Some(Image::from_rgb8(small)),
        Err(e) => {
            tracing::debug!(error = %e, "[ImageEdit: Unsaved] No thumbnail");
            None
        }
    });
    UnsavedPictureView {
        filename: entry.filename.as_str().into(),
        folder: entry.folder.as_str().into(),
        description: entry.description.as_str().into(),
        save: entry.save,
        needs_export: !entry.overwritable,
        has_thumbnail: thumbnail.is_some(),
        thumbnail: thumbnail.unwrap_or_default(),
    }
}

pub struct UnsavedDialog;

impl UnsavedDialog {
    /// Asks what to do with the pictures of `prompt`; `answer` gets the decision (Apply, Don't Save), or `None` when the user
    /// cancels (nothing closes then).
    pub fn ask(
        window: &MainWindow,
        prompt: UnsavedPrompt,
        pictures: &HashMap<TileKey, SharedPixelBuffer<Rgb8Pixel>>,
        answer: impl FnOnce(Option<UnsavedDecision>) + 'static,
    ) {
        Self::finish(Reply::Cancel);
        let shown = DIALOG.with(|d| {
            let mut dialog = d.borrow_mut();
            if dialog.is_none() {
                match ImageUnsavedDialog::new() {
                    Ok(win) => {
                        crate::ui::dialog_fit::install(&win);
                        crate::ui::dialog_parent::install(&win);
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
            let scale = win.window().scale_factor();
            win.set_pictures(ModelRc::from(Rc::new(VecModel::from(
                prompt
                    .entries
                    .iter()
                    .map(|entry| row(entry, pictures.get(&entry.tile), scale))
                    .collect::<Vec<_>>(),
            ))));
            win.set_apply_label(apply_label(&prompt.entries).into());
            window.set_is_context_menu_open(false);
            window.set_is_modal_locked(true);
            crate::ui::dialog_parent::show(win).is_ok()
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
        Self::finish(Reply::Cancel);
    }

    fn hide(window: &MainWindow) {
        DIALOG.with(|d| {
            if let Some(win) = d.borrow().as_ref() {
                let _ = win.hide();
            }
        });
        window.set_is_modal_locked(false);
    }

    fn finish(reply: Reply) {
        let pending = PENDING.with(|p| p.borrow_mut().take());
        if let Some(pending) = pending {
            let decision = match reply {
                Reply::Apply => Some(decide(&pending.entries)),
                Reply::DontSave => Some(discard_all(&pending.entries)),
                Reply::Cancel => None,
            };
            tracing::info!(?reply, "[ImageEdit: Unsaved] Answered");
            (pending.answer)(decision);
        }
    }

    fn reply(weak: &slint::Weak<MainWindow>, reply: Reply) {
        if let Some(window) = weak.upgrade() {
            Self::hide(&window);
        }
        Self::finish(reply);
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
        win.on_apply(move || Self::reply(&weak, Reply::Apply));
        let weak = window.as_weak();
        win.on_dont_save(move || Self::reply(&weak, Reply::DontSave));
        let weak = window.as_weak();
        win.on_cancel(move || Self::reply(&weak, Reply::Cancel));
        let weak = window.as_weak();
        win.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                window.set_is_modal_locked(false);
            }
            Self::finish(Reply::Cancel);
            CloseRequestResponse::HideWindow
        });
    }
}
