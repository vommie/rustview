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
