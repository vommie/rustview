//! Locks the mouse pointer while the minimap is dragged, so a slow, fine drag is not stopped by the edge of the screen.
//!
//! A fine drag moves the picture by less than the pointer moves, so it needs more room than the minimap (which sits in a corner of
//! the screen) has. The way applications solve this (Blender's "continuous grab", games, 3D viewports): hide the pointer, hold it
//! in one place and read only how far it was moved, then put it back where the drag started. The pointer is put in the middle of
//! the host window when the drag starts (so there is room in every direction), every movement is read as the distance from there
//! and the pointer is put back at once, and the release puts it where it was pressed.
//!
//! This is done on the X server (`XWarpPointer`, the cursor hidden with `XFixes`). Where it does not work (Wayland, a pointer that
//! does not follow, no X connection) `begin` says so and the drag stays absolute: the rectangle under the pointer.

use crate::viewer::x11::OverlayServer;
use std::cell::RefCell;

struct Lock {
    server: OverlayServer,
    /// Where the pointer is held (screen coordinates) and where it goes back to.
    center: (i32, i32),
    restore: (i32, i32),
    active: bool,
}

thread_local! {
    static LOCK: RefCell<Option<Lock>> = const { RefCell::new(None) };
}

/// Locks the pointer to the host window `host_xid`. Returns whether it is locked now.
pub fn begin(host_xid: u64) -> bool {
    if host_xid == 0 {
        return false;
    }
    LOCK.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            match OverlayServer::new() {
                Ok(server) => {
                    *slot = Some(Lock {
                        server,
                        center: (0, 0),
                        restore: (0, 0),
                        active: false,
                    });
                }
                Err(_) => return false,
            }
        }
        let Some(lock) = slot.as_mut() else { return false };
        if lock.active {
            return true;
        }
        let server = &lock.server;
        let (Some(((px, py), down)), Some(origin), Some((w, h))) = (
            server.pointer_and_buttons(),
            server.origin_in_root(host_xid),
            server.window_size(host_xid),
        ) else {
            return false;
        };
        if !down || w < 40 || h < 40 {
            return false;
        }
        let center = (origin.0 + w / 2, origin.1 + h / 2);
        server.warp_pointer(center.0, center.1);
        // The pointer must have followed (it does not under Wayland); else the drag stays absolute.
        let followed = server
            .pointer_and_buttons()
            .is_some_and(|((x, y), _)| (x - center.0).abs() <= 2 && (y - center.1).abs() <= 2);
        if !followed {
            server.warp_pointer(px, py);
            return false;
        }
        if !server.hide_cursor() {
            tracing::debug!("[Zoom: Minimap] The cursor could not be hidden (no libXfixes)");
        }
        lock.center = center;
        lock.restore = (px, py);
        lock.active = true;
        tracing::debug!("[Zoom: Minimap] Pointer locked for the drag");
        true
    })
}

/// What a host does for a minimap pointer event of `phase` (0 press, 1 move, 2 release, 3/4 clicks): the lock begins on the press
/// and ends on the release, and `apply` is called with the pointer's movement (`Some`) while it is locked, or with `None` for an
/// ordinary event (the rectangle follows the pointer).
pub fn drag_phase(host_xid: u64, phase: i32, apply: &mut dyn FnMut(Option<(i32, i32)>)) {
    match phase {
        0 => {
            apply(None);
            begin(host_xid);
        }
        1 => {
            if is_active() {
                if let Some(delta) = take_delta() {
                    apply(Some(delta));
                }
            } else {
                apply(None);
            }
        }
        2 => {
            end();
            apply(None);
        }
        _ => apply(None),
    }
}

pub fn is_active() -> bool {
    LOCK.with(|cell| cell.borrow().as_ref().is_some_and(|l| l.active))
}

/// How far the pointer was moved since the last call (device pixels), put back in the middle at once. `None` when it did not move
/// or nothing is locked. A lock whose button is up (a release that was missed) ends here.
pub fn take_delta() -> Option<(i32, i32)> {
    LOCK.with(|cell| {
        let mut slot = cell.borrow_mut();
        let lock = slot.as_mut().filter(|l| l.active)?;
        let ((x, y), down) = lock.server.pointer_and_buttons()?;
        if !down {
            finish(lock);
            return None;
        }
        let delta = (x - lock.center.0, y - lock.center.1);
        if delta == (0, 0) {
            return None;
        }
        lock.server.warp_pointer(lock.center.0, lock.center.1);
        Some(delta)
    })
}

/// Lets go: the pointer is back where the drag started and visible again.
pub fn end() {
    LOCK.with(|cell| {
        if let Some(lock) = cell.borrow_mut().as_mut().filter(|l| l.active) {
            finish(lock);
        }
    });
}

/// Called on every tick of a host: a lock that outlived its mouse button (the release never arrived) ends.
pub fn service() {
    LOCK.with(|cell| {
        if let Some(lock) = cell.borrow_mut().as_mut().filter(|l| l.active)
            && lock.server.pointer_and_buttons().is_none_or(|(_, down)| !down)
        {
            finish(lock);
        }
    });
}

fn finish(lock: &mut Lock) {
    lock.server.warp_pointer(lock.restore.0, lock.restore.1);
    lock.server.show_cursor();
    lock.active = false;
    tracing::debug!("[Zoom: Minimap] Pointer released");
}
