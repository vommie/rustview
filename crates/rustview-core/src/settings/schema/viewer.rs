//! Viewer and fullscreen (Multiview) settings.

use super::{
    DEFAULT_AB_REPEAT_CLICK_LOADS, DEFAULT_AB_REPEAT_COLORS, DEFAULT_AB_REPEAT_DRAG_FLAGS, DEFAULT_AB_REPEAT_ENABLED,
    DEFAULT_AB_REPEAT_HOVER_NAME, DEFAULT_AB_REPEAT_HOVER_TIMES, DEFAULT_AB_REPEAT_MENU, DEFAULT_AB_REPEAT_PAGE_SIZE,
    MAX_AB_REPEAT_PAGE_SIZE, MIN_AB_REPEAT_PAGE_SIZE,
};
use super::{
    DEFAULT_FULLSCREEN_BACKGROUND_COLOR, DEFAULT_FULLSCREEN_BACKGROUND_SOURCE, DEFAULT_FULLSCREEN_ENABLE_FALLBACK_RULE,
    DEFAULT_FULLSCREEN_ENABLE_SPECIFIC_MONITOR, DEFAULT_FULLSCREEN_FALLBACK_TARGET,
    DEFAULT_FULLSCREEN_HIDE_IDLE_CURSOR, DEFAULT_FULLSCREEN_HIDE_SELECTION_WITH_CURSOR,
    DEFAULT_FULLSCREEN_IDLE_CURSOR_MS, DEFAULT_FULLSCREEN_MEDIA_BAR_ENABLED,
    DEFAULT_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_CROP,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DELETE, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DUPLICATE,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_MUTE, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_AS_TAB,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_WITH, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_REMOVE,
    DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_STICKY, DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_ZOOM,
    DEFAULT_FULLSCREEN_PLAYER_BAR_FADE, DEFAULT_FULLSCREEN_PLAYER_BAR_HEIGHT_PX,
    DEFAULT_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT, DEFAULT_FULLSCREEN_PLAYER_BAR_REVEAL,
    DEFAULT_FULLSCREEN_PLAYER_BAR_STYLE, DEFAULT_FULLSCREEN_PLAYER_NORMALIZE, DEFAULT_FULLSCREEN_PLAYER_SEEK,
    DEFAULT_FULLSCREEN_PLAYER_SHOW_MS, DEFAULT_FULLSCREEN_PRIMARY_BAR_ENABLED,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_REVEAL_DISTANCE_PX, DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_CLOSE,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_HIDE_CURSOR, DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_MUTE_ALL,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PIN, DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PLAY_PAUSE_ALL,
    DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_RESET_LAYOUT, DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_SEND_TO_TAB,
    DEFAULT_FULLSCREEN_RATING_SHOW_FEEDBACK, DEFAULT_FULLSCREEN_SELECTION_BRIGHTEN,
    DEFAULT_FULLSCREEN_SELECTION_CORNER_MARKS, DEFAULT_FULLSCREEN_SOUND_AUTO_CONTROL, DEFAULT_FULLSCREEN_SOUND_FOLLOWS,
    DEFAULT_FULLSCREEN_SOUND_HAND_OVER, DEFAULT_FULLSCREEN_SPECIFIC_MONITOR, DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT,
    DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS, DEFAULT_FULLSCREEN_ZOOM_MINIMAP_POSITION,
    DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS, DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT, DEFAULT_FULLSCREEN_ZOOM_RESAMPLE,
    DEFAULT_FULLSCREEN_ZOOM_SMOOTH, DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT, DEFAULT_VIEWER_ALLOW_MULTIPLE_IMAGE_TABS,
    DEFAULT_VIEWER_ALLOW_MULTIPLE_VIDEO_TABS, DEFAULT_VIEWER_BACKGROUND_MODE, DEFAULT_VIEWER_OPEN_IMAGES_IN_TAB,
    DEFAULT_VIEWER_OPEN_VIDEOS_IN_TAB, DEFAULT_VIEWER_SHOW_TAB_RATING_BADGE, DEFAULT_VIEWER_VIDEO_JUMP_LONG_SECS,
    DEFAULT_VIEWER_VIDEO_JUMP_SECS,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerImageSettings {
    pub open_in_tab: bool,
    pub allow_multiple_tabs: bool,
}

impl Default for ViewerImageSettings {
    fn default() -> Self {
        Self {
            open_in_tab: DEFAULT_VIEWER_OPEN_IMAGES_IN_TAB,
            allow_multiple_tabs: DEFAULT_VIEWER_ALLOW_MULTIPLE_IMAGE_TABS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerVideoSettings {
    pub open_in_tab: bool,
    pub allow_multiple_tabs: bool,
    /// Added in 0.4.6: how far the jump keys of the video player move, in seconds.
    pub jump_secs: u32,
    /// Added in 0.4.6: how far the long jump keys move, in seconds.
    pub jump_long_secs: u32,
    /// Added in 0.5.4: support for the mpv script `ab_repeat.lua`.
    pub ab_repeat: AbRepeatSettings,
}

impl Default for ViewerVideoSettings {
    fn default() -> Self {
        Self {
            open_in_tab: DEFAULT_VIEWER_OPEN_VIDEOS_IN_TAB,
            allow_multiple_tabs: DEFAULT_VIEWER_ALLOW_MULTIPLE_VIDEO_TABS,
            jump_secs: DEFAULT_VIEWER_VIDEO_JUMP_SECS,
            jump_long_secs: DEFAULT_VIEWER_VIDEO_JUMP_LONG_SECS,
            ab_repeat: AbRepeatSettings::default(),
        }
    }
}

/// Support for the mpv script `ab_repeat.lua` (A-B repeats in the control bar of a video; `docs/mpv_ab_repeat.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AbRepeatSettings {
    /// Look for the script in the players.
    pub enabled: bool,
    /// "each" or "grey".
    pub colors: String,
    pub click_loads: bool,
    pub hover_name: bool,
    pub hover_times: bool,
    pub drag_flags: bool,
    /// "pages" or "scroll".
    pub menu: String,
    pub page_size: u32,
}

impl Default for AbRepeatSettings {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_AB_REPEAT_ENABLED,
            colors: DEFAULT_AB_REPEAT_COLORS.to_owned(),
            click_loads: DEFAULT_AB_REPEAT_CLICK_LOADS,
            hover_name: DEFAULT_AB_REPEAT_HOVER_NAME,
            hover_times: DEFAULT_AB_REPEAT_HOVER_TIMES,
            drag_flags: DEFAULT_AB_REPEAT_DRAG_FLAGS,
            menu: DEFAULT_AB_REPEAT_MENU.to_owned(),
            page_size: DEFAULT_AB_REPEAT_PAGE_SIZE,
        }
    }
}

impl AbRepeatSettings {
    /// Unknown names fall back to the defaults; the page size stays in its range.
    pub fn sanitize(&mut self) {
        if !matches!(self.colors.as_str(), "each" | "grey") {
            DEFAULT_AB_REPEAT_COLORS.clone_into(&mut self.colors);
        }
        if !matches!(self.menu.as_str(), "pages" | "scroll") {
            DEFAULT_AB_REPEAT_MENU.clone_into(&mut self.menu);
        }
        self.page_size = self.page_size.clamp(MIN_AB_REPEAT_PAGE_SIZE, MAX_AB_REPEAT_PAGE_SIZE);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerTabSettings {
    pub show_rating_badge: bool,
    /// Where media tabs show their close button (`VIEWER_TAB_CLOSE_BUTTON_MODES`, added in 0.6.14).
    pub close_buttons: String,
}

impl Default for ViewerTabSettings {
    fn default() -> Self {
        Self {
            show_rating_badge: DEFAULT_VIEWER_SHOW_TAB_RATING_BADGE,
            close_buttons: super::DEFAULT_VIEWER_TAB_CLOSE_BUTTONS.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerSettings {
    pub images: ViewerImageSettings,
    pub videos: ViewerVideoSettings,
    pub tabs: ViewerTabSettings,
    pub background_mode: String,
}

impl Default for ViewerSettings {
    fn default() -> Self {
        Self {
            images: ViewerImageSettings::default(),
            videos: ViewerVideoSettings::default(),
            tabs: ViewerTabSettings::default(),
            background_mode: DEFAULT_VIEWER_BACKGROUND_MODE.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenMediaBarSettings {
    pub enabled: bool,
    pub show_mute: bool,
    pub show_sticky: bool,
    pub show_duplicate: bool,
    pub show_open_as_tab: bool,
    pub show_open_with: bool,
    pub show_delete: bool,
    /// Remove (several media) and Close (the last medium) are one button that changes its role.
    pub show_remove: bool,
    pub show_zoom: bool,
    pub show_crop: bool,
    pub reveal_distance_px: u32,
}

impl Default for FullscreenMediaBarSettings {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_FULLSCREEN_MEDIA_BAR_ENABLED,
            show_mute: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_MUTE,
            show_sticky: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_STICKY,
            show_duplicate: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DUPLICATE,
            show_open_as_tab: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_AS_TAB,
            show_open_with: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_WITH,
            show_delete: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DELETE,
            show_remove: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_REMOVE,
            show_zoom: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_ZOOM,
            show_crop: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_CROP,
            reveal_distance_px: DEFAULT_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenPrimaryBarSettings {
    pub enabled: bool,
    pub show_send_to_tab: bool,
    pub show_close: bool,
    pub show_mute_all: bool,
    pub show_play_pause_all: bool,
    pub show_reset_layout: bool,
    pub show_hide_cursor: bool,
    pub show_pin: bool,
    pub reveal_distance_px: u32,
    /// Shared with the "hide idle mouse cursor" button of the bar: there is one value, the button writes it.
    pub hide_idle_cursor: bool,
    pub idle_cursor_ms: u32,
    /// The marks of the selected medium go with the hidden cursor (and come back with it).
    pub hide_selection_with_cursor: bool,
}

impl Default for FullscreenPrimaryBarSettings {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_FULLSCREEN_PRIMARY_BAR_ENABLED,
            show_send_to_tab: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_SEND_TO_TAB,
            show_close: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_CLOSE,
            show_mute_all: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_MUTE_ALL,
            show_play_pause_all: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PLAY_PAUSE_ALL,
            show_reset_layout: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_RESET_LAYOUT,
            show_hide_cursor: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_HIDE_CURSOR,
            show_pin: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PIN,
            reveal_distance_px: DEFAULT_FULLSCREEN_PRIMARY_BAR_REVEAL_DISTANCE_PX,
            hide_idle_cursor: DEFAULT_FULLSCREEN_HIDE_IDLE_CURSOR,
            idle_cursor_ms: DEFAULT_FULLSCREEN_IDLE_CURSOR_MS,
            hide_selection_with_cursor: DEFAULT_FULLSCREEN_HIDE_SELECTION_WITH_CURSOR,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenSoundSettings {
    pub auto_control: bool,
    pub follows: String, // "first", "newest", "selected" or "nobody"
    pub hand_over: bool,
}

impl Default for FullscreenSoundSettings {
    fn default() -> Self {
        Self {
            auto_control: DEFAULT_FULLSCREEN_SOUND_AUTO_CONTROL,
            follows: DEFAULT_FULLSCREEN_SOUND_FOLLOWS.to_string(),
            hand_over: DEFAULT_FULLSCREEN_SOUND_HAND_OVER,
        }
    }
}

/// Rating with the keyboard in the Multiview (fullscreen and tab view).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenRatingSettings {
    /// The icon of the new rating appears on every tile for a moment.
    pub show_feedback: bool,
}

impl Default for FullscreenRatingSettings {
    fn default() -> Self {
        Self {
            show_feedback: DEFAULT_FULLSCREEN_RATING_SHOW_FEEDBACK,
        }
    }
}

/// How the selected medium of a Multiview of several media is marked (fullscreen and tab view, `docs/multiview_focus.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenSelectionSettings {
    /// The empty area around the selected medium (its column) is lightened: "off", "weak", "medium" or "strong".
    pub brighten: String,
    /// The length of the corner marks: "short", "medium" or "long" (they cannot be switched off).
    pub corner_marks: String,
}

impl Default for FullscreenSelectionSettings {
    fn default() -> Self {
        Self {
            brighten: DEFAULT_FULLSCREEN_SELECTION_BRIGHTEN.to_string(),
            corner_marks: DEFAULT_FULLSCREEN_SELECTION_CORNER_MARKS.to_string(),
        }
    }
}

/// Image zoom in the Multiview (fullscreen and tab view).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenZoomSettings {
    pub step_percent: u32,
    pub max_percent: u32,
    pub out_percent: u32,
    pub smooth: bool,
    pub minimap_position: String, // "bottom_left" or "bottom_right"
    pub minimap_always: bool,
    pub minimap_secs: u32,
    pub resample: String, // "lanczos3", "catmull_rom", "bilinear" or "nearest"
}

impl Default for FullscreenZoomSettings {
    fn default() -> Self {
        Self {
            step_percent: DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT,
            max_percent: DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT,
            out_percent: DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT,
            smooth: DEFAULT_FULLSCREEN_ZOOM_SMOOTH,
            minimap_position: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_POSITION.to_string(),
            minimap_always: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS,
            minimap_secs: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS,
            resample: DEFAULT_FULLSCREEN_ZOOM_RESAMPLE.to_string(),
        }
    }
}

/// The control bar of a video in the Multiview (fullscreen and tab view): Settings > Multiview > Player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenPlayerSettings {
    pub bar_style: String, // "bar" or "pill"
    pub bar_opacity_percent: u32,
    pub bar_height_px: u32,
    pub reveal: String, // "bottom" or "anywhere"
    pub fade: bool,
    pub show_ms: bool,
    pub seek: String, // "exact", "osc", "keyframes" or "mpv"
    /// Added in 0.6.12: the volume normalization every video starts with ("off", "loudness", "dynamic", "speech",
    /// "compressor"); the filter menu of a video overrides it for that video in its view.
    pub normalize: String,
}

impl Default for FullscreenPlayerSettings {
    fn default() -> Self {
        Self {
            bar_style: DEFAULT_FULLSCREEN_PLAYER_BAR_STYLE.to_string(),
            bar_opacity_percent: DEFAULT_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT,
            bar_height_px: DEFAULT_FULLSCREEN_PLAYER_BAR_HEIGHT_PX,
            reveal: DEFAULT_FULLSCREEN_PLAYER_BAR_REVEAL.to_string(),
            fade: DEFAULT_FULLSCREEN_PLAYER_BAR_FADE,
            show_ms: DEFAULT_FULLSCREEN_PLAYER_SHOW_MS,
            seek: DEFAULT_FULLSCREEN_PLAYER_SEEK.to_string(),
            normalize: DEFAULT_FULLSCREEN_PLAYER_NORMALIZE.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenBackgroundSettings {
    pub source: String, // "theme", "viewer" or "custom"
    pub color: String,  // "#rrggbb"
}

impl Default for FullscreenBackgroundSettings {
    fn default() -> Self {
        Self {
            source: DEFAULT_FULLSCREEN_BACKGROUND_SOURCE.to_string(),
            color: DEFAULT_FULLSCREEN_BACKGROUND_COLOR.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenSettings {
    pub enable_specific_monitor: bool,
    pub specific_monitor: String,
    pub enable_fallback_rule: bool,
    pub fallback_target: String, // "same" or "different"
    /// Called `action_bar` before the primary action bar existed; old settings files keep working.
    #[serde(alias = "action_bar")]
    pub media_bar: FullscreenMediaBarSettings,
    pub primary_bar: FullscreenPrimaryBarSettings,
    pub sound: FullscreenSoundSettings,
    pub rating: FullscreenRatingSettings,
    pub selection: FullscreenSelectionSettings,
    pub background: FullscreenBackgroundSettings,
    pub zoom: FullscreenZoomSettings,
    pub player: FullscreenPlayerSettings,
}

impl Default for FullscreenSettings {
    fn default() -> Self {
        Self {
            enable_specific_monitor: DEFAULT_FULLSCREEN_ENABLE_SPECIFIC_MONITOR,
            specific_monitor: DEFAULT_FULLSCREEN_SPECIFIC_MONITOR.to_string(),
            enable_fallback_rule: DEFAULT_FULLSCREEN_ENABLE_FALLBACK_RULE,
            fallback_target: DEFAULT_FULLSCREEN_FALLBACK_TARGET.to_string(),
            media_bar: FullscreenMediaBarSettings::default(),
            primary_bar: FullscreenPrimaryBarSettings::default(),
            sound: FullscreenSoundSettings::default(),
            rating: FullscreenRatingSettings::default(),
            selection: FullscreenSelectionSettings::default(),
            background: FullscreenBackgroundSettings::default(),
            zoom: FullscreenZoomSettings::default(),
            player: FullscreenPlayerSettings::default(),
        }
    }
}
