//! When a drop-down menu of the media bar closes (pure).
//!
//! The menu hangs below the bar in the bar's own override-redirect window, which cannot take a pointer grab or the keyboard
//! focus. A click elsewhere therefore reaches the Multiview, not the menu; the bar polls the pointer instead and closes the menu
//! when a mouse button goes down outside of it.

use super::layout::Rect;

/// Turns polled pointer states into "a button was pressed outside the menu".
#[derive(Debug, Default)]
pub struct OutsidePress {
    was_down: bool,
}

impl OutsidePress {
    /// Starts watching with the current button state, so that the click that opened the menu (still held) does not count.
    pub fn arm(&mut self, down: bool) {
        self.was_down = down;
    }

    /// Feeds one poll. True for the poll that sees a new press outside every rectangle of `inside` (the bar and the menu).
    pub fn poll(&mut self, pointer: (i32, i32), down: bool, inside: &[Rect]) -> bool {
        let pressed = down && !self.was_down;
        self.was_down = down;
        pressed && !inside.iter().any(|r| r.contains(pointer.0, pointer.1))
    }
}
