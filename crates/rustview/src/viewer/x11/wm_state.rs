//! Window manager state of a top level window (EWMH): maximized and fullscreen, set through properties and client messages.

use super::session::Session;
use x11rb::protocol::xproto::{Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, PropMode, Window};
use x11rb::wrapper::ConnectionExt as _;

/// `_MOTIF_WM_HINTS` that ask for no decorations (flags = decorations, decorations = none).
const MOTIF_HINTS_NO_DECORATIONS: [u32; 5] = [2, 0, 0, 0, 0];
/// `_NET_WM_STATE` actions of the client message.
const STATE_REMOVE: u32 = 0;
const STATE_ADD: u32 = 1;
/// "Normal application" as the source of an EWMH client message.
const SOURCE_APPLICATION: u32 = 1;

/// Sends an EWMH client message about `window` to the root window, where the window manager listens.
fn send_to_window_manager(session: &Session, window: Window, message_type: Atom, data: [u32; 5]) {
    let event = ClientMessageEvent::new(32, window, message_type, data);
    let mask = EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT;
    let _ = session.conn.send_event(false, session.root, mask, event);
}

fn set_atom_list(session: &Session, window: Window, property: Atom, atoms: &[u32]) {
    let _ = session
        .conn
        .change_property32(PropMode::REPLACE, window, property, AtomEnum::ATOM, atoms);
}

pub fn is_x11_window_maximized(window_xid: u64) -> Option<bool> {
    let window = u32::try_from(window_xid).ok().filter(|&xid| xid != 0)?;
    let session = Session::open("the maximized check").ok()?;
    let [state, vert, horz] = session.atoms([
        "_NET_WM_STATE",
        "_NET_WM_STATE_MAXIMIZED_VERT",
        "_NET_WM_STATE_MAXIMIZED_HORZ",
    ])?;
    let reply = session
        .conn
        .get_property(false, window, state, AtomEnum::ANY, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    let atoms: Vec<u32> = reply.value32().map(Iterator::collect).unwrap_or_default();
    Some(atoms.contains(&vert) && atoms.contains(&horz))
}

/// Size of the X screen (the root window, i.e. all monitors together) in pixels. One short connection, no subprocess; `None`
/// without an X server (a Wayland session without `XWayland`).
pub fn x11_root_size() -> Option<(u32, u32)> {
    let session = Session::open("the screen size").ok()?;
    let geometry = session.conn.get_geometry(session.root).ok()?.reply().ok()?;
    (geometry.width > 0 && geometry.height > 0).then_some((u32::from(geometry.width), u32::from(geometry.height)))
}

/// Writes the maximized state into `_NET_WM_STATE` of a window that is not mapped yet. A window manager reads the initial state
/// from this property when the window is mapped (EWMH), so the window appears maximized. The client message that
/// `force_x11_maximized` and winit send only works for a window that is already mapped: the window would be shown at its
/// normal size first and maximized afterwards. Returns whether the property was written.
pub fn set_x11_initial_maximized(window_xid: u64) -> bool {
    let Some(window) = u32::try_from(window_xid).ok().filter(|&xid| xid != 0) else {
        return false;
    };
    let Ok(session) = Session::open("the initial maximized state") else {
        return false;
    };
    let Some([state, vert, horz]) = session.atoms([
        "_NET_WM_STATE",
        "_NET_WM_STATE_MAXIMIZED_VERT",
        "_NET_WM_STATE_MAXIMIZED_HORZ",
    ]) else {
        return false;
    };
    set_atom_list(&session, window, state, &[vert, horz]);
    session.flush();
    true
}

pub fn force_x11_maximized(window_xid: u64, maximized: bool) {
    let Some(window) = u32::try_from(window_xid).ok().filter(|&xid| xid != 0) else {
        return;
    };
    let Ok(session) = Session::open("the maximized state") else {
        return;
    };
    let Some([state, vert, horz]) = session.atoms([
        "_NET_WM_STATE",
        "_NET_WM_STATE_MAXIMIZED_VERT",
        "_NET_WM_STATE_MAXIMIZED_HORZ",
    ]) else {
        return;
    };
    if maximized {
        set_atom_list(&session, window, state, &[vert, horz]);
    }
    let action = if maximized { STATE_ADD } else { STATE_REMOVE };
    send_to_window_manager(&session, window, state, [action, vert, horz, SOURCE_APPLICATION, 0]);
    session.flush();
}

fn strip_decorations(session: &Session, window: Window, motif: Atom) {
    let _ = session
        .conn
        .change_property32(PropMode::REPLACE, window, motif, motif, &MOTIF_HINTS_NO_DECORATIONS);
}

pub fn force_x11_fullscreen(window_xid: u64) {
    let Some(window) = u32::try_from(window_xid).ok().filter(|&xid| xid != 0) else {
        return;
    };
    let Ok(session) = Session::open("the fullscreen state") else {
        return;
    };
    let Some([state, fullscreen, motif]) =
        session.atoms(["_NET_WM_STATE", "_NET_WM_STATE_FULLSCREEN", "_MOTIF_WM_HINTS"])
    else {
        return;
    };
    strip_decorations(&session, window, motif);
    set_atom_list(&session, window, state, &[fullscreen]);
    send_to_window_manager(
        &session,
        window,
        state,
        [STATE_ADD, fullscreen, 0, SOURCE_APPLICATION, 0],
    );
    session.flush();

    tracing::debug!(
        window_xid,
        "[X11Fullscreen: Enforce] Enforced true EWMH fullscreen state and stripped decorations via X11"
    );
}

/// Enforces true EWMH fullscreen state on a specific monitor using `_NET_WM_FULLSCREEN_MONITORS`.
pub fn force_x11_fullscreen_on_monitor(window_xid: u64, monitor_index: usize, x: i32, y: i32, width: u32, height: u32) {
    use x11rb::protocol::xproto::ConfigureWindowAux;

    let Some(window) = u32::try_from(window_xid).ok().filter(|&xid| xid != 0) else {
        return;
    };
    let Ok(session) = Session::open("the fullscreen on a monitor") else {
        return;
    };
    let Some([state, fullscreen, monitors, motif]) = session.atoms([
        "_NET_WM_STATE",
        "_NET_WM_STATE_FULLSCREEN",
        "_NET_WM_FULLSCREEN_MONITORS",
        "_MOTIF_WM_HINTS",
    ]) else {
        return;
    };
    let monitor = u32::try_from(monitor_index).unwrap_or(0);

    // 1. Strip window decorations.
    strip_decorations(&session, window, motif);

    // 2. Set the property _NET_WM_FULLSCREEN_MONITORS on the client window.
    let _ = session
        .conn
        .change_property32(PropMode::REPLACE, window, monitors, AtomEnum::CARDINAL, &[monitor; 4]);

    // 3. Ask the window manager through the root window.
    send_to_window_manager(
        &session,
        window,
        monitors,
        [monitor, monitor, monitor, monitor, SOURCE_APPLICATION],
    );

    // 4. Move and resize the window to the bounds of the target monitor.
    let _ = session.conn.configure_window(
        window,
        &ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(width.max(16))
            .height(height.max(16)),
    );

    // 5. Set the _NET_WM_STATE property and 6. ask for the fullscreen state.
    set_atom_list(&session, window, state, &[fullscreen]);
    send_to_window_manager(
        &session,
        window,
        state,
        [STATE_ADD, fullscreen, 0, SOURCE_APPLICATION, 0],
    );
    session.flush();

    tracing::info!(
        window_xid,
        monitor_index,
        x,
        y,
        width,
        height,
        "[X11Fullscreen: TargetMonitor] Positioned and fullscreened window onto target monitor via EWMH"
    );
}
