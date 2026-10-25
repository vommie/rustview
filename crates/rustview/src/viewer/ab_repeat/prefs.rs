//! The settings of the `ab_repeat.lua` support (Settings > Viewer > Videos), kept as a global like the other player preferences.

use parking_lot::RwLock;
use rustview_core::settings::schema::{
    AbRepeatSettings, DEFAULT_AB_REPEAT_CLICK_LOADS, DEFAULT_AB_REPEAT_DRAG_FLAGS, DEFAULT_AB_REPEAT_ENABLED,
    DEFAULT_AB_REPEAT_HOVER_NAME, DEFAULT_AB_REPEAT_HOVER_TIMES, DEFAULT_AB_REPEAT_OSD_MESSAGES,
    DEFAULT_AB_REPEAT_PAGE_SIZE,
};
use rustview_core::util::cast::SaturatingAs;

/// How saved repeats are colored in the playbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeColors {
    /// Each its own color (the default).
    Each,
    /// All grey; only the active A-B is yellow.
    Grey,
}

/// How the load menu shows many entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPaging {
    Pages,
    Scroll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbRepeatPrefs {
    /// Look for the script in the players.
    pub enabled: bool,
    /// The script shows its own on-screen messages.
    pub osd_messages: bool,
    pub colors: RangeColors,
    pub click_loads: bool,
    pub hover_name: bool,
    pub hover_times: bool,
    pub drag_flags: bool,
    pub paging: MenuPaging,
    /// Entries per page, or rows of the scroll list.
    pub page_size: usize,
}

impl AbRepeatPrefs {
    pub const DEFAULT: Self = Self {
        enabled: DEFAULT_AB_REPEAT_ENABLED,
        osd_messages: DEFAULT_AB_REPEAT_OSD_MESSAGES,
        colors: RangeColors::Each,
        click_loads: DEFAULT_AB_REPEAT_CLICK_LOADS,
        hover_name: DEFAULT_AB_REPEAT_HOVER_NAME,
        hover_times: DEFAULT_AB_REPEAT_HOVER_TIMES,
        drag_flags: DEFAULT_AB_REPEAT_DRAG_FLAGS,
        paging: MenuPaging::Pages,
        page_size: DEFAULT_AB_REPEAT_PAGE_SIZE as usize,
    };

    pub fn from_settings(settings: &AbRepeatSettings) -> Self {
        Self {
            enabled: settings.enabled,
            osd_messages: settings.osd_messages,
            colors: if settings.colors == "grey" {
                RangeColors::Grey
            } else {
                RangeColors::Each
            },
            click_loads: settings.click_loads,
            hover_name: settings.hover_name,
            hover_times: settings.hover_times,
            drag_flags: settings.drag_flags,
            paging: if settings.menu == "scroll" {
                MenuPaging::Scroll
            } else {
                MenuPaging::Pages
            },
            page_size: settings.page_size.saturating_as::<usize>().max(1),
        }
    }

    /// The `script-opts` entry that silences the script's messages in `RustView`'s players (`None`: its own settings apply).
    pub const fn script_opt(self) -> Option<&'static str> {
        if self.osd_messages {
            None
        } else {
            Some("ab_repeat-osd_messages=no")
        }
    }
}

static PREFS: RwLock<AbRepeatPrefs> = RwLock::new(AbRepeatPrefs::DEFAULT);

pub fn current() -> AbRepeatPrefs {
    *PREFS.read()
}

/// Makes `settings` the current preferences; returns the previous ones.
pub fn apply(settings: &AbRepeatSettings) -> AbRepeatPrefs {
    let next = AbRepeatPrefs::from_settings(settings);
    std::mem::replace(&mut *PREFS.write(), next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_map_to_prefs() {
        let defaults = AbRepeatPrefs::from_settings(&AbRepeatSettings::default());
        assert_eq!(defaults, AbRepeatPrefs::DEFAULT);
        assert_eq!(defaults.script_opt(), None);
        let settings = AbRepeatSettings {
            colors: "grey".into(),
            menu: "scroll".into(),
            osd_messages: false,
            page_size: 7,
            ..AbRepeatSettings::default()
        };
        let prefs = AbRepeatPrefs::from_settings(&settings);
        assert_eq!(prefs.colors, RangeColors::Grey);
        assert_eq!(prefs.paging, MenuPaging::Scroll);
        assert_eq!(prefs.page_size, 7);
        assert_eq!(prefs.script_opt(), Some("ab_repeat-osd_messages=no"));
    }
}
