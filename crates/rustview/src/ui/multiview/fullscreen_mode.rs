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

/// When the fullscreen state of an X11 window is checked after the request, and how often it is asked for again. When the window
/// is shown again (it is reused for every fullscreen), the request can reach the window manager before it manages the window
/// again, and it then maximizes the screen-sized window instead (seen in 2 of 9 opens in the VM, 5 of 12 caught by the check).
const X11_CONFIRM_DELAY: Duration = Duration::from_millis(200);
const X11_CONFIRM_TRIES: u32 = 3;

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

/// Enters the fullscreen state on `monitor`; `xid` is the window's XID from [`native_handle`] (0 on Wayland). On X11 the state
/// is checked again shortly afterwards and asked for once more when the window manager did not take it.
pub(super) fn enter<C: ComponentHandle + 'static>(component: &C, xid: u64, monitor: &MonitorInfo) {
    if xid != 0 {
        force_x11_fullscreen_on_monitor(xid, monitor.index, monitor.x, monitor.y, monitor.width, monitor.height);
        confirm_x11(component.as_weak(), xid, monitor.clone(), 1);
        return;
    }
    let matched = component.window().with_winit_window(|w| {
        let output = w.available_monitors().find(|m| {
            let position = m.position();
            position.x == monitor.x && position.y == monitor.y
        });
        let matched = output.is_some();
        w.set_fullscreen(Some(Fullscreen::Borderless(output)));
        matched
    });
    tracing::info!(
        monitor = monitor.index,
        output_matched = matched.unwrap_or(false),
        "[Multiview: Open] Wayland borderless fullscreen requested"
    );
}

fn confirm_x11<C: ComponentHandle + 'static>(component: slint::Weak<C>, xid: u64, monitor: MonitorInfo, attempt: u32) {
    Timer::single_shot(X11_CONFIRM_DELAY, move || {
        let Some(component) = component.upgrade() else {
            return;
        };
        if !component.window().is_visible() || is_x11_window_fullscreen(xid) != Some(false) {
            return;
        }
        tracing::warn!(
            window_xid = xid,
            attempt,
            "[Multiview: Open] The window manager did not take the fullscreen state; asking again"
        );
        force_x11_fullscreen_on_monitor(xid, monitor.index, monitor.x, monitor.y, monitor.width, monitor.height);
        if attempt < X11_CONFIRM_TRIES {
            confirm_x11(component.as_weak(), xid, monitor, attempt + 1);
        }
    });
}
