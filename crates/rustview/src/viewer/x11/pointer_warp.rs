//! What the pointer lock of the minimap drag (`ui::multiview::pointer_lock`) needs from the X server, on a private connection:
//! where the pointer and a window are, putting the pointer somewhere and hiding the cursor. Everything here is a single round
//! trip; nothing touches the drive.

use super::session::{Session, x11_error};
use crate::viewer::ViewerError;
use std::sync::atomic::{AtomicBool, Ordering};
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, Window};

/// The mouse buttons that count as "held down" in the modifier mask of a pointer query (buttons 1 to 3).
const BUTTONS: u16 = (1 << 8) | (1 << 9) | (1 << 10);

pub struct PointerWarp {
    session: Session,
    cursor_hidden: AtomicBool,
}

impl PointerWarp {
    pub fn new() -> Result<Self, ViewerError> {
        Ok(Self {
            session: Session::open("the pointer lock")?,
            cursor_hidden: AtomicBool::new(false),
        })
    }

    fn window(xid: u64) -> Option<Window> {
        u32::try_from(xid).ok()
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
        reply
            .same_screen
            .then_some((i32::from(reply.dst_x), i32::from(reply.dst_y)))
    }

    /// The pointer in screen coordinates and whether a mouse button is held down (one round trip, `QueryPointer`).
    pub fn pointer_and_buttons(&self) -> Option<((i32, i32), bool)> {
        let reply = self.session.conn.query_pointer(self.session.root).ok()?.reply().ok()?;
        reply.same_screen.then_some((
            (i32::from(reply.root_x), i32::from(reply.root_y)),
            u16::from(reply.mask) & BUTTONS != 0,
        ))
    }

    /// Puts the pointer at (`x`, `y`) on the screen (root window coordinates). The caller checks the result with
    /// `pointer_and_buttons`: under `XWayland` this has no effect.
    pub fn warp_pointer(&self, x: i32, y: i32) {
        let to = |value: i32| i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX });
        let _ = self
            .session
            .conn
            .warp_pointer(x11rb::NONE, self.session.root, 0, 0, 0, 0, to(x), to(y));
        self.session.sync();
    }

    /// The size of a window.
    pub fn window_size(&self, window: u64) -> Option<(i32, i32)> {
        let geometry = self
            .session
            .conn
            .get_geometry(Self::window(window)?)
            .ok()?
            .reply()
            .ok()?;
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
}

impl From<x11rb::errors::ConnectionError> for ViewerError {
    fn from(error: x11rb::errors::ConnectionError) -> Self {
        x11_error(error)
    }
}
