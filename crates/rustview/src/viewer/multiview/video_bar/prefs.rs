//! The settings of Fullscreen > Player as a global, like the zoom preferences (read on every tick of a host).

use super::view::DragPhase;
use crate::viewer::mpv_engine::SeekPrecision;
use parking_lot::RwLock;
use rustview_core::settings::schema::FullscreenPlayerSettings;
use rustview_core::util::cast::Az;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyle {
    /// A translucent bar over the whole width of the video.
    Bar,
    /// A floating pill with a margin to the edges of the video.
    Pill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealMode {
    /// The pointer in the lower part of the video shows the bar; it goes when the pointer leaves that part.
    Bottom,
    /// Any movement of the pointer over the video shows the bar; it goes after a moment without movement.
    Anywhere,
    /// The pointer anywhere over the video, resting or not (the small bar of a tab grid item; no setting).
    Hover,
}

/// How a click or a drag on the progress bar seeks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekMode {
    /// To the exact frame, the click and every step of a drag.
    Exact,
    /// Like mpv's own controller: a click to the exact frame, a drag by keyframes.
    LikeMpvController,
    /// To the nearest keyframe (the fastest).
    Keyframes,
    /// As mpv is configured (its `hr-seek` option).
    MpvSetting,
}

impl SeekMode {
    /// The seek a press (`Press`), a step of a drag (`Move`) or the release of the progress bar makes, if any. mpv's
    /// controller seeks by keyframes while dragging and does nothing more on release.
    pub fn seek_for(self, phase: DragPhase) -> Option<SeekPrecision> {
        match (self, phase) {
            (Self::Exact, _) | (Self::LikeMpvController, DragPhase::Press) => Some(SeekPrecision::Exact),
            (Self::LikeMpvController, DragPhase::Move) | (Self::Keyframes, _) => Some(SeekPrecision::Keyframes),
            (Self::LikeMpvController, DragPhase::Release) => None,
            (Self::MpvSetting, _) => Some(SeekPrecision::MpvSetting),
        }
    }

    fn from_name(name: &str) -> Self {
        match name {
            "osc" => Self::LikeMpvController,
            "keyframes" => Self::Keyframes,
            "mpv" => Self::MpvSetting,
            _ => Self::Exact,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoBarPrefs {
    pub style: BarStyle,
    /// The opacity of the bar's background, 0..=1.
    pub opacity: f32,
    /// The height of the bar in logical pixels.
    pub height: f32,
    pub reveal: RevealMode,
    pub fade: bool,
    /// The times of a new video start with milliseconds.
    pub show_ms: bool,
    pub seek: SeekMode,
}

impl VideoBarPrefs {
    pub fn from_settings(player: &FullscreenPlayerSettings) -> Self {
        Self {
            style: if player.bar_style == "pill" {
                BarStyle::Pill
            } else {
                BarStyle::Bar
            },
            opacity: player.bar_opacity_percent.min(100).az::<f32>() / 100.0,
            height: player.bar_height_px.az::<f32>(),
            reveal: if player.reveal == "anywhere" {
                RevealMode::Anywhere
            } else {
                RevealMode::Bottom
            },
            fade: player.fade,
            show_ms: player.show_ms,
            seek: SeekMode::from_name(&player.seek),
        }
    }
}

impl Default for VideoBarPrefs {
    fn default() -> Self {
        Self::from_settings(&FullscreenPlayerSettings::default())
    }
}

static PREFS: LazyLock<RwLock<VideoBarPrefs>> = LazyLock::new(|| RwLock::new(VideoBarPrefs::default()));

pub fn current() -> VideoBarPrefs {
    *PREFS.read()
}

/// Makes the player settings the current preferences (at startup and when they change).
pub fn apply(player: &FullscreenPlayerSettings) {
    let prefs = VideoBarPrefs::from_settings(player);
    *PREFS.write() = prefs;
    tracing::debug!(?prefs, "[Multiview: VideoBar] Preferences applied");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_become_preferences() {
        let defaults = VideoBarPrefs::default();
        assert_eq!(defaults.style, BarStyle::Bar);
        assert_eq!(defaults.reveal, RevealMode::Bottom);
        assert!((defaults.opacity - 0.7).abs() < 1e-6);
        assert!((defaults.height - 30.0).abs() < f32::EPSILON);
        assert!(defaults.fade && !defaults.show_ms);
        assert_eq!(defaults.seek, SeekMode::Exact);
        assert_eq!(SeekMode::from_name("osc"), SeekMode::LikeMpvController);
        assert_eq!(SeekMode::from_name("mpv"), SeekMode::MpvSetting);
    }

    #[test]
    fn every_seek_mode_seeks_as_promised() {
        use DragPhase::{Move, Press, Release};
        for phase in [Press, Move, Release] {
            assert_eq!(SeekMode::Exact.seek_for(phase), Some(SeekPrecision::Exact));
            assert_eq!(SeekMode::Keyframes.seek_for(phase), Some(SeekPrecision::Keyframes));
            assert_eq!(SeekMode::MpvSetting.seek_for(phase), Some(SeekPrecision::MpvSetting));
        }
        let osc = SeekMode::LikeMpvController;
        assert_eq!(osc.seek_for(Press), Some(SeekPrecision::Exact));
        assert_eq!(osc.seek_for(Move), Some(SeekPrecision::Keyframes));
        assert_eq!(osc.seek_for(Release), None);
        let pill = FullscreenPlayerSettings {
            bar_style: "pill".into(),
            reveal: "anywhere".into(),
            bar_opacity_percent: 40,
            ..FullscreenPlayerSettings::default()
        };
        let prefs = VideoBarPrefs::from_settings(&pill);
        assert_eq!(prefs.style, BarStyle::Pill);
        assert_eq!(prefs.reveal, RevealMode::Anywhere);
        assert!((prefs.opacity - 0.4).abs() < 1e-6);
    }
}
