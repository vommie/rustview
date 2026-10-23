//! Hiding the mouse cursor over a Multiview while it rests (UI thread only).
//!
//! The Multiview is made of a Slint window (images, gutters, margins) and native video windows on top of it. mpv has
//! nothing to do with it: the cursor is hidden on the Slint window through winit and on the video windows with an invisible
//! X11 cursor (`X11VideoSubwindow::set_cursor_hidden`), so it disappears everywhere over the Multiview, over a video, an
//! image, a resizer or empty space alike, and a single medium is no different. The pointer is polled by the host tick; any
//! movement shows the cursor again. It is only ever hidden while the pointer rests inside the Multiview.

use crate::viewer::multiview::instance::SharedInstance;
use slint::winit_030::WinitWindowAccessor;

/// Hides or shows the cursor over the Slint window itself.
pub fn set_hidden(window: &slint::Window, hidden: bool) {
    window.with_winit_window(|w| w.set_cursor_visible(!hidden));
    tracing::debug!(hidden, "[Multiview: Cursor] Mouse cursor visibility changed");
}

/// Brings the cursor in line with the state of the instance: hidden while the user wants idle cursors hidden, the pointer
/// rests inside the Multiview and no bar is in use. `window_hidden` is what the host remembers about its window (a window
/// can outlive the instance it showed). Called every tick by the host.
pub fn sync(window: &slint::Window, instance: &SharedInstance, bars_visible: bool, window_hidden: &mut bool) {
    let hide = !bars_visible && instance.lock().cursor_should_hide();
    if hide != *window_hidden {
        *window_hidden = hide;
        set_hidden(window, hide);
    }
    let mut guard = instance.lock();
    if guard.is_cursor_hidden() != hide {
        guard.set_cursor_hidden(hide);
    }
}

/// The host stops showing the instance: the cursor is visible again over the window and the videos.
pub fn release(window: &slint::Window, instance: Option<&SharedInstance>, window_hidden: &mut bool) {
    if std::mem::replace(window_hidden, false) {
        set_hidden(window, false);
    }
    if let Some(instance) = instance {
        let mut guard = instance.lock();
        if guard.is_cursor_hidden() {
            guard.set_cursor_hidden(false);
        }
    }
}
