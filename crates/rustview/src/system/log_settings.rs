//! Applies the log options of Settings > Debug (`docs/debug_page.md`) at startup and after Save.

use super::app_dirs;
use rustview_core::logging::{self, LogLevel};
use rustview_core::settings::{DebugSettings, LogObscurer};

/// Obfuscation, level, log files and `mpv.log` as the settings say. A file that cannot be started is logged and stays off.
pub fn apply(debug: &DebugSettings) {
    LogObscurer::set_obfuscate(debug.obfuscate_logs);
    logging::set_level(LogLevel::from_setting(&debug.log_level));
    let dir = app_dirs::log_dir();
    if let Err(error) = logging::set_files(debug.log_files.then_some(dir.as_path())) {
        tracing::warn!(%error, folder = %dir.display(), "[Logging] The log files could not be started");
    }
    if let Err(error) = logging::set_mpv_log(debug.mpv_log.then_some(dir.as_path())) {
        tracing::warn!(%error, folder = %dir.display(), "[Logging] mpv.log could not be started");
    }
}
