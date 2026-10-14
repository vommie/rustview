//! Instance behavior around the tiles: creating and starting them and decoding images.

use super::audio::{self, TileAudio};
use super::events::{DecodedImage, PlayerEvent};
use super::image_decode;
use super::instance::MultiviewInstance;
use super::model::{AspectSource, TileKind, aspect_differs, aspect_from_size};
use super::teardown::teardown_videos;
use super::tile;
use crate::library::models::MediaId;
use crate::settings::LogObscurer;
use slint::{Rgb8Pixel, SharedPixelBuffer};

/// Error texts can embed file paths; with log obfuscation active they are not written to the log.
fn loggable(error: &str) -> &str {
    if LogObscurer::is_active() {
        "<obfuscated>"
    } else {
        error
    }
}

impl MultiviewInstance {
    /// Starts every tile once the host exists: the leftmost video gets the sound, the others are muted by policy.
    pub fn start_all(&mut self) {
        self.assign_audio_on_open();
        for index in 0..self.tiles.len() {
            self.start_tile(index);
        }
        self.apply_audio();
        self.bump();
        tracing::info!(
            instance = self.id,
            tiles = self.tiles.len(),
            width = self.container.width,
            height = self.container.height,
            scale = self.container.dpi,
            "[Multiview: Open] Tiles created"
        );
    }

    fn assign_audio_on_open(&mut self) {
        let mut slots: Vec<Option<TileAudio>> = self.tiles.iter().map(|t| t.audio).collect();
        audio::assign_on_open(&mut slots);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }
    }

    pub(super) fn start_tile(&mut self, index: usize) {
        match self.tiles.get(index).map(|t| (t.kind, t.video.is_some())) {
            Some((TileKind::Video, true)) => self.reload_video(index),
            Some((TileKind::Video, false)) => self.start_video(index),
            Some((TileKind::Image, _)) => self.start_image(index),
            None => {}
        }
    }

    /// A video tile took over the player of the tile it replaced: the player plays the new file in its existing window.
    fn reload_video(&mut self, index: usize) {
        let (media_id, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.path.clone())
        };
        let Some(video) = self.tiles[index].video.as_ref() else {
            return;
        };
        match tile::reload_video(video, media_id, &path) {
            Ok(()) => tracing::info!(
                media_id,
                index,
                filename = %LogObscurer::filename(&self.tiles[index].filename),
                "[Multiview: Tile] Video tile recycled the player of the tile it replaced"
            ),
            Err(e) => {
                tracing::error!(media_id, index, error = %loggable(&e), "[Multiview: Tile] Recycled video failed");
                let tile = &mut self.tiles[index];
                tile.error = Some(format!("This video could not be played\n{e}"));
                tile.audio = None;
                let failed = tile.video.take();
                teardown_videos(failed.into_iter().collect());
                self.bump();
            }
        }
    }

    fn start_video(&mut self, index: usize) {
        let content = self.host_content(index).unwrap_or_default();
        let (media_id, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.path.clone())
        };
        let created = tile::create_video(
            &self.sink,
            &self.me,
            self.host.surface.parent_xid,
            content,
            media_id,
            &path,
        );
        match created {
            Ok(video) => {
                let tile = &mut self.tiles[index];
                tile.video = Some(video);
                tracing::info!(
                    media_id,
                    index,
                    filename = %LogObscurer::filename(&tile.filename),
                    "[Multiview: Tile] Video tile started"
                );
            }
            Err(e) => {
                tracing::error!(media_id, index, error = %loggable(&e), "[Multiview: Tile] Video tile failed");
                let tile = &mut self.tiles[index];
                tile.error = Some(format!("This video could not be played\n{e}"));
                tile.audio = None;
                self.bump();
            }
        }
    }

    fn start_image(&mut self, index: usize) {
        let (media_id, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.path.clone())
        };
        let (max_w, max_h) = (
            self.container.width.max(1) as u32,
            self.container.height.max(1) as u32,
        );
        let (sink, me) = (self.sink.clone(), self.me.clone());
        let spawned = std::thread::Builder::new()
            .name("multiview-image".into())
            .spawn(move || {
                let result: DecodedImage = image_decode::load_scaled(&path, max_w, max_h);
                sink.post(me, PlayerEvent::ImageDecoded { media_id, result });
            });
        if let Err(e) = spawned {
            tracing::error!(media_id, error = %e, "[Multiview: Tile] Could not start the image decoder thread");
            self.on_image_loaded(media_id, Err("This image could not be loaded".to_string()));
        }
    }

    /// Shows `pixels` (a thumbnail the application already holds) in an image tile whose decode is still running, so
    /// browsing does not flash an empty tile. The decoded image replaces it.
    pub fn set_placeholder(&mut self, media_id: MediaId, pixels: SharedPixelBuffer<Rgb8Pixel>) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.media_id == media_id) else {
            return;
        };
        if tile.kind == TileKind::Image && tile.is_loading && tile.pixels.is_none() {
            tile.pixels = Some(pixels);
            self.bump();
        }
    }

    /// Result of a background decode. Ignored if the tile left in the meantime.
    pub fn on_image_loaded(&mut self, media_id: MediaId, result: DecodedImage) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.media_id == media_id) else {
            return;
        };
        tile.is_loading = false;
        let mut relayout = false;
        match result {
            Ok((width, height, pixels)) => {
                let aspect = aspect_from_size(width, height, tile.aspect);
                relayout = aspect_differs(aspect, tile.aspect);
                tile.aspect = aspect;
                tile.aspect_source = AspectSource::Probed;
                tile.pixels = Some(pixels);
                tracing::debug!(
                    media_id,
                    width,
                    height,
                    filename = %LogObscurer::filename(&tile.filename),
                    "[Multiview: Tile] Image tile decoded"
                );
            }
            Err(e) => {
                tracing::warn!(media_id, error = %loggable(&e), "[Multiview: Tile] Image tile failed");
                tile.error = Some(format!("This image could not be loaded\n{e}"));
            }
        }
        if relayout {
            self.apply_layout();
        } else {
            self.bump();
        }
    }
}
