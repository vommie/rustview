//! What a floating overlay window (the action bar of the Multiview) needs from the X server, on a private connection:
//! whether a compositing manager blends windows, where a window is on the screen, stacking and the input and bounding
//! shape of the overlay. Everything here is a single round trip; nothing touches the drive.

use super::session::{Session, x11_error};
use super::subwindow::{XRectangle, protocol_rectangles};
use crate::viewer::ViewerError;
use std::sync::atomic::{AtomicBool, Ordering};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, StackMode, Window};

/// The mouse buttons that count as "held down" in the modifier mask of a pointer query (buttons 1 to 3).
const BUTTONS: u16 = (1 << 8) | (1 << 9) | (1 << 10);

pub struct OverlayServer {
    session: Session,
    cursor_hidden: AtomicBool,
}

impl OverlayServer {
    pub fn new() -> Result<Self, ViewerError> {
        Ok(Self { session: Session::open("the overlay server")?, cursor_hidden: AtomicBool::new(false) })
    }

    fn window(xid: u64) -> Option<Window> {
        u32::try_from(xid).ok()
    }

    /// True while a compositing manager owns the `_NET_WM_CM_S<screen>` selection: only then are the alpha channels of
    /// windows blended with what is below them. Without one a window with an alpha channel shows its raw color bits.
    pub fn compositor_running(&self) -> bool {
        let Some(atom) = self.session.atom(&format!("_NET_WM_CM_S{}", self.session.screen), true) else {
            return false;
        };
        self.session
            .conn
            .get_selection_owner(atom)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.owner != x11rb::NONE)
    }

    /// The color depth of a window (32 = it has an alpha channel).
    pub fn window_depth(&self, window: u64) -> Option<u32> {
        let geometry = self.session.conn.get_geometry(Self::window(window)?).ok()?.reply().ok()?;
        Some(u32::from(geometry.depth))
    }

    /// The position of the top-left corner of a window's client area on the screen (root window coordinates).
    pub fn origin_in_root(&self, window: u64) -> Option<(i32, i32)> {
        let reply = self
            .session
            .conn
            .translate_coordinates(Self::window(window)?, self.session.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        reply.same_screen.then_some((i32::from(reply.dst_x), i32::from(reply.dst_y)))
    }

    /// The pointer in screen coordinates and whether a mouse button is held down (one round trip, `QueryPointer`).
    pub fn pointer_and_buttons(&self) -> Option<((i32, i32), bool)> {
        let reply = self.session.conn.query_pointer(self.session.root).ok()?.reply().ok()?;
        reply
            .same_screen
            .then_some(((i32::from(reply.root_x), i32::from(reply.root_y)), u16::from(reply.mask) & BUTTONS != 0))
    }

    /// The top level window (a child of the root window) that is in front at the pointer, or `None` if the pointer is on
    /// another screen. A window of another application that covers the pointer position is the answer then, not the
    /// window below it.
    pub fn top_level_at_pointer(&self) -> Option<u64> {
        let reply = self.session.conn.query_pointer(self.session.root).ok()?.reply().ok()?;
        (reply.same_screen && reply.child != x11rb::NONE).then_some(u64::from(reply.child))
    }

    /// Puts the pointer at (`x`, `y`) on the screen (root window coordinates). The caller checks the result with
    /// `pointer_and_buttons`: under XWayland this has no effect.
    pub fn warp_pointer(&self, x: i32, y: i32) {
        let to = |value: i32| i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX });
        let _ = self.session.conn.warp_pointer(x11rb::NONE, self.session.root, 0, 0, 0, 0, to(x), to(y));
        self.session.sync();
    }

    /// The size of a window.
    pub fn window_size(&self, window: u64) -> Option<(i32, i32)> {
        let geometry = self.session.conn.get_geometry(Self::window(window)?).ok()?.reply().ok()?;
        Some((i32::from(geometry.width), i32::from(geometry.height)))
    }

    /// Hides the mouse cursor (until `show_cursor`, or until this connection ends). Returns whether it could be hidden.
    pub fn hide_cursor(&self) -> bool {
        if self.cursor_hidden.load(Ordering::Relaxed) {
            return true;
        }
        let conn = &self.session.conn;
        // Hiding the cursor needs version 4 of the XFixes extension to be negotiated first.
        let hidden = conn
            .xfixes_query_version(4, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|_| conn.xfixes_hide_cursor(self.session.root).is_ok());
        self.session.sync();
        self.cursor_hidden.store(hidden, Ordering::Relaxed);
        hidden
    }

    pub fn show_cursor(&self) {
        if !self.cursor_hidden.swap(false, Ordering::Relaxed) {
            return;
        }
        let _ = self.session.conn.xfixes_show_cursor(self.session.root);
        self.session.sync();
    }

    /// The window the window manager reports as active (`_NET_ACTIVE_WINDOW`, the client window the user works in), or `None`
    /// if there is none or the window manager does not say.
    pub fn active_window(&self) -> Option<u64> {
        let atom = self.session.atom("_NET_ACTIVE_WINDOW", true)?;
        let reply = self
            .session
            .conn
            .get_property(false, self.session.root, atom, AtomEnum::ANY, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        reply.value32()?.next().filter(|&window| window != 0).map(u64::from)
    }

    /// The top level window that contains `window`: the window itself, or its frame if the window manager reparented it.
    pub fn top_level_of(&self, window: u64) -> Option<u64> {
        let mut current = Self::window(window)?;
        for _ in 0..8 {
            let reply = self.session.conn.query_tree(current).ok()?.reply().ok()?;
            if reply.parent == x11rb::NONE || reply.parent == self.session.root {
                return Some(u64::from(current));
            }
            current = reply.parent;
        }
        None
    }

    /// Puts the window on top of the stack of its siblings (the screen's top level windows).
    pub fn raise(&self, window: u64) {
        if let Some(window) = Self::window(window) {
            let _ = self.session.conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
            self.session.flush();
        }
    }

    /// The part of the window that receives pointer events; everything else passes them to the windows below.
    pub fn set_input_region(&self, window: u64, rects: &[XRectangle]) {
        self.combine(window, shape::SK::INPUT, Some(rects));
    }

    /// The visible shape of the window, or the full rectangle again for `None`.
    pub fn set_bounding_region(&self, window: u64, rects: Option<&[XRectangle]>) {
        self.combine(window, shape::SK::BOUNDING, rects);
    }

    fn combine(&self, window: u64, kind: shape::SK, rects: Option<&[XRectangle]>) {
        let Some(window) = Self::window(window) else { return };
        let conn = &self.session.conn;
        match rects {
            Some(rects) => {
                let _ = conn.shape_rectangles(shape::SO::SET, kind, ClipOrdering::UNSORTED, window, 0, 0, &protocol_rectangles(rects));
            }
            None => {
                let _ = conn.shape_mask(shape::SO::SET, kind, window, 0, 0, x11rb::NONE);
            }
        }
        self.session.flush();
    }
}

impl From<x11rb::errors::ConnectionError> for ViewerError {
    fn from(error: x11rb::errors::ConnectionError) -> Self {
        x11_error(error)
    }
}
