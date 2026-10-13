//! Session behavior around the tiles: creating and starting them, decoding images and the live reconcile with the
//! selection of the Browse view.

use super::audio::{self, TileAudio};
use super::eligibility::{self, PlannedTile};
use super::image_tile;
use super::layout::Split;
use super::model::{AspectSource, TileKind, aspect_differs, aspect_from_size};
use super::reconcile;
use super::session::{Session, with_session};
use super::tiles::{self, TileRuntime, VideoTile};
use crate::library::models::MediaId;
use crate::settings::LogObscurer;
use crate::settings::schema::MULTIVIEW_MAX_TILES;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::HashMap;

type DecodedImage = Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), String>;

/// Error texts can embed file paths; with log obfuscation active they are not written to the log.
fn loggable(error: &str) -> &str {
    if LogObscurer::is_active() {
        "<obfuscated>"
    } else {
        error
    }
}

impl Session {
    pub(super) fn new_tile(&self, planned: PlannedTile) -> TileRuntime {
        let is_video = planned.kind == TileKind::Video;
        TileRuntime {
            media_id: planned.media_id,
            kind: planned.kind,
            filename: planned.filename,
            path: planned.path,
            aspect: planned.aspect,
            aspect_source: AspectSource::Library,
            image: None,
            is_loading: !is_video,
            error: None,
            video: None,
            audio: is_video.then(TileAudio::native),
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
        let content = self
            .layout
            .tiles
            .get(index)
            .map(|t| t.content)
            .unwrap_or_default();
        let (media_id, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.path.clone())
        };
        match tiles::create_video(&self.handle, self.fs_xid, content, media_id, &path) {
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
                self.sync_models();
            }
        }
    }

    fn start_image(&mut self, index: usize) {
        let (media_id, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.path.clone())
        };
        let generation = self.generation;
        let (max_w, max_h) = (self.monitor.width.max(1), self.monitor.height.max(1));
        let spawned = std::thread::Builder::new()
            .name("multiview-image".into())
            .spawn(move || {
                let result: DecodedImage = image_tile::load_scaled(&path, max_w, max_h);
                let _ = slint::invoke_from_event_loop(move || {
                    with_session(|s| s.on_image_loaded(generation, media_id, result))
                });
            });
        if let Err(e) = spawned {
            tracing::error!(media_id, error = %e, "[Multiview: Tile] Could not start the image decoder thread");
            self.on_image_loaded(
                generation,
                media_id,
                Err("This image could not be loaded".to_string()),
            );
        }
    }

    /// Result of a background decode. Ignored if the session changed or the tile left in the meantime.
    pub(super) fn on_image_loaded(
        &mut self,
        generation: u64,
        media_id: MediaId,
        result: DecodedImage,
    ) {
        if generation != self.generation {
            return;
        }
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
                tile.image = Some(slint::Image::from_rgb8(pixels));
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
            self.sync_models();
        }
    }

    /// Brings the tiles in line with the current selection: removed items leave, new items take the freed slots, the
    /// remaining tiles keep running. An empty selection changes nothing.
    pub(super) fn reconcile_now(&mut self) {
        if !self.populated {
            return;
        }
        let wanted = eligibility::wanted_ids(&self.state.read());
        let current: Vec<MediaId> = self.tiles.iter().map(|t| t.media_id).collect();
        let plan = reconcile::reconcile(&current, &wanted, MULTIVIEW_MAX_TILES);
        if plan.is_noop() {
            return;
        }

        self.drag = None;
        self.clear_hover();
        let mut planned: HashMap<MediaId, PlannedTile> = {
            let st = self.state.read();
            plan.add
                .iter()
                .filter_map(|&id| eligibility::plan_tile_for(&st, id))
                .map(|p| (p.media_id, p))
                .collect()
        };
        let mut kept: HashMap<MediaId, TileRuntime> =
            self.tiles.drain(..).map(|t| (t.media_id, t)).collect();

        let mut removed: Vec<VideoTile> = Vec::new();
        for id in &plan.remove {
            removed.extend(kept.remove(id).and_then(|t| t.video));
        }
        let mut added: Vec<usize> = Vec::new();
        for id in &plan.slots {
            if let Some(tile) = kept.remove(id) {
                self.tiles.push(tile);
            } else if let Some(p) = planned.remove(id) {
                added.push(self.tiles.len());
                let tile = self.new_tile(p);
                self.tiles.push(tile);
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
        tiles::teardown_videos(removed);
        tracing::info!(
            removed = removed_count,
            added = added.len(),
            shown = self.tiles.len(),
            players_closed = teardown_count,
            "[Multiview: Reconcile] Tiles updated to match the selection"
        );
    }
}
