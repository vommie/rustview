//! The user's Multiview preferences (Settings > Fullscreen) as a snapshot the whole Multiview can read cheaply.
//!
//! The settings dialog is the only writer (`apply`, at startup and after every change); the instances read the snapshot
//! when they need a decision (which buttons a tile offers, how new videos start), so nothing has to be threaded through
//! the many places that create or host an instance and a change takes effect on the next event.

use super::actions::ActionId;
use super::audio::{AudioPolicy, SoundFollows};
use crate::settings::schema::{
    DEFAULT_FULLSCREEN_BAR_ENABLED, DEFAULT_FULLSCREEN_BAR_REVEAL_DISTANCE_PX,
    DEFAULT_FULLSCREEN_BAR_SHOW_MUTE, DEFAULT_FULLSCREEN_BAR_SHOW_OPEN_AS_TAB,
    DEFAULT_FULLSCREEN_BAR_SHOW_REMOVE, DEFAULT_FULLSCREEN_BAR_SHOW_STICKY, FullscreenSettings,
};
use parking_lot::RwLock;

/// Which buttons the hover bar may show and how close the pointer has to come before it appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarPrefs {
    pub enabled: bool,
    pub mute: bool,
    pub sticky: bool,
    pub open_as_tab: bool,
    /// Remove and Close are one button that changes its role with the number of media.
    pub remove: bool,
    /// Logical pixels around the place of the bar in which the pointer reveals it.
    pub reveal_distance_px: u32,
}

impl BarPrefs {
    pub const DEFAULT: Self = Self {
        enabled: DEFAULT_FULLSCREEN_BAR_ENABLED,
        mute: DEFAULT_FULLSCREEN_BAR_SHOW_MUTE,
        sticky: DEFAULT_FULLSCREEN_BAR_SHOW_STICKY,
        open_as_tab: DEFAULT_FULLSCREEN_BAR_SHOW_OPEN_AS_TAB,
        remove: DEFAULT_FULLSCREEN_BAR_SHOW_REMOVE,
        reveal_distance_px: DEFAULT_FULLSCREEN_BAR_REVEAL_DISTANCE_PX,
    };

    /// True if the user wants the button of `id` in the bar.
    pub fn allows(&self, id: ActionId) -> bool {
        self.enabled
            && match id {
                ActionId::Mute => self.mute,
                ActionId::Sticky => self.sticky,
                ActionId::OpenAsTab => self.open_as_tab,
                ActionId::Remove | ActionId::Close => self.remove,
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiviewPrefs {
    pub bar: BarPrefs,
    pub audio: AudioPolicy,
}

impl MultiviewPrefs {
    pub const DEFAULT: Self = Self {
        bar: BarPrefs::DEFAULT,
        audio: AudioPolicy::DEFAULT,
    };

    pub fn from_settings(settings: &FullscreenSettings) -> Self {
        let bar = &settings.action_bar;
        let sound = &settings.sound;
        Self {
            bar: BarPrefs {
                enabled: bar.enabled,
                mute: bar.show_mute,
                sticky: bar.show_sticky,
                open_as_tab: bar.show_open_as_tab,
                remove: bar.show_remove,
                reveal_distance_px: bar.reveal_distance_px,
            },
            audio: AudioPolicy {
                auto_control: sound.auto_control,
                follows: SoundFollows::from_setting(&sound.follows),
                hand_over: sound.hand_over,
            },
        }
    }
}

static PREFS: RwLock<MultiviewPrefs> = RwLock::new(MultiviewPrefs::DEFAULT);

pub fn current() -> MultiviewPrefs {
    *PREFS.read()
}

/// Makes the fullscreen settings the current preferences.
pub fn apply(settings: &FullscreenSettings) {
    let next = MultiviewPrefs::from_settings(settings);
    let mut current = PREFS.write();
    if *current != next {
        tracing::info!(
            bar = ?next.bar,
            audio = ?next.audio,
            "[Multiview: Prefs] Preferences applied"
        );
        *current = next;
    }
}
