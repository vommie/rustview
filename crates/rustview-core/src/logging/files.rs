//! Log files: written only while the user has switched them on (`debug.log_files`, `debug.mpv_log`).
//!
//! Each start begins a new file; the previous ones are kept as `name.1`, `name.2`, ... (`file-rotate`), the newest
//! [`KEPT_FILES`] in all. Lines go through `tracing-appender`'s non-blocking writer, so no thread that logs ever waits for the
//! drive.

use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

/// How many files of each log are kept: the current one and the ones of the previous starts.
pub const KEPT_FILES: usize = 5;
/// The log with every name shown (for oneself).
pub const REAL_LOG_FILE: &str = "rustview.log";
/// The same log with file names, paths, folders, tags and searches hidden (to share).
pub const PRIVATE_LOG_FILE: &str = "rustview-private.log";
/// libmpv's own detailed messages.
pub const MPV_LOG_FILE: &str = "mpv.log";

/// One log file that is written in the background.
pub struct RotatingLog {
    writer: NonBlocking,
    // Dropping the guard writes what is still queued and ends the writer thread.
    _guard: WorkerGuard,
}

impl RotatingLog {
    /// Starts a new `dir/name`: an existing file with content becomes `name.1` (and so on), the oldest beyond
    /// [`KEPT_FILES`] are deleted.
    pub fn start(dir: &Path, name: &str) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(name);
        let mut file = FileRotate::new(
            &path,
            AppendCount::new(KEPT_FILES - 1),
            ContentLimit::None,
            Compression::None,
            None,
        );
        if fs::metadata(&path).is_ok_and(|meta| meta.len() > 0) {
            file.rotate()?;
        }
        // `FileRotate` reports a file it could not open only when it is written; find out now.
        fs::OpenOptions::new().append(true).open(&path)?;
        let (writer, guard) = tracing_appender::non_blocking(file);
        Ok(Self { writer, _guard: guard })
    }

    /// Queues a line (with its line break) for the file.
    pub fn write(&self, line: &str) {
        let mut writer = self.writer.clone();
        let _ = writer.write_all(line.as_bytes());
    }
}

/// The pair of log files of `RustView` itself.
pub struct LogFiles {
    pub real: RotatingLog,
    pub private: RotatingLog,
}

impl LogFiles {
    pub fn start(dir: &Path) -> io::Result<Self> {
        Ok(Self {
            real: RotatingLog::start(dir, REAL_LOG_FILE)?,
            private: RotatingLog::start(dir, PRIVATE_LOG_FILE)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{KEPT_FILES, LogFiles, RotatingLog};
    use std::fs;

    #[test]
    fn every_start_begins_a_new_file_and_only_the_newest_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for start in 0..KEPT_FILES + 2 {
            let log = RotatingLog::start(dir.path(), "x.log").unwrap();
            log.write(&format!("start {start}\n"));
        }
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["x.log", "x.log.1", "x.log.2", "x.log.3", "x.log.4"]);
        assert_eq!(
            fs::read_to_string(dir.path().join("x.log")).unwrap(),
            format!("start {}\n", KEPT_FILES + 1)
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("x.log.1")).unwrap(),
            format!("start {KEPT_FILES}\n")
        );
    }

    #[test]
    fn an_empty_file_is_not_rotated() {
        let dir = tempfile::tempdir().unwrap();
        drop(RotatingLog::start(dir.path(), "x.log").unwrap());
        drop(RotatingLog::start(dir.path(), "x.log").unwrap());
        assert!(!dir.path().join("x.log.1").exists());
    }

    #[test]
    fn the_pair_is_made_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let files = LogFiles::start(&dir.path().join("logs")).unwrap();
        files.real.write("r\n");
        files.private.write("p\n");
        drop(files);
        assert_eq!(fs::read_to_string(dir.path().join("logs/rustview.log")).unwrap(), "r\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("logs/rustview-private.log")).unwrap(),
            "p\n"
        );
    }
}
