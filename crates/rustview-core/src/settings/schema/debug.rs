//! Settings > Debug (`docs/debug_page.md`): the log options, and the remembered state of the page and of the log window.

use super::{
    DEFAULT_DEBUG_OPEN_CARDS, DEFAULT_LOG_FILES, DEFAULT_LOG_LEVEL, DEFAULT_LOG_WINDOW_HEIGHT,
    DEFAULT_LOG_WINDOW_WIDTH, DEFAULT_MPV_LOG, DEFAULT_OBFUSCATE_LOGS,
};
use serde::{Deserialize, Serialize};

/// The options of the Debug page. `log_level`, `log_files` and `mpv_log` were added in 0.4.15; older files get the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugSettings {
    /// Hide file names, paths, folders, tags and searches in the terminal output and in the diagnostic report.
    pub obfuscate_logs: bool,
    /// `error`, `warn`, `info`, `debug` or `trace` (`RUST_LOG` replaces it when set).
    pub log_level: String,
    /// Write `rustview.log` and `rustview-private.log` in the state folder.
    pub log_files: bool,
    /// Write libmpv's detailed messages to `mpv.log` next to them.
    pub mpv_log: bool,
}

impl DebugSettings {
    /// An unknown level becomes the default (a hand-edited file never breaks loading).
    pub fn sanitize(&mut self) {
        crate::logging::LogLevel::from_setting(&self.log_level)
            .as_str()
            .clone_into(&mut self.log_level);
    }
}

impl Default for DebugSettings {
    fn default() -> Self {
        Self {
            obfuscate_logs: DEFAULT_OBFUSCATE_LOGS,
            log_level: DEFAULT_LOG_LEVEL.to_owned(),
            log_files: DEFAULT_LOG_FILES,
            mpv_log: DEFAULT_MPV_LOG,
        }
    }
}

/// What the Debug page shows open (added in 0.4.15, kept at once without waiting for Save).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugPageState {
    /// Ids of the open cards ("dependencies", "health", "log", "report", "tools", "startup", "runtime", "privacy").
    pub open_cards: Vec<String>,
    /// Ids of the cards whose passed checks are listed (otherwise they are one row).
    pub passed_shown: Vec<String>,
    /// The group "Developer" of the Dependencies card.
    pub developer_open: bool,
}

impl Default for DebugPageState {
    fn default() -> Self {
        Self {
            open_cards: DEFAULT_DEBUG_OPEN_CARDS.iter().map(|id| (*id).to_owned()).collect(),
            passed_shown: Vec::new(),
            developer_open: false,
        }
    }
}

/// The log window (added in 0.4.15): its place, size and what it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogWindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    /// `all`, `warnings` or `errors`.
    pub filter: String,
    /// Show the lines as the private log has them (names hidden).
    pub private_view: bool,
    /// Scroll to every new line.
    pub follow: bool,
}

impl LogWindowState {
    /// The smallest and largest size kept (logical pixels).
    pub const MIN_SIZE: u32 = 360;
    pub const MAX_SIZE: u32 = 16_384;

    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(Self::MIN_SIZE, Self::MAX_SIZE);
        self.height = self.height.clamp(Self::MIN_SIZE, Self::MAX_SIZE);
        if !["all", "warnings", "errors"].contains(&self.filter.as_str()) {
            "all".clone_into(&mut self.filter);
        }
    }
}

impl Default for LogWindowState {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: DEFAULT_LOG_WINDOW_WIDTH,
            height: DEFAULT_LOG_WINDOW_HEIGHT,
            filter: "all".to_owned(),
            private_view: false,
            follow: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DebugPageState, DebugSettings, LogWindowState};

    #[test]
    fn files_of_older_versions_get_the_defaults() {
        let debug: DebugSettings = serde_json::from_str(r#"{"obfuscate_logs": false}"#).unwrap();
        assert!(!debug.obfuscate_logs);
        assert_eq!(debug.log_level, "info");
        assert!(!debug.log_files && !debug.mpv_log);
        let page: DebugPageState = serde_json::from_str("{}").unwrap();
        assert_eq!(page.open_cards, ["dependencies", "health"]);
        let window: LogWindowState = serde_json::from_str(r#"{"width": 700}"#).unwrap();
        assert_eq!((window.width, window.height, window.follow), (700, 560, true));
    }

    #[test]
    fn hand_edited_values_are_repaired() {
        let mut debug = DebugSettings {
            log_level: "LOUD".into(),
            ..DebugSettings::default()
        };
        debug.sanitize();
        assert_eq!(debug.log_level, "info");
        debug.log_level = "Debug".into();
        debug.sanitize();
        assert_eq!(debug.log_level, "debug");
        let mut window = LogWindowState {
            width: 5,
            filter: "loud".into(),
            ..LogWindowState::default()
        };
        window.sanitize();
        assert_eq!(
            (window.width, window.filter.as_str()),
            (LogWindowState::MIN_SIZE, "all")
        );
    }
}
