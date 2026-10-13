//! Instance behavior around the tiles: creating and starting them, decoding images and the live reconcile with the
//! selection of the Browse view.

use super::audio::{self, TileAudio};
use super::events::{DecodedImage, PlayerEvent};
use super::image_decode;
use super::instance::MultiviewInstance;
use super::layout::Split;
use super::model::{AspectSource, TileKind, aspect_differs, aspect_from_size};
use super::reconcile::ReconcilePlan;
use super::teardown::teardown_videos;
use super::tile::{self, TileSeed, TileState, VideoTile};
use crate::library::models::MediaId;
use crate::settings::LogObscurer;
use std::collections::HashMap;

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
        match self.tiles.get(index).map(|t| t.kind) {
            Some(TileKind::Video) => self.start_video(index),
            Some(TileKind::Image) => self.start_image(index),
            None => {}
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

    /// Brings the tiles in line with a reconcile plan: removed items leave, new items take the freed slots, the remaining
    /// tiles keep running. `seeds` holds the new items.
    pub fn reconcile(&mut self, plan: &ReconcilePlan, mut seeds: HashMap<MediaId, TileSeed>) {
        if plan.is_noop() {
            return;
        }
        self.drag = None;
        self.clear_hover();
        let mut kept: HashMap<MediaId, TileState> =
            self.tiles.drain(..).map(|t| (t.media_id, t)).collect();

        let mut removed: Vec<VideoTile> = Vec::new();
        for id in &plan.remove {
            removed.extend(kept.remove(id).and_then(|t| t.video));
        }
        let mut added: Vec<usize> = Vec::new();
        for id in &plan.slots {
            if let Some(tile) = kept.remove(id) {
                self.tiles.push(tile);
            } else if let Some(seed) = seeds.remove(id) {
                added.push(self.tiles.len());
                self.tiles.push(TileState::from_seed(seed));
            }
        }
        removed.extend(kept.into_values().filter_map(|t| t.video));

        let removed_count = plan.remove.len();
        if removed_count != added.len() {
            self.split = Split::Auto;
        }
        let mut slots: Vec<Option<TileAudio>> = self.tiles.iter().map(|t| t.audio).collect();
        audio::after_reconcile(&mut slots, &added);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }

        self.apply_layout();
        for &index in &added {
            self.start_tile(index);
        }
        self.apply_audio();
        let teardown_count = removed.len();
        teardown_videos(removed);
        self.bump();
        tracing::info!(
            removed = removed_count,
            added = added.len(),
            shown = self.tiles.len(),
            players_closed = teardown_count,
            "[Multiview: Reconcile] Tiles updated to match the selection"
        );
    }
}
