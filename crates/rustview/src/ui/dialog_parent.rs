//! Every dialog window belongs to the window it was opened from: the main window, the Multiview fullscreen, the settings window
//! or another dialog (the one that has the focus when the dialog opens; without one the fullscreen, then the settings window,
//! then the main window).
//!
//! **X11:** the dialog tells the window manager before it is mapped (`viewer::set_x11_dialog_parent`: `WM_TRANSIENT_FOR`, dialog
//! type, `_NET_WM_STATE_MODAL`). `KWin` keeps it above its parent and only there (no dialog floats above other programs, so no
//! dialog is `always-on-top`), centers it on the parent, dims the parent ("Dialog Parent" effect) and activates the dialog when
//! the user activates the parent. The window manager still passes the click on, so the parent keeps its own lock
//! (`is-modal-locked`), and a click on the locked parent brings the dialog to the front (`raise_open`).
//! The first time a dialog is shown its window does not exist yet (Slint creates it in the event loop): the hints are written
//! by its rendering notifier at `RenderingSetup`, which runs after the window was created and before it is mapped.
//!
//! **Wayland:** winit 0.30 has neither an xdg parent nor xdg-dialog (and no window levels: `always-on-top` never worked there),
//! so a dialog is an ordinary window; the application lock of the main window keeps it modal and a click on the locked main
//! window closes the dialogs as before (`raise_open` returns false: winit cannot activate a window there).
//!
//! The settings window is both: dialogs open from it, and it is itself a modal child of the window it was opened from (the
//! main window locks, a click on it brings the settings to the front instead of closing them).
//!
//! Every dialog window calls [`install`] right after it is created and is shown with [`show`]; the settings window calls
//! [`install_window`] instead; the other windows dialogs open from are announced with [`register_parent`]; folder and file
//! pickers are made with [`file_dialog`] (all enforced by `tests/dialog_layout_policy.rs`). UI thread only.

use crate::viewer::{get_raw_window_xid, request_x11_modal_state, set_x11_dialog_parent};
use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What a window is to the dialogs. The order of the first three is the order a dialog falls back to when no window of the
/// application has the focus (a dialog opened by background work): the fullscreen covers the screen, so a dialog behind it
/// could not be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Fullscreen,
    Settings,
    Main,
    Dialog,
}

const FALLBACK_ORDER: [Role; 3] = [Role::Fullscreen, Role::Settings, Role::Main];

/// What the window manager is told about a dialog.
#[derive(Debug, Clone, Copy)]
struct Hint {
    parent_xid: u64,
    modal: bool,
}

type LendWindow = Box<dyn Fn(&mut dyn FnMut(&slint::Window)) -> bool>;

struct Entry {
    role: Role,
    /// Shown as a modal child of its parent (dialogs, the settings window; not the color picker, which only stays above its
    /// parent, and not the windows that only are parents).
    modal: bool,
    /// Lends the window of the component to the callback; false once the component is gone.
    window: LendWindow,
    /// The hint the next `RenderingSetup` writes (the first time the dialog is shown, before its window exists).
    pending: Cell<Option<Hint>>,
    /// The window manager knows the parent of the dialog that is shown now.
    parented: Cell<bool>,
    /// The rendering notifier is installed (else the hints are written right after the window appears).
    notifier: Cell<bool>,
    /// When the dialog was shown last (larger = later).
    shown_at: Cell<u64>,
}

impl Entry {
    fn with_window<R>(&self, f: impl FnOnce(&slint::Window) -> R) -> Option<R> {
        let mut f = Some(f);
        let mut result = None;
        (self.window)(&mut |window| {
            if let Some(f) = f.take() {
                result = Some(f(window));
            }
        });
        result
    }

    fn is(&self, window: &slint::Window) -> bool {
        self.with_window(|w| std::ptr::eq(w, window)).unwrap_or(false)
    }

    fn is_visible(&self) -> bool {
        self.with_window(slint::Window::is_visible).unwrap_or(false)
    }

    fn has_focus(&self) -> bool {
        self.with_window(|w| {
            w.with_winit_window(slint::winit_030::winit::window::Window::has_focus)
                .unwrap_or(false)
        })
        .unwrap_or(false)
    }

    fn xid(&self) -> Option<u64> {
        self.with_window(get_raw_window_xid).flatten()
    }
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<Entry>>> = const { RefCell::new(Vec::new()) };
    static SHOWS: Cell<u64> = const { Cell::new(0) };
}

fn add<C: ComponentHandle + 'static>(component: &C, role: Role, modal: bool) -> Rc<Entry> {
    let weak = component.as_weak();
    let entry = Rc::new(Entry {
        role,
        modal,
        window: Box::new(move |f| {
            weak.upgrade().is_some_and(|c| {
                f(c.window());
                true
            })
        }),
        pending: Cell::new(None),
        parented: Cell::new(false),
        notifier: Cell::new(false),
        shown_at: Cell::new(0),
    });
    WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        windows.retain(|e| e.with_window(|_| ()).is_some());
        windows.push(entry.clone());
    });
    entry
}

fn entries() -> Vec<Rc<Entry>> {
    WINDOWS.with(|windows| windows.borrow().clone())
}

/// Announces a window dialogs can be opened from (main window, fullscreen); once, after it is created.
pub fn register_parent<C: ComponentHandle + 'static>(window: &C, role: Role)
where
    for<'a> crate::ui::Theme<'a>: slint::Global<'a, C>,
    for<'a> crate::ui::Palette<'a>: slint::Global<'a, C>,
{
    crate::ui::theme::install(window);
    add(window, role, false);
}

/// Prepares a modal dialog window; once, right after it is created.
pub fn install<C: ComponentHandle + 'static>(dialog: &C)
where
    for<'a> crate::ui::Theme<'a>: slint::Global<'a, C>,
    for<'a> crate::ui::Palette<'a>: slint::Global<'a, C>,
{
    install_as(dialog, Role::Dialog, true);
}

/// Prepares a window that dialogs open from and that is itself shown as a modal child of the window it was opened from (the
/// settings window, `role`); once, right after it is created. It is shown with [`show`].
pub fn install_window<C: ComponentHandle + 'static>(window: &C, role: Role)
where
    for<'a> crate::ui::Theme<'a>: slint::Global<'a, C>,
    for<'a> crate::ui::Palette<'a>: slint::Global<'a, C>,
{
    install_as(window, role, true);
}

/// Prepares a dialog window that only stays above its parent and leaves the parent usable (the color picker, whose colors
/// the parent shows live); once, right after it is created.
pub fn install_transient<C: ComponentHandle + 'static>(dialog: &C)
where
    for<'a> crate::ui::Theme<'a>: slint::Global<'a, C>,
    for<'a> crate::ui::Palette<'a>: slint::Global<'a, C>,
{
    install_as(dialog, Role::Dialog, false);
}

fn install_as<C: ComponentHandle + 'static>(dialog: &C, role: Role, modal: bool)
where
    for<'a> crate::ui::Theme<'a>: slint::Global<'a, C>,
    for<'a> crate::ui::Palette<'a>: slint::Global<'a, C>,
{
    crate::ui::theme::install(dialog);
    let entry = add(dialog, role, modal);
    let (weak_entry, weak_dialog) = (Rc::downgrade(&entry), dialog.as_weak());
    let installed = dialog.window().set_rendering_notifier(move |state, _| {
        // Called while the window exists but is not mapped yet.
        if matches!(state, slint::RenderingState::RenderingSetup)
            && let (Some(entry), Some(dialog)) = (weak_entry.upgrade(), weak_dialog.upgrade())
            && let Some(hint) = entry.pending.take()
        {
            write_hint(&entry, dialog.window(), hint);
        }
    });
    match installed {
        Ok(()) => entry.notifier.set(true),
        Err(e) => tracing::warn!(
            error = ?e,
            "[DialogParent] No rendering notifier: the parent of this dialog is set after its window appears"
        ),
    }
}

fn find(window: &slint::Window) -> Option<Rc<Entry>> {
    entries().into_iter().find(|e| e.is(window))
}

/// The window the dialog belongs to: the visible window of the application that has the focus (not the dialog itself), else the
/// first visible one of `FALLBACK_ORDER`.
fn parent_of(dialog: &slint::Window) -> Option<Rc<Entry>> {
    let candidates: Vec<Rc<Entry>> = entries()
        .into_iter()
        .filter(|e| !e.is(dialog) && e.is_visible())
        .collect();
    if let Some(focused) = candidates.iter().find(|e| e.has_focus()) {
        return Some(focused.clone());
    }
    FALLBACK_ORDER
        .iter()
        .find_map(|role| candidates.iter().find(|e| e.role == *role).cloned())
}

fn write_hint(entry: &Entry, dialog: &slint::Window, hint: Hint) {
    let written = get_raw_window_xid(dialog).is_some_and(|xid| set_x11_dialog_parent(xid, hint.parent_xid, hint.modal));
    entry.parented.set(written);
}

/// Shows `dialog` (installed with [`install`]) as a dialog of the window the user is in.
pub fn show<C: ComponentHandle + 'static>(dialog: &C) -> Result<(), slint::PlatformError> {
    let Some(entry) = find(dialog.window()) else {
        tracing::warn!("[DialogParent] A dialog that was not installed is shown without a parent");
        return dialog.show();
    };
    let parent = parent_of(dialog.window());
    let hint = parent.as_ref().and_then(|p| p.xid()).map(|parent_xid| Hint {
        parent_xid,
        modal: entry.modal,
    });
    entry.shown_at.set(SHOWS.with(|n| {
        n.set(n.get() + 1);
        n.get()
    }));
    entry.parented.set(false);
    entry.pending.set(None);
    // A dialog shown before still has its (unmapped) window: the hints go in now. A new one gets them at `RenderingSetup`.
    let existing = get_raw_window_xid(dialog.window());
    if let Some(hint) = hint {
        if existing.is_some() {
            write_hint(&entry, dialog.window(), hint);
        } else if entry.notifier.get() {
            entry.pending.set(Some(hint));
        } else {
            hint_after_creation(dialog, &entry, hint);
        }
    }
    dialog.show()?;
    // The window manager may not have withdrawn the window from its last showing yet when the property was written (it clears
    // the state then); the message makes it modal anyway once it manages the window again.
    if let (Some(xid), Some(hint)) = (existing, hint)
        && hint.modal
    {
        request_x11_modal_state(xid, true);
    }
    tracing::info!(
        parent = ?parent.as_ref().map(|p| p.role),
        modal = entry.modal,
        x11 = hint.is_some(),
        "[DialogParent] Dialog shown over the window it was opened from"
    );
    Ok(())
}

/// Without a rendering notifier the window is only known once Slint created and mapped it: the hints follow then.
fn hint_after_creation<C: ComponentHandle + 'static>(dialog: &C, entry: &Rc<Entry>, hint: Hint) {
    let (weak, entry) = (dialog.as_weak(), Rc::downgrade(entry));
    let spawned = slint::spawn_local(async move {
        let Some(dialog) = weak.upgrade() else { return };
        if dialog.window().winit_window().await.is_err() {
            return;
        }
        if let (Some(entry), Some(xid)) = (entry.upgrade(), get_raw_window_xid(dialog.window())) {
            write_hint(&entry, dialog.window(), hint);
            if hint.modal {
                request_x11_modal_state(xid, true);
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "[DialogParent] The parent of the dialog could not be set");
    }
}

/// Brings the open modal dialog (or settings window) that was shown last to the front and gives it the focus (a click on its
/// locked parent). False when none is open whose parent the window manager knows (Wayland): the caller then dismisses them
/// instead.
pub fn raise_open() -> bool {
    let newest = entries()
        .into_iter()
        .filter(|e| e.modal && e.parented.get() && e.is_visible())
        .max_by_key(|e| e.shown_at.get());
    let Some(dialog) = newest else {
        return false;
    };
    dialog.with_window(|w| w.with_winit_window(slint::winit_030::winit::window::Window::focus_window));
    tracing::info!("[DialogParent] The locked parent was clicked; the open dialog comes to the front");
    true
}

/// True while a modal dialog or the settings window is open.
pub fn modal_dialog_open() -> bool {
    entries().iter().any(|e| e.modal && e.is_visible())
}

/// A folder or file picker of the desktop (`rfd`, through the desktop portal) that belongs to `parent`.
pub fn file_dialog(parent: &slint::Window) -> rfd::FileDialog {
    rfd::FileDialog::new().set_parent(&parent.window_handle())
}
