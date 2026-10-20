//! The user's zoom preferences (Settings > Fullscreen > Image Zoom) as a snapshot everything that zooms can read cheaply.
//!
//! Like `multiview::prefs`: the settings dialog is the only writer (`apply`, at startup and after every change), a change takes
//! effect with the next zoom.

use super::geometry::Limits;
use super::render::Resample;
use crate::settings::schema::{
    DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT, DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS, DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS,
    DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT, DEFAULT_FULLSCREEN_ZOOM_SMOOTH, DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT, FullscreenSettings,
};
use parking_lot::RwLock;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimapCorner {
    BottomLeft,
    BottomRight,
}

impl MinimapCorner {
    pub fn from_setting(name: &str) -> Self {
        if name.trim().eq_ignore_ascii_case("bottom_right") { Self::BottomRight } else { Self::BottomLeft }
    }

    pub fn as_setting(self) -> &'static str {
        match self {
            Self::BottomLeft => "bottom_left",
            Self::BottomRight => "bottom_right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomPrefs {
    /// A step of Ctrl + mouse wheel over an image, as a share (0.1 = 10%).
    pub wheel_step: f64,
    /// The largest zoom, as a factor of the fitted size (10.0 = 1000%).
    pub max_zoom: f64,
    /// The smallest zoom, as a factor of the fitted size (1.0 = no zooming out).
    pub min_zoom: f64,
    /// Zooms glide instead of jumping.
    pub smooth: bool,
    pub minimap_corner: MinimapCorner,
    /// The minimap is shown whenever the picture is zoomed in, not only on mouse-over and after a zoom.
    pub minimap_always: bool,
    /// How long the minimap stays after the zoom changed.
    pub minimap_timeout: Duration,
    pub resample: Resample,
}

/// The smallest zoom the setting "how far the picture may shrink" (percent) allows: 200 = a third, 0 = the fit.
pub fn min_zoom_for(out_percent: u32) -> f64 {
    1.0 / (1.0 + f64::from(out_percent) / 100.0)
}

impl ZoomPrefs {
    pub const DEFAULT: Self = Self {
        wheel_step: DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT as f64 / 100.0,
        max_zoom: DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT as f64 / 100.0,
        min_zoom: 1.0 / (1.0 + DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT as f64 / 100.0),
        smooth: DEFAULT_FULLSCREEN_ZOOM_SMOOTH,
        minimap_corner: MinimapCorner::BottomLeft,
        minimap_always: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS,
        minimap_timeout: Duration::from_secs(DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS as u64),
        resample: Resample::Lanczos3,
    };

    pub fn limits(&self) -> Limits {
        Limits::new(self.min_zoom, self.max_zoom)
    }

    pub fn from_settings(settings: &FullscreenSettings) -> Self {
        let zoom = &settings.zoom;
        Self {
            wheel_step: f64::from(zoom.step_percent.max(1)) / 100.0,
            max_zoom: f64::from(zoom.max_percent.max(150)) / 100.0,
            min_zoom: min_zoom_for(zoom.out_percent),
            smooth: zoom.smooth,
            minimap_corner: MinimapCorner::from_setting(&zoom.minimap_position),
            minimap_always: zoom.minimap_always,
            minimap_timeout: Duration::from_secs(u64::from(zoom.minimap_secs.max(1))),
            resample: Resample::from_setting(&zoom.resample),
        }
    }
}

static PREFS: RwLock<ZoomPrefs> = RwLock::new(ZoomPrefs::DEFAULT);

pub fn current() -> ZoomPrefs {
    *PREFS.read()
}

/// Makes the fullscreen settings the current zoom preferences.
pub fn apply(settings: &FullscreenSettings) {
    let next = ZoomPrefs::from_settings(settings);
    let mut current = PREFS.write();
    if *current != next {
        tracing::info!(prefs = ?next, "[Zoom: Prefs] Preferences applied");
        *current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let d = ZoomPrefs::DEFAULT;
        assert!((d.wheel_step - 0.15).abs() < 1e-9);
        assert!(d.smooth && !d.minimap_always);
        assert_eq!(d.minimap_timeout, Duration::from_secs(3));
        assert!((d.min_zoom - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(d.minimap_corner, MinimapCorner::BottomLeft);
        assert_eq!(d.resample, Resample::Lanczos3);
        let l = d.limits();
        assert!(l.min < 1.0 && l.max > 1.0);
    }

    #[test]
    fn zooming_out_can_be_switched_off() {
        assert!((min_zoom_for(0) - 1.0).abs() < 1e-12);
        assert!((min_zoom_for(100) - 0.5).abs() < 1e-12);
        let mut settings = FullscreenSettings::default();
        settings.zoom.out_percent = 0;
        let prefs = ZoomPrefs::from_settings(&settings);
        assert!(!prefs.limits().can_zoom_out());
        settings.zoom.out_percent = 1000;
        assert!(ZoomPrefs::from_settings(&settings).limits().can_zoom_out());
    }

    #[test]
    fn the_corner_names_round_trip() {
        for corner in [MinimapCorner::BottomLeft, MinimapCorner::BottomRight] {
            assert_eq!(MinimapCorner::from_setting(corner.as_setting()), corner);
        }
        assert_eq!(MinimapCorner::from_setting("garbage"), MinimapCorner::BottomLeft);
    }
}
