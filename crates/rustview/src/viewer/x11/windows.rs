//! Finding the X11 window of the application: from the window handle of Slint, or by searching the window tree for the process.

use super::session::Session;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Window};

/// Retrieves the raw native X11 Window ID directly from Slint's internal window handle with zero heuristic searching.
pub fn get_raw_window_xid(window: &slint::Window) -> Option<u64> {
    if let Ok(handle) = window.window_handle().window_handle() {
        match handle.as_raw() {
            RawWindowHandle::Xlib(h) => {
                if h.window > 0 {
                    return Some(h.window);
                }
            }
            RawWindowHandle::Xcb(h) => {
                let xid = u64::from(h.window.get());
                if xid > 0 {
                    return Some(xid);
                }
            }
            _ => {}
        }
    }
    None
}

/// Retrieves the native X11 Window ID (`XID`) directly from a Slint Window with "`RustView`" title fallback.
pub fn get_slint_window_xid(window: &slint::Window) -> Option<u64> {
    get_slint_window_xid_with_title_fallback(window, "RustView")
}

/// Retrieves the native X11 Window ID (`XID`) from a Slint Window with process PID and strict title verification.
pub fn get_slint_window_xid_with_title_fallback(window: &slint::Window, fallback_title: &str) -> Option<u64> {
    if let Some(xid) = get_raw_window_xid(window) {
        return Some(xid);
    }

    find_window_by_pid_and_title(std::process::id(), fallback_title)
}

/// Discovers the native X11 Window ID (`XID`) belonging to `target_pid` whose title contains `title_contains`.
/// Performs STRICT title matching to prevent accidentally capturing other windows of the same process.
pub fn find_window_by_pid_and_title(target_pid: u32, title_contains: &str) -> Option<u64> {
    let session = Session::open("the window search").ok()?;
    let [atom_pid, net_wm_name, wm_name, utf8, string] =
        session.atoms(["_NET_WM_PID", "_NET_WM_NAME", "WM_NAME", "UTF8_STRING", "STRING"])?;
    let atoms = TitleAtoms {
        names: [net_wm_name, wm_name],
        types: [utf8, string, u32::from(AtomEnum::ANY)],
    };
    let top_level = session.conn.query_tree(session.root).ok()?.reply().ok()?.children;

    for window in top_level.into_iter().rev() {
        if window_pid(&session, window, atom_pid) != Some(target_pid) {
            continue;
        }
        if title_contains.is_empty() || window_tree_has_title(&session, &atoms, window, title_contains) {
            return Some(u64::from(window));
        }
    }
    None
}

pub fn find_window_by_title(title_contains: &str) -> Option<u64> {
    find_window_by_pid_and_title(std::process::id(), title_contains)
}

fn window_pid(session: &Session, window: Window, atom_pid: Atom) -> Option<u32> {
    let reply = session
        .conn
        .get_property(false, window, atom_pid, AtomEnum::ANY, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next()
}

/// The atoms needed to read a window title: its property names and the types it may be stored as.
struct TitleAtoms {
    names: [Atom; 2],
    types: [Atom; 3],
}

/// Whether the window or any window below it has a title that matches `title_match`.
fn window_tree_has_title(session: &Session, atoms: &TitleAtoms, window: Window, title_match: &str) -> bool {
    if window_title_matches(session, atoms, window, title_match) {
        return true;
    }
    let Some(children) = session
        .conn
        .query_tree(window)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.children)
    else {
        return false;
    };
    children
        .into_iter()
        .rev()
        .any(|child| window_tree_has_title(session, atoms, child, title_match))
}

fn window_title_matches(session: &Session, atoms: &TitleAtoms, window: Window, title_match: &str) -> bool {
    for name_atom in atoms.names {
        for requested_type in atoms.types {
            let Some(reply) = session
                .conn
                .get_property(false, window, name_atom, requested_type, 0, 256)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            else {
                continue;
            };
            if reply.value.is_empty() {
                continue;
            }
            let title = String::from_utf8_lossy(&reply.value);
            // Match window title strictly to avoid matching "RustView - Settings" when searching for "RustView".
            let matches = if title_match == "RustView" {
                title.trim() == "RustView"
            } else {
                title.contains(title_match)
            };
            if matches {
                return true;
            }
        }
    }
    false
}
