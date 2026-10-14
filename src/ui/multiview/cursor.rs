//! Hiding the mouse cursor over a host window while it rests (UI thread only).
//!
//! The Slint window covers images, gutters and margins; mpv hides the cursor over a video itself (the instance sets its
//! `cursor-autohide`). Here only the window part is handled, and only while the host asks for it: the cursor is shown again
//! the moment the pointer moves, and it is never hidden outside the Multiview because the window only hides it while the
//! pointer is idle inside the Multiview.

use slint::winit_030::WinitWindowAccessor;

pub(super) fn set_hidden(window: &slint::Window, hidden: bool) {
    window.with_winit_window(|w| w.set_cursor_visible(!hidden));
    tracing::debug!(hidden, "[Multiview: Cursor] Mouse cursor visibility changed");
}
