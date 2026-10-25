//! The folders `RustView` keeps its files in (XDG base directories): the settings, the library with the thumbnails, and the
//! log files (only written while the user has switched them on).

use directories::ProjectDirs;
use std::path::PathBuf;

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "rustview")
}

/// `$XDG_DATA_HOME/rustview`: the library database and the thumbnails.
pub fn data_dir() -> PathBuf {
    project_dirs().map_or_else(|| PathBuf::from("./data"), |dirs| dirs.data_dir().to_path_buf())
}

/// `$XDG_CONFIG_HOME/rustview`: `settings.json`.
pub fn config_dir() -> PathBuf {
    project_dirs().map_or_else(|| PathBuf::from("./config"), |dirs| dirs.config_dir().to_path_buf())
}

/// `$XDG_STATE_HOME/rustview`: the log files and `mpv.log`.
pub fn log_dir() -> PathBuf {
    project_dirs()
        .and_then(|dirs| dirs.state_dir().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| data_dir().join("logs"))
}
