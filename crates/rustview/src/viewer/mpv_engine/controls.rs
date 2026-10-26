//! The video player keys of `RustView` (Settings > Hotkeys > Video Player) and the control bar of a video on mpv: mpv takes
//! no keys and no pointer input itself, these commands carry out what the keys and the control bar ask for.

use super::MpvPlayer;
use crate::viewer::error::ViewerError;

/// How exactly an absolute seek lands (mpv's seek flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekPrecision {
    /// On the exact frame (slower: mpv decodes from the keyframe before it).
    Exact,
    /// On the nearest keyframe (the fastest).
    Keyframes,
    /// As mpv is configured (its `hr-seek` option).
    MpvSetting,
}

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

    /// Goes to `secs` as `precision` says (the progress bar of the control bar; a seek that comes while one runs replaces it
    /// in mpv). Shows no on-screen message: `mpv_command` does not use the OSD.
    pub fn seek_to(&self, secs: f64, precision: SeekPrecision) -> Result<(), ViewerError> {
        let flags = match precision {
            SeekPrecision::Exact => "absolute+exact",
            SeekPrecision::Keyframes => "absolute+keyframes",
            SeekPrecision::MpvSetting => "absolute",
        };
        self.mpv
            .command("seek", &[&format!("{:.3}", secs.max(0.0)), flags])
            .map_err(|e| ViewerError::Mpv(format!("Failed to seek: {e:?}")))
    }

    /// Faster (`factor` above 1) or slower.
    pub fn multiply_speed(&self, factor: f64) -> Result<(), ViewerError> {
        self.mpv
            .command("multiply", &["speed", &format!("{factor:.6}")])
            .map_err(|e| ViewerError::Mpv(format!("Failed to change the speed: {e:?}")))
    }
}
