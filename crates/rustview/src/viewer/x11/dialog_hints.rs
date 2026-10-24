//! A dialog window belongs to the window it was opened from: ICCCM `WM_TRANSIENT_FOR` names that window, EWMH
//! `_NET_WM_WINDOW_TYPE_DIALOG` and `_NET_WM_STATE_MODAL` say what the dialog is. The window manager then keeps the dialog above
//! its parent (and only above it, not above other programs), centers it on the parent and, for a modal dialog, activates the
//! dialog whenever the parent is activated; `KWin` also dims the parent ("Dialog Parent" effect). winit 0.30 has no such hint (its
//! parent window is an embedded child window), so the properties are written here.

use super::session::Session;
use super::wm_state::{SOURCE_APPLICATION, STATE_ADD, STATE_REMOVE, send_to_window_manager, set_atom_list};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

fn window_of(xid: u64) -> Option<u32> {
    u32::try_from(xid).ok().filter(|&xid| xid != 0)
}

/// Makes `dialog_xid` a dialog of `parent_xid` (modal or not). The dialog must not be mapped: the window manager reads these
/// properties when the window is mapped (a hidden dialog is withdrawn and managed anew when it is shown again). Waits until the
/// X server has the properties, so that a map request sent afterwards on another connection (winit's) cannot overtake them.
/// Returns whether they were written.
pub fn set_x11_dialog_parent(dialog_xid: u64, parent_xid: u64, modal: bool) -> bool {
    let (Some(dialog), Some(parent)) = (window_of(dialog_xid), window_of(parent_xid)) else {
        return false;
    };
    let Ok(session) = Session::open("the dialog parent") else {
        return false;
    };
    let Some([state, modal_state, window_type, dialog_type]) = session.atoms([
        "_NET_WM_STATE",
        "_NET_WM_STATE_MODAL",
        "_NET_WM_WINDOW_TYPE",
        "_NET_WM_WINDOW_TYPE_DIALOG",
    ]) else {
        return false;
    };
    let conn = &session.conn;
    let _ = conn.change_property32(
        PropMode::REPLACE,
        dialog,
        AtomEnum::WM_TRANSIENT_FOR,
        AtomEnum::WINDOW,
        &[parent],
    );
    set_atom_list(&session, dialog, window_type, &[dialog_type]);
    // Other states (written by winit or left from the last time) stay; only the modal one follows `modal`.
    let mut states: Vec<u32> = conn
        .get_property(false, dialog, state, AtomEnum::ATOM, 0, 64)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().map(Iterator::collect))
        .unwrap_or_default();
    states.retain(|&atom| atom != modal_state);
    if modal {
        states.push(modal_state);
    }
    set_atom_list(&session, dialog, state, &states);
    session.sync();
    tracing::debug!(
        dialog_xid,
        parent_xid,
        modal,
        "[X11: DialogParent] Transient-for, dialog type and modal state written before the dialog is mapped"
    );
    true
}

/// Asks the window manager to make the mapped `dialog_xid` modal (or not). A window manager that has not managed the window
/// yet ignores the message; it reads the property `set_x11_dialog_parent` wrote instead.
pub fn request_x11_modal_state(dialog_xid: u64, modal: bool) {
    let Some(dialog) = window_of(dialog_xid) else {
        return;
    };
    let Ok(session) = Session::open("the modal state") else {
        return;
    };
    let Some([state, modal_state]) = session.atoms(["_NET_WM_STATE", "_NET_WM_STATE_MODAL"]) else {
        return;
    };
    let action = if modal { STATE_ADD } else { STATE_REMOVE };
    send_to_window_manager(&session, dialog, state, [action, modal_state, 0, SOURCE_APPLICATION, 0]);
    session.flush();
}
