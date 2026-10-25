//! The log of `RustView` (`docs/debug_page.md`): the terminal output as before, plus every line in memory (for the log window and
//! the diagnostic report) and, while switched on, in two log files: a real one and a private one with names hidden.
//! One reloadable filter decides the level for all of them; `RUST_LOG` replaces it when it is set.

mod capture;
mod files;
mod level;
mod ring;

pub use capture::{CaptureLayer, LogSinks};
pub use files::{KEPT_FILES, MPV_LOG_FILE, PRIVATE_LOG_FILE, REAL_LOG_FILE};
pub use level::LogLevel;
pub use ring::{LogLine, LogRing};

use parking_lot::Mutex;
use std::io;
use std::path::Path;
use std::sync::{Arc, LazyLock, OnceLock};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, reload};

/// How many lines the memory keeps (both renderings of each, about 2 MB at most).
pub const RING_CAPACITY: usize = 5_000;
/// The environment variable that replaces the level chosen in the settings.
pub const LEVEL_VARIABLE: &str = "RUST_LOG";

static SINKS: LazyLock<Arc<LogSinks>> = LazyLock::new(|| Arc::new(LogSinks::new(RING_CAPACITY)));
static CONTROL: OnceLock<Control> = OnceLock::new();

struct Control {
    filter: reload::Handle<EnvFilter, Registry>,
    env_override: Option<String>,
    level: Mutex<LogLevel>,
}

/// The sinks of this process (the ring and the files).
pub fn sinks() -> &'static Arc<LogSinks> {
    &SINKS
}

/// The lines in memory.
pub fn ring() -> &'static LogRing {
    SINKS.ring()
}

/// `RUST_LOG` when it is set and not empty.
fn env_level() -> Option<String> {
    std::env::var(LEVEL_VARIABLE)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Installs the log of the application: the terminal output (as before: no target, thread names), the capture layer and
/// the filter. Call once, first thing in `main`.
pub fn init_app(level: LogLevel) -> Result<(), tracing_subscriber::util::TryInitError> {
    let env_override = env_level();
    let filter = env_override
        .as_deref()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| EnvFilter::new(level.filter_directive()));
    let (filter, handle) = reload::Layer::new(filter);
    let terminal = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_thread_names(true);
    tracing_subscriber::registry()
        .with(filter)
        .with(terminal)
        .with(CaptureLayer::new(SINKS.clone()))
        .try_init()?;
    let _ = CONTROL.set(Control {
        filter: handle,
        env_override,
        level: Mutex::new(level),
    });
    Ok(())
}

/// The value of `RUST_LOG` when it decides the level (the settings' level is then not used).
pub fn env_override() -> Option<&'static str> {
    CONTROL.get().and_then(|control| control.env_override.as_deref())
}

/// The level chosen in the settings (also while `RUST_LOG` decides).
pub fn level() -> LogLevel {
    CONTROL
        .get()
        .map_or_else(LogLevel::default, |control| *control.level.lock())
}

/// Applies the level of the settings at once, without a restart. Returns `false` when `RUST_LOG` decides the level.
pub fn set_level(level: LogLevel) -> bool {
    let Some(control) = CONTROL.get() else {
        return false;
    };
    let previous = std::mem::replace(&mut *control.level.lock(), level);
    if control.env_override.is_some() {
        return false;
    }
    if previous != level {
        let applied = control
            .filter
            .modify(|filter| *filter = EnvFilter::new(level.filter_directive()));
        if let Err(error) = applied {
            tracing::warn!(%error, "[Logging] The log level could not be changed");
            return false;
        }
        tracing::info!(level = level.as_str(), "[Logging] Log level changed");
    }
    true
}

/// Switches the two log files in `dir` on or off (`None` = off).
pub fn set_files(dir: Option<&Path>) -> io::Result<()> {
    match dir {
        Some(dir) => {
            let was_running = SINKS.files_running();
            SINKS.start_files(dir)?;
            if !was_running {
                tracing::info!(folder = %dir.display(), "[Logging] Log files started");
            }
        }
        None => {
            if SINKS.files_running() {
                tracing::info!("[Logging] Log files stopped");
                SINKS.stop_files();
            }
        }
    }
    Ok(())
}

/// Switches `mpv.log` in `dir` on or off (`None` = off).
pub fn set_mpv_log(dir: Option<&Path>) -> io::Result<()> {
    if let Some(dir) = dir {
        return SINKS.start_mpv_log(dir);
    }
    SINKS.stop_mpv_log();
    Ok(())
}

/// Whether libmpv's detailed messages are wanted (players ask libmpv for them while this is on).
pub fn mpv_log_running() -> bool {
    SINKS.mpv_log_running()
}

/// Appends one of libmpv's messages to `mpv.log` (nothing while it is off).
pub fn write_mpv(line: &str) {
    SINKS.write_mpv(line);
}
