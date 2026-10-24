//! The user's Multiview preferences (Settings > Fullscreen) as a snapshot the whole Multiview can read cheaply.
//!
//! The settings dialog is the only writer (`apply`, at startup and after every change); the instances read the snapshot
//! when they need a decision (which buttons a tile offers, how new videos start), so nothing has to be threaded through
//! the many places that create or host an instance and a change takes effect on the next event.

use super::audio::{AudioPolicy, SoundFollows};
use super::media_actions::MediaActionId;
use super::primary_actions::PrimaryActionId;
use parking_lot::RwLock;
use rustview_core::settings::schema::{
    DEFAULT_FULLSCREEN_HIDE_IDLE_CURSOR, DEFAULT_FULLSCREEN_IDLE_CURSOR_MS, DEFAULT_FULLSCREEN_MEDIA_BAR_ENABLED,
    DEFAULT_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_CROP,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DELETE, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DUPLICATE,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_MUTE, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_AS_TAB,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_WITH, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_REMOVE,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_STICKY, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_ZOOM,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_ENABLED, DEFAULT_FULLSCREEN_PRIMARY_BAR_REVEAL_DISTANCE_PX,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_CLOSE, DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_HIDE_CURSOR,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_MUTE_ALL, DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PIN,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PLAY_PAUSE_ALL, DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_RESET_LAYOUT,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_SEND_TO_TAB, DEFAULT_FULLSCREEN_RATING_SHOW_FEEDBACK, FullscreenSettings,
    MULTIVIEW_FOCUS_BRIGHTEN_PERCENT, MULTIVIEW_FOCUS_CORNER_LOGICAL_PX,
};

/// Which buttons the hover bar may show and how close the pointer has to come before it appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaBarPrefs {
    pub enabled: bool,
    pub mute: bool,
    pub sticky: bool,
    pub duplicate: bool,
    pub open_as_tab: bool,
    pub open_with: bool,
    pub delete: bool,
    /// Remove and Close are one button that changes its role with the number of media.
    pub remove: bool,
    pub zoom: bool,
    /// The crop button; Save, Undo and Redo appear by themselves while a picture has edits.
    pub crop: bool,
    /// Logical pixels around the place of the bar in which the pointer reveals it.
    pub reveal_distance_px: u32,
}

impl MediaBarPrefs {
    pub const DEFAULT: Self = Self {
        enabled: DEFAULT_FULLSCREEN_MEDIA_BAR_ENABLED,
        mute: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_MUTE,
        sticky: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_STICKY,
        duplicate: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DUPLICATE,
        open_as_tab: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_AS_TAB,
        open_with: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_WITH,
        delete: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DELETE,
        remove: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_REMOVE,
        zoom: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_ZOOM,
        crop: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_CROP,
        reveal_distance_px: DEFAULT_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
    };

    /// True if the user wants the button of `id` in the bar.
    pub fn allows(&self, id: MediaActionId) -> bool {
        self.enabled
            && match id {
                MediaActionId::Mute => self.mute,
                MediaActionId::Sticky => self.sticky,
                MediaActionId::Duplicate => self.duplicate,
                MediaActionId::OpenAsTab => self.open_as_tab,
                MediaActionId::OpenWith => self.open_with,
                MediaActionId::Delete => self.delete,
                MediaActionId::Remove | MediaActionId::Close => self.remove,
                MediaActionId::Zoom => self.zoom,
                MediaActionId::Crop => self.crop,
                // The edit buttons exist only while a picture has edits, and saving must always be possible then.
                MediaActionId::Save | MediaActionId::SaveMenu | MediaActionId::Undo | MediaActionId::Redo => true,
            }
    }
}

/// Which buttons the primary bar (view-wide buttons, top-left) may show, how close the pointer has to come, and when the
/// idle mouse cursor is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimaryBarPrefs {
    pub enabled: bool,
    pub send_to_tab: bool,
    pub close: bool,
    pub mute_all: bool,
    pub play_pause_all: bool,
    pub reset_layout: bool,
    pub hide_cursor: bool,
    pub pin: bool,
    pub reveal_distance_px: u32,
    pub hide_idle_cursor: bool,
    pub idle_cursor_ms: u32,
}

impl PrimaryBarPrefs {
    pub const DEFAULT: Self = Self {
        enabled: DEFAULT_FULLSCREEN_PRIMARY_BAR_ENABLED,
        send_to_tab: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_SEND_TO_TAB,
        close: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_CLOSE,
        mute_all: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_MUTE_ALL,
        play_pause_all: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PLAY_PAUSE_ALL,
        reset_layout: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_RESET_LAYOUT,
        hide_cursor: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_HIDE_CURSOR,
        pin: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PIN,
        reveal_distance_px: DEFAULT_FULLSCREEN_PRIMARY_BAR_REVEAL_DISTANCE_PX,
        hide_idle_cursor: DEFAULT_FULLSCREEN_HIDE_IDLE_CURSOR,
        idle_cursor_ms: DEFAULT_FULLSCREEN_IDLE_CURSOR_MS,
    };

    /// True if the user wants the button of `id` in the bar.
    pub fn allows(&self, id: PrimaryActionId) -> bool {
        self.enabled
            && match id {
                PrimaryActionId::SendToTab => self.send_to_tab,
                PrimaryActionId::Close => self.close,
                PrimaryActionId::MuteAll => self.mute_all,
                PrimaryActionId::PlayPauseAll => self.play_pause_all,
                PrimaryActionId::ResetLayout => self.reset_layout,
                PrimaryActionId::HideIdleCursor => self.hide_cursor,
                PrimaryActionId::Pin => self.pin,
            }
    }
}

/// How the selected medium of a Multiview of 2+ tiles is marked (`focus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusMarkPrefs {
    /// White over the column of the selected tile, in percent (0 = off).
    pub brighten_percent: u8,
    /// Length of the corner marks in logical pixels.
    pub corner_logical_px: u32,
}

impl FocusMarkPrefs {
    pub const DEFAULT: Self = Self {
        brighten_percent: MULTIVIEW_FOCUS_BRIGHTEN_PERCENT,
        corner_logical_px: MULTIVIEW_FOCUS_CORNER_LOGICAL_PX,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiviewPrefs {
    pub media_bar: MediaBarPrefs,
    pub primary: PrimaryBarPrefs,
    pub audio: AudioPolicy,
    /// A rating given with the keyboard shows its icon on the tiles for a moment.
    pub rating_feedback: bool,
    pub focus_marks: FocusMarkPrefs,
}

impl MultiviewPrefs {
    pub const DEFAULT: Self = Self {
        media_bar: MediaBarPrefs::DEFAULT,
        primary: PrimaryBarPrefs::DEFAULT,
        audio: AudioPolicy::DEFAULT,
        rating_feedback: DEFAULT_FULLSCREEN_RATING_SHOW_FEEDBACK,
        focus_marks: FocusMarkPrefs::DEFAULT,
    };

    pub fn from_settings(settings: &FullscreenSettings) -> Self {
        let bar = &settings.media_bar;
        let primary = &settings.primary_bar;
        let sound = &settings.sound;
        Self {
            primary: PrimaryBarPrefs {
                enabled: primary.enabled,
                send_to_tab: primary.show_send_to_tab,
                close: primary.show_close,
                mute_all: primary.show_mute_all,
                play_pause_all: primary.show_play_pause_all,
                reset_layout: primary.show_reset_layout,
                hide_cursor: primary.show_hide_cursor,
                pin: primary.show_pin,
                reveal_distance_px: primary.reveal_distance_px,
                hide_idle_cursor: primary.hide_idle_cursor,
                idle_cursor_ms: primary.idle_cursor_ms,
            },
            media_bar: MediaBarPrefs {
                enabled: bar.enabled,
                mute: bar.show_mute,
                sticky: bar.show_sticky,
                duplicate: bar.show_duplicate,
                open_as_tab: bar.show_open_as_tab,
                open_with: bar.show_open_with,
                delete: bar.show_delete,
                remove: bar.show_remove,
                zoom: bar.show_zoom,
                crop: bar.show_crop,
                reveal_distance_px: bar.reveal_distance_px,
            },
            audio: AudioPolicy {
                auto_control: sound.auto_control,
                follows: SoundFollows::from_setting(&sound.follows),
                hand_over: sound.hand_over,
            },
            rating_feedback: settings.rating.show_feedback,
            focus_marks: FocusMarkPrefs::DEFAULT,
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
            media_bar = ?next.media_bar,
            primary_bar = ?next.primary,
            audio = ?next.audio,
            "[Multiview: Prefs] Preferences applied"
        );
        *current = next;
    }
}
