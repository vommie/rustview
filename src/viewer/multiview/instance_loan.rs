//! Instance behavior for a viewer tab that is shown in the fullscreen: a normal image or video tab becomes a Multiview of
//! one tile (isolated, it never follows the Browse selection).
//!
//! An image tab hands over its decoded pixels. A video tab lends its running player and window: nothing restarts, the
//! position and the settings of the player carry over, and the player and window go back to the tab when the fullscreen
//! closes (`return_loans`), never into a teardown.

use super::audio::TileAudio;
use super::instance::MultiviewInstance;
use super::model::TileKind;
use super::tile::{self, VideoTile};
use crate::library::models::MediaId;
use crate::settings::LogObscurer;

impl MultiviewInstance {
    /// Lets the tile of `media_id` show the lent player of a video tab: its callbacks report to this instance for the time of
    /// the loan, and playback continues.
    pub fn lend_video(&mut self, media_id: MediaId, video: VideoTile) -> bool {
        let Some(index) = self
            .tiles
            .iter()
            .position(|t| t.media_id == media_id && t.kind == TileKind::Video)
        else {
            return false;
        };
        tile::connect_callbacks(&video.player, &self.sink, &self.me, &video.current_media);
        video.player.set_user_paused(false);
        if let Err(e) = video.player.play() {
            tracing::warn!(media_id, error = %e, "[Multiview: Loan] The lent player did not resume");
        }
        let tile = &mut self.tiles[index];
        tile.video = Some(video);
        tile.audio = Some(TileAudio::native());
        tile.is_loading = false;
        tracing::info!(
            media_id,
            filename = %LogObscurer::filename(&tile.filename),
            "[Multiview: Loan] A video tab lent its player to the fullscreen"
        );
        self.bump();
        true
    }

    /// The tile of `media_id` cannot show its medium (for example a tab whose file is missing).
    pub fn fail_tile(&mut self, media_id: MediaId, text: String) {
        if let Some(tile) = self.tiles.iter_mut().find(|t| t.media_id == media_id) {
            tile.error = Some(text);
            tile.is_loading = false;
            tile.audio = None;
            self.bump();
        }
    }

    /// Starts the decoder of every image tile that has neither pixels nor an error yet.
    pub fn start_pending_images(&mut self) {
        for index in 0..self.tiles.len() {
            let tile = &self.tiles[index];
            if tile.kind == TileKind::Image && tile.is_loading && tile.pixels.is_none() {
                self.start_tile(index);
            }
        }
    }

    /// Gives the lent players and windows back to their tabs. The windows must already be parked below the main window
    /// (`park`). If the tab is gone (`tab_alive` is false) nothing can take them back, so they are torn down with the
    /// instance as usual. Returns the number of windows handed back.
    pub fn return_loans(&mut self, tab_alive: bool) -> usize {
        if !tab_alive {
            return 0;
        }
        let mut returned = 0;
        for tile in &mut self.tiles {
            let Some(video) = tile.video.take() else {
                continue;
            };
            let Some(slot) = video.loan.clone() else {
                tile.video = Some(video);
                continue;
            };
            *slot.lock() = Some(video.window);
            returned += 1;
            tracing::info!(
                media_id = tile.media_id,
                "[Multiview: Loan] The player and window went back to their tab"
            );
        }
        if returned > 0 {
            self.bump();
        }
        returned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::instance::SharedInstance;
    use crate::viewer::multiview::tile::TileSeed;
    use std::path::PathBuf;

    fn instance(kind: TileKind) -> SharedInstance {
        let shared = MultiviewInstance::detached(vec![TileSeed {
            media_id: 7,
            kind,
            aspect: 1.78,
            path: PathBuf::new(),
            filename: "7.mp4".into(),
        }]);
        shared
            .lock()
            .set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        shared
    }

    #[test]
    fn a_tile_that_cannot_be_shown_explains_why() {
        let shared = instance(TileKind::Video);
        let mut mv = shared.lock();
        let before = mv.revision();
        mv.fail_tile(7, "This video could not be played\nmissing".to_string());
        let view = mv.view();
        assert!(view.tiles[0].error.as_deref().is_some_and(|e| e.contains("missing")));
        assert!(!view.tiles[0].is_loading);
        assert_ne!(mv.revision(), before);
        mv.fail_tile(99, "unknown".into());
    }

    #[test]
    fn nothing_goes_back_to_a_tab_that_lent_nothing() {
        let shared = instance(TileKind::Image);
        let mut mv = shared.lock();
        assert_eq!(mv.return_loans(true), 0);
        assert_eq!(mv.return_loans(false), 0);
    }
}
