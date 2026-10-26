//! Makes a Slint window a host of video surfaces (`viewer::video_surface::hosts`): its `video-frames` model, a redraw request
//! that works from any thread, its native display and the forwarding of its rendering notifier.

use crate::viewer::mpv_engine::NativeDisplay;
use crate::viewer::video_surface::{HostWindowId, hosts};
use raw_window_handle::{HasDisplayHandle as _, RawDisplayHandle};
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::rc::Rc;
use std::sync::Arc;

/// The native display of a window (the X display of an Xlib connection, or the Wayland display), once it exists.
fn native_display(window: &slint::Window) -> NativeDisplay {
    let handle = window.window_handle();
    let Ok(display) = handle.display_handle() else {
        return NativeDisplay::None;
    };
    match display.as_raw() {
        RawDisplayHandle::Xlib(xlib) => xlib
            .display
            .map_or(NativeDisplay::None, |d| NativeDisplay::X11(d.as_ptr())),
        RawDisplayHandle::Wayland(wayland) => NativeDisplay::Wayland(wayland.display.as_ptr()),
        _ => NativeDisplay::None,
    }
}

/// Registers `win` as host `id`: `set_frames` hands it the model of `video-frames`. The rendering notifier must forward to
/// `hosts::on_rendering` (`install_notifier`, or the window's own notifier for the main window).
pub fn register<W: ComponentHandle + 'static>(win: &W, id: HostWindowId, set_frames: impl FnOnce(&W, ModelRc<Image>)) {
    let frames: Rc<VecModel<Image>> = Rc::new(VecModel::default());
    set_frames(win, ModelRc::from(frames.clone()));
    let weak = win.as_weak();
    let redraw: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = weak.upgrade_in_event_loop(|w| w.window().request_redraw());
    });
    let weak = win.as_weak();
    let display = Box::new(move || {
        weak.upgrade()
            .map_or(NativeDisplay::None, |w| native_display(w.window()))
    });
    hosts::register_window(id, frames, redraw, display);
}

/// A rendering notifier that only forwards to the video hosts (for windows without a notifier of their own).
pub fn install_notifier<W: ComponentHandle>(win: &W, id: HostWindowId) {
    if let Err(e) = win
        .window()
        .set_rendering_notifier(move |state, api| hosts::on_rendering(id, &state, api))
    {
        tracing::error!(window = id, error = %e, "[VideoSurface: Host] No rendering notifier; videos cannot be drawn here");
    }
}
