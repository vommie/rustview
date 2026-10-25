//! Everything the viewer needs from the X server, written on x11rb (a pure Rust implementation of the X11 protocol): the native
//! video windows that mpv draws into, the floating action bar windows, the window manager state of the main window and the pointer.
//! The one exception is `error_handler`: libmpv draws through Xlib and installs a process-wide Xlib error handler.

mod dialog_hints;
mod error_handler;
mod ghost;
mod overlay_server;
mod pointer;
mod server_facts;
mod session;
mod subwindow;
mod windows;
mod wm_state;
mod xdnd;

pub use dialog_hints::{request_x11_modal_state, set_x11_dialog_parent};
pub use error_handler::install_nonfatal_error_handler;
pub use ghost::{GhostPixels, GhostWindow};
pub use overlay_server::OverlayServer;
pub use pointer::PointerProbe;
pub use server_facts::x11_server_facts;
pub use session::x11_display_answers;
pub use subwindow::{SubwindowGeometry, ViewportBounds, X11VideoSubwindow, XRectangle};
pub use windows::{
    find_window_by_pid_and_title, find_window_by_title, get_raw_window_xid, get_slint_window_xid,
    get_slint_window_xid_with_title_fallback,
};
pub use wm_state::{
    force_x11_fullscreen, force_x11_fullscreen_on_monitor, force_x11_maximized, is_x11_window_maximized,
    set_x11_initial_maximized, x11_root_size,
};
pub use xdnd::{XdndOutcome, drop_on_window_under_pointer};
