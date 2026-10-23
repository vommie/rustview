//! Reading where the mouse pointer is.

use super::session::Session;
use crate::viewer::ViewerError;
use x11rb::protocol::xproto::{ConnectionExt as _, Window};

/// Reads the global pointer position with a single `QueryPointer` round trip on a private connection.
pub struct PointerProbe {
    session: Session,
}

impl PointerProbe {
    pub fn new() -> Result<Self, ViewerError> {
        Ok(Self { session: Session::open("the pointer probe")? })
    }

    /// Pointer position in root window (screen) coordinates.
    pub fn position(&self) -> Option<(i32, i32)> {
        self.query(self.session.root).map(|(root_x, root_y, _, _)| (root_x, root_y))
    }

    /// Pointer position relative to the top-left corner of `window_xid` (it does not have to contain the pointer).
    pub fn position_in(&self, window_xid: u64) -> Option<(i32, i32)> {
        let window = u32::try_from(window_xid).ok()?;
        self.query(window).map(|(_, _, win_x, win_y)| (win_x, win_y))
    }

    /// Returns `(root_x, root_y, win_x, win_y)` of the pointer, the last two relative to `window`.
    fn query(&self, window: Window) -> Option<(i32, i32, i32, i32)> {
        let reply = self.session.conn.query_pointer(window).ok()?.reply().ok()?;
        reply
            .same_screen
            .then_some((i32::from(reply.root_x), i32::from(reply.root_y), i32::from(reply.win_x), i32::from(reply.win_y)))
    }
}
