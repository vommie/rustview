//! What the developer probe (`ui/perf_probe`) needs from the hosts of a Multiview: the fullscreen window, and a way in for
//! the pointer the hosts normally read from winit (the probe's events enter Slint behind that filter).

use super::{session, tab_host};
use crate::ui::MultiviewFullscreenWindow;

/// The fullscreen window once it shows its media.
pub fn fullscreen_window() -> Option<MultiviewFullscreenWindow> {
    session::populated_window()
}

/// The pointer moved to `pointer` (physical pixels of the window) over the fullscreen window or over the main window.
pub fn pointer_moved(fullscreen: bool, pointer: (i32, i32)) {
    if fullscreen {
        session::with_session(|s| s.on_pointer(Some(pointer)));
    } else {
        tab_host::on_pointer(Some(pointer));
    }
}
