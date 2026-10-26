//! The window system `RustView` runs on (X11, `XWayland` or native Wayland), chosen once at the start of `main`, before the first
//! window exists.
//!
//! In a Wayland session `RustView` runs as an X11 program through `XWayland` (user decision 2026-10-25, `docs/rendering.md`): the
//! X11 features (dialogs that belong to their window, dragging files to other programs, the pointer lock) exist only there.
//! Videos and everything over them are drawn by Slint since `video-in-slint`, so native Wayland shows them too; it becomes the
//! default after the joint test of M7 (`docs/video_in_slint/WAYLAND.md`). Every place
//! that depends on the window system (the clipboard tool, drag and drop, the logs) asks [`current`] instead of reading the
//! environment, which says "Wayland" while the windows are X11 windows.

mod choice;

pub use choice::{Choice, Decision, REQUEST_VARIABLE, RequestNote, SessionEnv, WaylandReason, WindowSystem, choose};

use slint::winit_030::SlintEvent;
use slint::winit_030::winit::event_loop::EventLoop;
use slint::winit_030::winit::platform::wayland::EventLoopBuilderExtWayland;
use slint::winit_030::winit::platform::x11::EventLoopBuilderExtX11;
use std::sync::OnceLock;

static CHOSEN: OnceLock<Choice> = OnceLock::new();

/// Reads what the choice needs from the environment of the process.
pub fn session_env() -> SessionEnv {
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    SessionEnv {
        wayland_display: set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET"),
        wayland_session: std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland")),
        x11_display: set("DISPLAY"),
        request: std::env::var(REQUEST_VARIABLE).ok(),
    }
}

/// Chooses the window system (once; in a Wayland session the X display is asked with one short connection), logs the choice and
/// keeps it for [`current`]. Called at the start of `main`, before Slint's backend is selected.
pub fn decide() -> Choice {
    *CHOSEN.get_or_init(|| {
        let decision = choose(&session_env(), || match crate::viewer::x11_display_answers() {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "[Startup: WindowSystem] XWayland does not answer");
                false
            }
        });
        log_decision(&decision);
        decision.choice
    })
}

/// The window system chosen at startup. Before [`decide`] ran (unit tests, the command line API) it is the environment's own
/// answer, without asking the X display.
pub fn current() -> Choice {
    CHOSEN
        .get()
        .copied()
        .unwrap_or_else(|| choose(&session_env(), || true).choice)
}

/// Makes Slint's winit backend open the chosen window system; winit itself would take Wayland whenever `WAYLAND_DISPLAY` is set.
/// The backend is forced through winit's event loop builder, not through the environment, so the programs `RustView` starts (a file
/// manager, "Open With" applications) still see the session as it is.
pub fn select_backend(selector: slint::BackendSelector, choice: Choice) -> slint::BackendSelector {
    let mut event_loop = EventLoop::<SlintEvent>::with_user_event();
    match choice.window_system() {
        Some(WindowSystem::X11) => {
            event_loop.with_x11();
        }
        Some(WindowSystem::Wayland) => {
            event_loop.with_wayland();
        }
        None => return selector,
    }
    selector.with_winit_event_loop_builder(event_loop)
}

fn log_decision(decision: &Decision) {
    match &decision.note {
        Some(RequestNote::Unknown(value)) => tracing::warn!(
            variable = REQUEST_VARIABLE,
            value = %value,
            "[Startup: WindowSystem] Unknown value ignored (known: wayland, auto)"
        ),
        Some(RequestNote::WaylandUnavailable) => tracing::warn!(
            variable = REQUEST_VARIABLE,
            "[Startup: WindowSystem] Native Wayland was requested, but the session has no Wayland display"
        ),
        None => {}
    }
    let window_system = decision.choice.label();
    match decision.choice {
        Choice::X11Session => tracing::info!(
            window_system,
            "[Startup: WindowSystem] X11 session; RustView runs on X11"
        ),
        Choice::XWayland => tracing::info!(
            window_system,
            "[Startup: WindowSystem] Wayland session; RustView runs as an X11 program through XWayland ({REQUEST_VARIABLE}=wayland for native Wayland)"
        ),
        Choice::Wayland(reason) => {
            let why = match reason {
                WaylandReason::Requested => "requested",
                WaylandReason::NoXDisplay => "DISPLAY is not set (no XWayland)",
                WaylandReason::XDisplayUnreachable => "XWayland does not answer",
            };
            tracing::warn!(
                window_system,
                why,
                "[Startup: WindowSystem] RustView runs on native Wayland: the X11 features (dialog parents, dragging files out, the pointer lock) degrade"
            );
        }
        Choice::NoDisplay => tracing::warn!(
            window_system,
            "[Startup: WindowSystem] Neither WAYLAND_DISPLAY nor DISPLAY is set; no window can be opened"
        ),
    }
}
