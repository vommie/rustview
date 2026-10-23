//! The user's type-ahead options (Settings > Appearance & Grid > Type-Ahead Find) as a small `Copy` value that every key press reads
//! without cloning the whole settings.

use parking_lot::RwLock;
use rustview_core::settings::schema::{
    AppearanceSettings, DEFAULT_TYPE_AHEAD_CENTER_TARGET, DEFAULT_TYPE_AHEAD_CYCLE_REPEATED_KEYS,
    DEFAULT_TYPE_AHEAD_ENABLED, DEFAULT_TYPE_AHEAD_INCLUDE_FOLDERS, DEFAULT_TYPE_AHEAD_MULTIVIEW_DELAY_MS,
    DEFAULT_TYPE_AHEAD_NEW_SEARCH_GAP_MS, DEFAULT_TYPE_AHEAD_NO_MATCH_MESSAGE, DEFAULT_TYPE_AHEAD_TIMEOUT_MS,
};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeAheadConfig {
    pub enabled: bool,
    /// Keys typed within this time of the previous one belong to the same search string.
    pub timeout: Duration,
    /// A key that continues no name starts a new search after this pause; `None` = it is always a typo.
    pub new_search_gap: Option<Duration>,
    pub include_folders: bool,
    pub cycle_repeated_keys: bool,
    pub center_target: bool,
    pub show_no_match: bool,
    /// How long an open Multiview waits after an item selected by typing; zero = at once.
    pub multiview_delay: Duration,
}

impl TypeAheadConfig {
    pub const DEFAULT: Self = Self {
        enabled: DEFAULT_TYPE_AHEAD_ENABLED,
        timeout: Duration::from_millis(DEFAULT_TYPE_AHEAD_TIMEOUT_MS as u64),
        new_search_gap: gap_of(DEFAULT_TYPE_AHEAD_NEW_SEARCH_GAP_MS),
        include_folders: DEFAULT_TYPE_AHEAD_INCLUDE_FOLDERS,
        cycle_repeated_keys: DEFAULT_TYPE_AHEAD_CYCLE_REPEATED_KEYS,
        center_target: DEFAULT_TYPE_AHEAD_CENTER_TARGET,
        show_no_match: DEFAULT_TYPE_AHEAD_NO_MATCH_MESSAGE,
        multiview_delay: Duration::from_millis(DEFAULT_TYPE_AHEAD_MULTIVIEW_DELAY_MS as u64),
    };

    pub fn from_settings(appearance: &AppearanceSettings) -> Self {
        Self {
            enabled: appearance.type_ahead_enabled,
            timeout: Duration::from_millis(u64::from(appearance.type_ahead_timeout_ms)),
            new_search_gap: gap_of(appearance.type_ahead_new_search_gap_ms),
            include_folders: appearance.type_ahead_include_folders,
            cycle_repeated_keys: appearance.type_ahead_cycle_repeated_keys,
            center_target: appearance.type_ahead_center_target,
            show_no_match: appearance.type_ahead_show_no_match,
            multiview_delay: Duration::from_millis(u64::from(appearance.type_ahead_multiview_delay_ms)),
        }
    }
}

impl Default for TypeAheadConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

const fn gap_of(ms: u32) -> Option<Duration> {
    if ms == 0 {
        None
    } else {
        Some(Duration::from_millis(ms as u64))
    }
}

static CURRENT: RwLock<TypeAheadConfig> = RwLock::new(TypeAheadConfig::DEFAULT);

/// Takes over the options (at startup and whenever they change).
pub fn apply(appearance: &AppearanceSettings) {
    *CURRENT.write() = TypeAheadConfig::from_settings(appearance);
}

pub fn current() -> TypeAheadConfig {
    *CURRENT.read()
}
