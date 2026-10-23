//! libmpv draws through Xlib and installs a process-wide Xlib error handler; this is the one place that talks to Xlib itself.

use std::ffi::c_int;
use std::sync::OnceLock;
use x11_dl::xlib::{Display, XErrorEvent, Xlib};

type ErrorHandler = Option<unsafe extern "C" fn(*mut Display, *mut XErrorEvent) -> c_int>;
type SetErrorHandler = unsafe extern "C" fn(ErrorHandler) -> ErrorHandler;

unsafe extern "C" fn nonfatal_x_error(_display: *mut Display, event: *mut XErrorEvent) -> c_int {
    // SAFETY: Xlib hands the handler a valid event for the duration of the call.
    if let Some(event) = unsafe { event.as_ref() } {
        tracing::debug!(
            error_code = event.error_code,
            request_code = event.request_code,
            minor_code = event.minor_code,
            resource_id = event.resourceid,
            "[X11: Error] Ignored non-fatal X protocol error"
        );
    }
    0
}

/// `XSetErrorHandler` of the Xlib that this process already uses (libmpv, Slint); `None` without Xlib.
fn set_error_handler() -> Option<SetErrorHandler> {
    static SET_HANDLER: OnceLock<Option<SetErrorHandler>> = OnceLock::new();
    *SET_HANDLER.get_or_init(|| {
        let xlib = Xlib::open().ok()?;
        let function = xlib.XSetErrorHandler;
        // The library stays loaded for the life of the process: the function pointer must stay valid.
        Box::leak(Box::new(xlib));
        Some(function)
    })
}

/// Installs an X error handler that only logs. libmpv resets the process-wide Xlib error handler to Xlib's default when a
/// video output shuts down, and that default terminates the whole process on any later X error (for example a stale
/// window id). Call this after a player has been destroyed.
pub fn install_nonfatal_error_handler() {
    if let Some(set_handler) = set_error_handler() {
        // SAFETY: the handler is an `extern "C"` function that only reads the event it is given.
        unsafe {
            set_handler(Some(nonfatal_x_error));
        }
    }
}
