//! libmpv's own messages (`docs/debug_page.md`): warnings and errors become log lines of `RustView` (the file name of the video
//! is hidden in the private log), and while `mpv.log` is switched on every message at mpv's `v` level is written there.

use libmpv2::Mpv;
use rustview_core::logging;
use rustview_core::settings::LogObscurer;
use std::path::Path;

/// Asks libmpv for its messages: `v` (detailed) while `mpv.log` is on, else warnings and errors only. Applies to this player for
/// its whole life.
pub(super) fn request(mpv: &Mpv) {
    let level = if logging::mpv_log_running() { c"v" } else { c"warn" };
    // SAFETY: `mpv.ctx` is the live handle owned by `mpv` (it outlives the call) and `level` is a NUL-terminated string.
    let result = unsafe { libmpv2_sys::mpv_request_log_messages(mpv.ctx.as_ptr(), level.as_ptr()) };
    if result < 0 {
        tracing::warn!(
            error = result,
            "[MpvPlayer: Messages] libmpv does not send its messages"
        );
    }
}

/// Handles one message of libmpv. `file` is the video the player shows (hidden in the private log).
pub(super) fn handle(prefix: &str, level: &str, text: &str, file: Option<&Path>) {
    let text = text.trim_end();
    if text.is_empty() {
        return;
    }
    if logging::mpv_log_running() {
        logging::write_mpv(&format!(
            "{} [{prefix}] {level}: {text}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
        ));
    }
    match level {
        "fatal" | "error" => tracing::error!("[mpv: {prefix}] {}", LogObscurer::foreign_text(text, file)),
        "warn" => tracing::warn!("[mpv: {prefix}] {}", LogObscurer::foreign_text(text, file)),
        _ => {}
    }
}
