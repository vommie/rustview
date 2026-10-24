//! The video player keys of `RustView` (Settings > Hotkeys > Video Player) on mpv: mpv takes no keys itself, these commands do
//! what its own keys did (and show mpv's on-screen message for them until `RustView` has an OSD of its own).

use super::MpvPlayer;
use crate::viewer::error::ViewerError;

impl MpvPlayer {
    /// One frame forward or back (mpv pauses for it).
    pub fn frame_step(&self, forward: bool) -> Result<(), ViewerError> {
        let command = if forward { "frame-step" } else { "frame-back-step" };
        self.mpv
            .command(command, &[])
            .map_err(|e| ViewerError::Mpv(format!("Failed to step a frame: {e:?}")))
    }

    /// Louder or quieter by `delta` percent.
    pub fn add_volume(&self, delta: f64) -> Result<(), ViewerError> {
        self.mpv
            .command("add", &["volume", &format!("{delta}")])
            .map_err(|e| ViewerError::Mpv(format!("Failed to change the volume: {e:?}")))
    }

    /// Faster (`factor` above 1) or slower.
    pub fn multiply_speed(&self, factor: f64) -> Result<(), ViewerError> {
        self.mpv
            .command("multiply", &["speed", &format!("{factor:.6}")])
            .map_err(|e| ViewerError::Mpv(format!("Failed to change the speed: {e:?}")))
    }
}
