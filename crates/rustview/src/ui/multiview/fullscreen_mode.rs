//! Puts the Multiview fullscreen window into the fullscreen state on the chosen monitor, for the window system `RustView` runs on.
//!
//! X11 (also through `XWayland`): `_NET_WM_STATE` and `_NET_WM_FULLSCREEN_MONITORS` on the window's XID (`viewer::x11::wm_state`).
//! Native Wayland has no XID and the client cannot place a window: winit's borderless fullscreen on the output that matches the
//! monitor's position, or the output the compositor chose for the window when none matches.

use crate::system::MonitorInfo;
use crate::system::window_system::{self, WindowSystem};
use crate::viewer::{force_x11_fullscreen_on_monitor, get_raw_window_xid, is_x11_window_fullscreen};
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit::window::Fullscreen;
use slint::{ComponentHandle, Timer};
use std::time::Duration;

/// When the fullscreen state is checked after the request, and how often it is asked for again. The window is reused for every
/// fullscreen; when it is shown again the request can be lost: on X11 it can reach the window manager before it manages the window
/// again, which then maximizes the screen-sized window instead (2 of 9 opens on the virtualized test machine), and on native Wayland the compositor ignored every
/// second request after the window was hidden and shown again (the panel stayed above it).
const CONFIRM_DELAY: Duration = Duration::from_millis(200);
const CONFIRM_TRIES: u32 = 3;

/// Whether the window system runs the fullscreen without an X11 window.
pub(super) fn is_wayland() -> bool {
    window_system::current().window_system() == Some(WindowSystem::Wayland)
}

/// The window's handle for the fullscreen once it exists: its XID on X11, `Some(0)` on native Wayland as soon as winit made the
/// window, `None` while it does not exist yet.
pub(super) fn native_handle(window: &slint::Window) -> Option<u64> {
    if is_wayland() {
        window.has_winit_window().then_some(0)
    } else {
        get_raw_window_xid(window)
    }
}

/// Enters the fullscreen state on `monitor`; `xid` is the window's XID from [`native_handle`] (0 on Wayland). The state is checked
/// again shortly afterwards and asked for once more when the window manager did not take it.
pub(super) fn enter<C: ComponentHandle + 'static>(component: &C, xid: u64, monitor: &MonitorInfo) {
    request(component.window(), xid, monitor);
    confirm(component.as_weak(), xid, monitor.clone(), 1);
}

fn request(window: &slint::Window, xid: u64, monitor: &MonitorInfo) {
    if xid != 0 {
        force_x11_fullscreen_on_monitor(xid, monitor.index, monitor.x, monitor.y, monitor.width, monitor.height);
        return;
    }
    let matched = window.with_winit_window(|w| {
        let output = w.available_monitors().find(|m| {
            let position = m.position();
            position.x == monitor.x && position.y == monitor.y
        });
        let matched = output.is_some();
        // Leaving the state first makes the request a real change for the compositor after the window was hidden fullscreen.
        w.set_fullscreen(None);
        w.set_fullscreen(Some(Fullscreen::Borderless(output)));
        matched
    });
    tracing::info!(
        monitor = monitor.index,
        output_matched = matched.unwrap_or(false),
        "[Multiview: Open] Wayland borderless fullscreen requested"
    );
}

/// Whether the window is shown in the fullscreen state; `None` when that cannot be told. On Wayland it is the state of the
/// compositor's last configure that winit keeps.
fn is_fullscreen(window: &slint::Window, xid: u64) -> Option<bool> {
    if xid != 0 {
        is_x11_window_fullscreen(xid)
    } else {
        window.with_winit_window(|w| w.fullscreen().is_some())
    }
}

fn confirm<C: ComponentHandle + 'static>(component: slint::Weak<C>, xid: u64, monitor: MonitorInfo, attempt: u32) {
    Timer::single_shot(CONFIRM_DELAY, move || {
        let Some(component) = component.upgrade() else {
            return;
        };
        let window = component.window();
        if !window.is_visible() || is_fullscreen(window, xid) != Some(false) {
            return;
        }
        tracing::warn!(
            window_xid = xid,
            attempt,
            "[Multiview: Open] The window manager did not take the fullscreen state; asking again"
        );
        request(window, xid, &monitor);
        if attempt < CONFIRM_TRIES {
            confirm(component.as_weak(), xid, monitor, attempt + 1);
        }
    });
}
