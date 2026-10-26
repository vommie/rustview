//! Where the pointer is over a window that shows a Multiview, from the window's own pointer events (UI thread only).
//!
//! The hosts (the fullscreen window, the tab view of the main window) need the pointer in window coordinates even where a
//! touch area of the canvas sits on top of another one (Slint reports hover only to the topmost), so they read the events winit
//! delivers to the window before Slint handles them: every movement and leaving the window, nothing is polled. The events are
//! passed on to Slint untouched. A window has one such filter; this module is its only user.

use rustview_core::util::cast::FloatToInt;
use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor};

/// Calls `on_pointer` with the pointer in physical pixels of `window` whenever it moves over the window, and with `None` when it
/// leaves the window. Once per window, right after it is made.
pub fn install(window: &slint::Window, on_pointer: impl Fn(Option<(i32, i32)>) + 'static) {
    window.on_winit_window_event(move |_, event| {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                on_pointer(Some((
                    position.x.round().to_int::<i32>(),
                    position.y.round().to_int::<i32>(),
                )));
            }
            WindowEvent::CursorLeft { .. } => on_pointer(None),
            _ => {}
        }
        EventResult::Propagate
    });
}
