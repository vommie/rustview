//! What the Settings page Hotkeys keeps between sessions (the keys themselves are `AppSettings::hotkeys`).

use serde::{Deserialize, Serialize};

/// The filter, the "Only changed" switch and the collapsed sections of the Hotkeys page (added in 0.4.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeysPageState {
    pub filter: String,
    pub only_changed: bool,
    /// Ids of the collapsed sections ("browse", "tab", ...).
    pub collapsed_areas: Vec<String>,
}
