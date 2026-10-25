//! Log privacy: file names, paths, folder names, tag names and search queries are hidden in log lines while obfuscation is
//! active.
//!
//! The helpers return an [`Obscured`] value that decides how to print itself when it is FORMATTED, not when the log call is
//! made. That lets one log event be written twice, once with the names (the real log) and once without (the private log to
//! share): the capture layer of `crate::logging` formats each event inside [`LogObscurer::with_mode`]. Everywhere else
//! (the terminal, `println!` of the migration tool) the setting `debug.obfuscate_logs` decides, as before.

mod obscured;

pub use obscured::Obscured;

use crate::util::cast::Az;
use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static OBFUSCATE_LOGS: AtomicBool = AtomicBool::new(crate::settings::schema::DEFAULT_OBFUSCATE_LOGS);

thread_local! {
    /// The mode forced for this thread by [`LogObscurer::with_mode`] (`None` = the global setting).
    static FORCED_MODE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Puts the previous forced mode back, also when the formatting panics.
struct RestoreMode(Option<bool>);

impl Drop for RestoreMode {
    fn drop(&mut self) {
        FORCED_MODE.with(|mode| mode.set(self.0));
    }
}

/// High-performance, thread-safe log sanitization and privacy protection subsystem.
/// Masks file names, folder paths, tag labels, and search queries with database IDs or surrogate identifiers when enabled.
pub struct LogObscurer;

impl LogObscurer {
    /// Returns true if log obfuscation / privacy masking is active for the current thread: the mode forced by
    /// [`Self::with_mode`] while it runs, else the global setting.
    #[inline]
    pub fn is_active() -> bool {
        FORCED_MODE
            .with(Cell::get)
            .unwrap_or_else(|| OBFUSCATE_LOGS.load(Ordering::Acquire))
    }

    /// Whether the setting `debug.obfuscate_logs` is on (ignores a mode forced for this thread).
    pub fn is_enabled() -> bool {
        OBFUSCATE_LOGS.load(Ordering::Acquire)
    }

    /// Sets whether log obfuscation is globally enabled.
    pub fn set_obfuscate(enabled: bool) {
        OBFUSCATE_LOGS.store(enabled, Ordering::Release);
    }

    /// Runs `render` with obfuscation forced on or off for this thread (the capture layer formats every event once in
    /// each mode). Nested calls restore the outer mode.
    pub fn with_mode<R>(obfuscate: bool, render: impl FnOnce() -> R) -> R {
        let previous = FORCED_MODE.with(|mode| mode.replace(Some(obfuscate)));
        let _restore = RestoreMode(previous);
        render()
    }

    /// Obfuscates or formats a filesystem path.
    pub fn path(path: impl AsRef<Path>) -> Obscured {
        Obscured::path(path.as_ref().to_path_buf())
    }

    /// Obfuscates or formats an optional filesystem path ("None" without one).
    pub fn path_opt(path_opt: Option<impl AsRef<Path>>) -> Obscured {
        Obscured::path_opt(path_opt.map(|p| p.as_ref().to_path_buf()))
    }

    /// Obfuscates or formats a directory / folder record reference.
    pub fn folder(id: impl Into<Option<u64>>, path: impl AsRef<Path>) -> Obscured {
        Obscured::folder(id.into(), path.as_ref().to_path_buf())
    }

    /// Obfuscates or formats a media file reference.
    pub fn media(id: impl Into<Option<u64>>, filename: &str) -> Obscured {
        Obscured::media(id.into(), filename.to_owned())
    }

    /// Obfuscates or formats a raw filename.
    pub fn filename(filename: &str) -> Obscured {
        Obscured::filename(filename.to_owned())
    }

    /// Obfuscates or formats a tag record reference.
    pub fn tag(id: impl Into<Option<u32>>, name: &str) -> Obscured {
        Obscured::tag(id.into(), name.to_owned())
    }

    /// Formats a known tag ID.
    pub fn tag_id(id: u32) -> String {
        format!("Tag#{id}")
    }

    /// Obfuscates or formats a search/filter query string.
    pub fn query(query: &str) -> Obscured {
        Obscured::query(query.to_owned())
    }

    /// Text written by another program (mpv's messages) that may contain the path of the file it plays: the path and its
    /// file name are hidden while obfuscation is active.
    pub fn foreign_text(text: &str, known_path: Option<&Path>) -> Obscured {
        Obscured::foreign_text(text.to_owned(), known_path.map(Path::to_path_buf))
    }

    /// Unified media target descriptor used across image & video thumbnailing pipelines. The size is read once, here.
    pub fn target(path: &Path, media_id: Option<u64>) -> Obscured {
        let size_hint = std::fs::metadata(path)
            .map(|m| format!(", {:.2} KB", m.len().az::<f64>() / 1024.0))
            .unwrap_or_default();
        Obscured::target(path.to_path_buf(), media_id, size_hint)
    }
}

#[cfg(test)]
mod tests {
    use super::LogObscurer;
    use std::path::Path;

    #[test]
    fn the_mode_is_decided_when_the_value_is_formatted() {
        let path = LogObscurer::path(Path::new("/media/Holiday/beach.MP4"));
        let real = LogObscurer::with_mode(false, || path.to_string());
        let private = LogObscurer::with_mode(true, || path.to_string());
        assert_eq!(real, "/media/Holiday/beach.MP4");
        assert!(private.starts_with("[Path:") && private.ends_with(".mp4]"), "{private}");
        assert!(!private.contains("Holiday"));
    }

    #[test]
    fn with_mode_restores_the_outer_mode() {
        LogObscurer::with_mode(true, || {
            assert!(LogObscurer::is_active());
            LogObscurer::with_mode(false, || assert!(!LogObscurer::is_active()));
            assert!(LogObscurer::is_active());
        });
    }

    #[test]
    fn every_kind_hides_its_name_and_keeps_ids() {
        let hidden = |value: &dyn std::fmt::Display| LogObscurer::with_mode(true, || value.to_string());
        let shown = |value: &dyn std::fmt::Display| LogObscurer::with_mode(false, || value.to_string());

        assert_eq!(hidden(&LogObscurer::folder(7, "/a/Secret")), "Folder#7");
        assert_eq!(shown(&LogObscurer::folder(7, "/a/Secret")), "Folder#7(/a/Secret)");
        assert_eq!(hidden(&LogObscurer::media(9, "Secret.JPG")), "Media#9[*.jpg]");
        assert_eq!(shown(&LogObscurer::media(None, "a.jpg")), "'a.jpg'");
        assert!(hidden(&LogObscurer::filename("Secret.png")).starts_with("[File:"));
        assert_eq!(hidden(&LogObscurer::tag(3, "Private")), "Tag#3");
        assert_eq!(hidden(&LogObscurer::tag(None, "Private")), "[Tag:masked]");
        assert_eq!(shown(&LogObscurer::tag(3, "Private")), "Tag#3(Private)");
        assert_eq!(hidden(&LogObscurer::query("beach")), "[Query:5 chars]");
        assert_eq!(shown(&LogObscurer::query("beach")), "\"beach\"");
        assert_eq!(hidden(&LogObscurer::path_opt(None::<&Path>)), "None");
    }

    #[test]
    fn debug_formatting_quotes_like_a_string() {
        let value = LogObscurer::query("x");
        let debug = LogObscurer::with_mode(false, || format!("{value:?}"));
        assert_eq!(debug, "\"\\\"x\\\"\"");
    }

    #[test]
    fn foreign_text_hides_the_known_path_and_its_file_name() {
        let path = Path::new("/media/Holiday/beach.mp4");
        let text = LogObscurer::foreign_text(
            "Cannot open file '/media/Holiday/beach.mp4': beach.mp4 is broken",
            Some(path),
        );
        let private = LogObscurer::with_mode(true, || text.to_string());
        assert!(!private.contains("Holiday") && !private.contains("beach"), "{private}");
        let real = LogObscurer::with_mode(false, || text.to_string());
        assert!(real.contains("/media/Holiday/beach.mp4"));
    }
}
