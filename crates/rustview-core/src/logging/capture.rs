//! The `tracing` layer that keeps every log line in memory and, while switched on, in the log files: each event is formatted
//! twice, once with names shown and once with names hidden.

use super::files::{LogFiles, RotatingLog};
use super::ring::LogRing;
use crate::settings::LogObscurer;
use chrono::{DateTime, Local};
use parking_lot::RwLock;
use std::fmt::Write as _;
use std::io;
use std::path::Path;
use std::sync::Arc;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Where the captured lines go: the ring in memory and the log files while they are on.
pub struct LogSinks {
    ring: LogRing,
    files: RwLock<Option<LogFiles>>,
    mpv: RwLock<Option<RotatingLog>>,
}

impl LogSinks {
    pub fn new(capacity: usize) -> Self {
        Self {
            ring: LogRing::new(capacity),
            files: RwLock::new(None),
            mpv: RwLock::new(None),
        }
    }

    pub fn ring(&self) -> &LogRing {
        &self.ring
    }

    /// Keeps a line. The files are locked first, so a line is never lost or written twice while they are switched on.
    pub fn record(&self, time: DateTime<Local>, level: Level, real: String, private: String) {
        let files = self.files.read();
        let line = self.ring.push(time, level, real, private);
        if let Some(files) = files.as_ref() {
            files.real.write(&line.file_line(false));
            files.private.write(&line.file_line(true));
        }
    }

    /// Starts the two log files in `dir` (if they are not running yet); they begin with the lines already in memory.
    pub fn start_files(&self, dir: &Path) -> io::Result<()> {
        let mut files = self.files.write();
        if files.is_some() {
            return Ok(());
        }
        let started = LogFiles::start(dir)?;
        for line in self.ring.since(0) {
            started.real.write(&line.file_line(false));
            started.private.write(&line.file_line(true));
        }
        *files = Some(started);
        Ok(())
    }

    /// Stops the log files (what is queued is still written).
    pub fn stop_files(&self) {
        self.files.write().take();
    }

    pub fn files_running(&self) -> bool {
        self.files.read().is_some()
    }

    /// Starts `mpv.log` in `dir` (if it is not running yet).
    pub fn start_mpv_log(&self, dir: &Path) -> io::Result<()> {
        let mut mpv = self.mpv.write();
        if mpv.is_none() {
            *mpv = Some(RotatingLog::start(dir, super::files::MPV_LOG_FILE)?);
        }
        Ok(())
    }

    pub fn stop_mpv_log(&self) {
        self.mpv.write().take();
    }

    pub fn mpv_log_running(&self) -> bool {
        self.mpv.read().is_some()
    }

    /// Appends one of libmpv's messages to `mpv.log` (nothing while it is off).
    pub fn write_mpv(&self, line: &str) {
        if let Some(log) = self.mpv.read().as_ref() {
            log.write(line);
        }
    }
}

/// Collects the message and the fields of an event the way the terminal output shows them: `message key=value ...`.
#[derive(Default)]
struct LineVisitor {
    message: String,
    fields: String,
}

impl Visit for LineVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

fn render(event: &Event<'_>) -> String {
    let mut visitor = LineVisitor::default();
    event.record(&mut visitor);
    visitor.message.push_str(&visitor.fields);
    visitor.message
}

/// The layer installed by `logging::init_app`.
pub struct CaptureLayer {
    sinks: Arc<LogSinks>,
}

impl CaptureLayer {
    pub fn new(sinks: Arc<LogSinks>) -> Self {
        Self { sinks }
    }
}

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let real = LogObscurer::with_mode(false, || render(event));
        let private = LogObscurer::with_mode(true, || render(event));
        self.sinks
            .record(Local::now(), *event.metadata().level(), real, private);
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureLayer, LogSinks};
    use crate::settings::LogObscurer;
    use std::path::Path;
    use std::sync::Arc;
    use tracing::Level;
    use tracing_subscriber::layer::SubscriberExt;

    fn capture(log: impl FnOnce()) -> Arc<LogSinks> {
        let sinks = Arc::new(LogSinks::new(100));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer::new(sinks.clone()));
        tracing::subscriber::with_default(subscriber, log);
        sinks
    }

    #[test]
    fn every_line_is_kept_with_names_shown_and_hidden() {
        let sinks = capture(|| {
            tracing::warn!(
                file = %LogObscurer::path(Path::new("/media/Secret/a.jpg")),
                count = 3,
                "[Test] Opened {}",
                LogObscurer::folder(5, "/media/Secret")
            );
        });
        let lines = sinks.ring().since(0);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.level, Level::WARN);
        assert_eq!(
            line.real,
            "[Test] Opened Folder#5(/media/Secret) file=/media/Secret/a.jpg count=3"
        );
        assert!(
            line.private.starts_with("[Test] Opened Folder#5 file=[Path:"),
            "{}",
            line.private
        );
        assert!(!line.private.contains("Secret"));
    }

    #[test]
    fn string_fields_are_quoted_like_in_the_terminal() {
        let sinks = capture(|| tracing::info!(name = "x y", "plain"));
        assert_eq!(sinks.ring().since(0)[0].real, "plain name=\"x y\"");
    }

    #[test]
    fn switched_on_files_begin_with_the_lines_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let sinks = capture(|| tracing::info!(path = %LogObscurer::path(Path::new("/m/Secret.png")), "before"));
        sinks.start_files(dir.path()).unwrap();
        assert!(sinks.files_running());
        let subscriber = tracing_subscriber::registry().with(CaptureLayer::new(sinks.clone()));
        tracing::subscriber::with_default(subscriber, || tracing::error!("after"));
        sinks.stop_files();
        let real = std::fs::read_to_string(dir.path().join("rustview.log")).unwrap();
        let private = std::fs::read_to_string(dir.path().join("rustview-private.log")).unwrap();
        assert!(
            real.contains("before path=/m/Secret.png") && real.contains("ERROR after"),
            "{real}"
        );
        assert!(
            private.contains("before path=[Path:") && !private.contains("Secret"),
            "{private}"
        );
    }

    #[test]
    fn the_mpv_log_is_written_only_while_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let sinks = LogSinks::new(4);
        sinks.write_mpv("lost\n");
        sinks.start_mpv_log(dir.path()).unwrap();
        sinks.write_mpv("kept\n");
        sinks.stop_mpv_log();
        assert_eq!(std::fs::read_to_string(dir.path().join("mpv.log")).unwrap(), "kept\n");
    }
}
