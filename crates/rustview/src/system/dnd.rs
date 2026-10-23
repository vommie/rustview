use crate::icons::Icon;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::LazyLock;

mod drop;
mod ghost;

/// Edge length of the ghost window's thumbnail/icon slot (matches the scaling in the helper script below).
const GHOST_ICON_SIZE_PX: u32 = 24;
/// Color of the ghost fallback icon; equals `Theme.text` in `ui/styles.slint` (the helper window cannot read the theme).
const GHOST_ICON_COLOR: &str = "#cdd6f4";

static ACTIVE_GHOST_CHILD: LazyLock<Mutex<Option<Child>>> = LazyLock::new(|| Mutex::new(None));

/// Detailed telemetry metrics for external Drag & Drop export sessions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DndExportMetrics {
    pub files_count: usize,
    pub total_bytes: u64,
    pub session_type: &'static str,
    pub target_window_id: u64,
    pub target_app_class: String,
    pub elapsed_us: u128,
    pub success: bool,
}

/// Centralized Linux Drag and Drop export subsystem (supporting modern Wayland and X11 protocols).
/// Handles seamless dragging and dropping of files into 3rd-party applications
/// (web browsers, media players, chat apps, image editors, and file managers) via direct XDND transactions.
pub struct SystemDndHandler;

impl SystemDndHandler {
    /// Detects whether the current Linux session is running on Wayland or X11.
    #[inline]
    pub fn detect_session_type() -> &'static str {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
        {
            "Wayland"
        } else {
            "X11"
        }
    }

    /// Converts a local filesystem path into an RFC 2483 compliant `file://` URI string.
    pub fn path_to_file_uri(path: &Path) -> String {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
        };

        url::Url::from_file_path(&abs_path)
            .map_or_else(|()| format!("file://{}", abs_path.to_string_lossy()), String::from)
    }

    /// Formats a slice of filesystem paths into a standard CRLF-delimited `text/uri-list` payload.
    pub fn format_uri_list<P: AsRef<Path>>(paths: &[P]) -> String {
        let mut out = String::new();
        for p in paths {
            let uri = Self::path_to_file_uri(p.as_ref());
            out.push_str(&uri);
            out.push_str("\r\n");
        }
        out
    }

    /// Formats a slice of filesystem paths into a newline-delimited plain path list.
    pub fn format_plain_paths<P: AsRef<Path>>(paths: &[P]) -> String {
        paths
            .iter()
            .map(|p| p.as_ref().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Renders (once) and returns the generic file icon PNG used by the ghost window when no thumbnail exists.
    fn fallback_icon_path() -> Option<PathBuf> {
        let dest = std::env::temp_dir()
            .join("rustview")
            .join("icons")
            .join(format!("ghost-file-{GHOST_ICON_SIZE_PX}.png"));
        if !dest.is_file()
            && let Err(e) = crate::icons::raster::render_png(Icon::File, GHOST_ICON_SIZE_PX, GHOST_ICON_COLOR, &dest)
        {
            tracing::warn!(error = %e, "[SystemDnd] Cannot render ghost fallback icon");
            return None;
        }
        Some(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::SystemDndHandler;
    use std::path::Path;

    #[test]
    fn file_uri_escapes_spaces_hashes_and_unicode() {
        assert_eq!(
            SystemDndHandler::path_to_file_uri(Path::new("/home/u/My Photos/a #1 \u{fc}.jpg")),
            "file:///home/u/My%20Photos/a%20%231%20%C3%BC.jpg"
        );
    }

    #[test]
    fn uri_list_is_crlf_terminated() {
        let list = SystemDndHandler::format_uri_list(&["/a/b.jpg", "/c d.png"]);
        assert_eq!(list, "file:///a/b.jpg\r\nfile:///c%20d.png\r\n");
    }
}
