//! What the observer thread of a video's player reports (`PlayerEvent::{Playback, VideoParams, Failed}`), on the UI thread.

use super::follow::TileKey;
use super::instance::MultiviewInstance;
use rustview_core::settings::LogObscurer;

impl MultiviewInstance {
    /// Pause, end of file, duration or volume of a video changed: the view (control bar, time badges) shows the new numbers.
    pub fn on_player_playback(&mut self, key: TileKey) {
        if self.tiles.iter().any(|t| t.key == key && t.video.is_some()) {
            self.bump();
        }
    }

    /// mpv reported the display size of a video: the tile takes its real aspect ratio and gets its zoom.
    pub fn on_video_params(&mut self, key: TileKey) {
        if self.tiles.iter().any(|t| t.key == key && t.video.is_some()) {
            self.probe_aspects();
        }
    }

    /// mpv cannot play the video of a tile (or stopped by itself): the tile shows the reason where the video would be. The
    /// player stays, so a refresh (F5) can try the file again.
    pub fn on_player_failed(&mut self, key: TileKey, message: &str) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.key == key) else {
            return;
        };
        tracing::warn!(
            media_id = tile.media_id,
            filename = %LogObscurer::filename(&tile.filename),
            reason = %LogObscurer::foreign_text(message, Some(&tile.path)),
            "[Multiview: Tile] The video cannot be played"
        );
        tile.error = Some(format!("This video could not be played\n{message}"));
        tile.zoom = None;
        self.bump();
    }
}
