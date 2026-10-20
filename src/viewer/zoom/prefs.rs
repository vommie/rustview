//! The user's zoom preferences (Settings > Fullscreen > Image Zoom) as a snapshot everything that zooms can read cheaply.
//!
//! Like `multiview::prefs`: the settings dialog is the only writer (`apply`, at startup and after every change), a change takes
//! effect with the next zoom.

use super::geometry::Limits;
use super::render::Resample;
use crate::settings::schema::{
    DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT, DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS, DEFAULT_FULLSCREEN_ZOOM_SMOOTH,
    DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT, FullscreenSettings,
};
use parking_lot::RwLock;

/// The smallest zoom (10% of the fitted size): below that the picture is a dot.
pub const MIN_ZOOM: f64 = 0.1;

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
    /// Zooms glide instead of jumping.
    pub smooth: bool,
    pub minimap_corner: MinimapCorner,
    /// The minimap is shown whenever the picture is zoomed in, not only on mouse-over and after a zoom.
    pub minimap_always: bool,
    pub resample: Resample,
}

impl ZoomPrefs {
    pub const DEFAULT: Self = Self {
        wheel_step: DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT as f64 / 100.0,
        max_zoom: DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT as f64 / 100.0,
        smooth: DEFAULT_FULLSCREEN_ZOOM_SMOOTH,
        minimap_corner: MinimapCorner::BottomLeft,
        minimap_always: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS,
        resample: Resample::Lanczos3,
    };

    pub fn limits(&self) -> Limits {
        Limits::new(MIN_ZOOM, self.max_zoom)
    }

    pub fn from_settings(settings: &FullscreenSettings) -> Self {
        let zoom = &settings.zoom;
        Self {
            wheel_step: f64::from(zoom.step_percent.max(1)) / 100.0,
            max_zoom: f64::from(zoom.max_percent.max(150)) / 100.0,
            smooth: zoom.smooth,
            minimap_corner: MinimapCorner::from_setting(&zoom.minimap_position),
            minimap_always: zoom.minimap_always,
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
        assert!((d.wheel_step - 0.10).abs() < 1e-9);
        assert!(!d.smooth && !d.minimap_always);
        assert_eq!(d.minimap_corner, MinimapCorner::BottomLeft);
        assert_eq!(d.resample, Resample::Lanczos3);
        let l = d.limits();
        assert!(l.min < 1.0 && l.max > 1.0);
    }

    #[test]
    fn the_corner_names_round_trip() {
        for corner in [MinimapCorner::BottomLeft, MinimapCorner::BottomRight] {
            assert_eq!(MinimapCorner::from_setting(corner.as_setting()), corner);
        }
        assert_eq!(MinimapCorner::from_setting("garbage"), MinimapCorner::BottomLeft);
    }
}
