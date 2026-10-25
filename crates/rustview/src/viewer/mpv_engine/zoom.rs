//! The zoom of a video, drawn by mpv itself: the Multiview's zoom model (`viewer::zoom`, the same as for pictures) decides where
//! the picture goes and `set_view` hands that to mpv (`video-zoom`, a power of two, and `video-pan-*`). `zoom_video_by` is the
//! fallback for a video whose size is not known yet.

use super::MpvPlayer;
use crate::viewer::zoom::mpv_view::MpvView;

impl MpvPlayer {
    /// Multiplies the size of the picture by `factor` (above 1 zooms in).
    pub fn zoom_video_by(&self, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let delta = factor.log2();
        if let Err(e) = self.mpv.command("add", &["video-zoom", &format!("{delta:.4}")]) {
            tracing::warn!(error = ?e, "[MpvPlayer: Zoom] Could not zoom the video");
        }
    }

    /// Shows the picture as `view` says (`viewer::zoom::mpv_view`).
    pub fn set_view(&self, view: &MpvView) {
        for (property, value) in [
            ("video-zoom", view.zoom),
            ("video-pan-x", view.pan_x),
            ("video-pan-y", view.pan_y),
        ] {
            if let Err(e) = self.mpv.set_property(property, value) {
                tracing::debug!(property, error = ?e, "[MpvPlayer: Zoom] mpv did not take the view");
            }
        }
    }

    /// The whole picture again: no zoom, no panning.
    pub fn reset_video_zoom(&self) {
        for property in [
            "video-zoom",
            "video-pan-x",
            "video-pan-y",
            "video-align-x",
            "video-align-y",
            "panscan",
        ] {
            let _ = self.mpv.set_property(property, 0.0);
        }
    }
}
