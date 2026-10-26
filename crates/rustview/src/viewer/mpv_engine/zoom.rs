//! mpv's own zoom of a video, only the fallback for a video whose size is not known yet: once it is, the Multiview's zoom model
//! (`viewer::zoom`, the same as for pictures) places and scales the video's texture in the canvas (`video_placement`) and mpv
//! always renders the whole picture.

use super::MpvPlayer;

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
