//! Instance behavior around the tiles: creating and starting them and decoding images.

use super::audio::{self, TileAudio};
use super::events::{DecodedImage, PlayerEvent};
use super::follow::TileKey;
use super::image_decode;
use super::instance::MultiviewInstance;
use super::model::{AspectSource, TileKind, aspect_differs, aspect_from_size};
use super::prefs;
use super::tile;
use crate::viewer::edit::tile_edit::{EditSource, TileEdit};
use crate::viewer::zoom::state::ZoomState;
use rustview_core::image_edit::conflict::FileIdentity;
use rustview_core::image_edit::probe;
use rustview_core::library::models::MediaId;
use rustview_core::media::MediaFormat;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::SaturatingAs;
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
    /// Whether tile `index` still has to be started: a video without a player, an image whose decoder has not delivered. A tile
    /// that came running from another Multiview (a merge of tabs) needs nothing.
    pub(super) fn needs_start(&self, index: usize) -> bool {
        self.tiles.get(index).is_some_and(|t| {
            t.error.is_none()
                && match t.kind {
                    TileKind::Video => t.video.is_none(),
                    TileKind::Image => t.is_loading,
                }
        })
    }

    /// Starts every tile that is not running once the host exists: the leftmost video gets the sound, the others are muted by
    /// policy (unless a merge of tabs applied the rules to the tiles already). Does nothing (and returns `false`) when the tiles
    /// were started before.
    pub fn start_all(&mut self) -> bool {
        if self.started {
            return false;
        }
        self.started = true;
        if !self.sound_assigned {
            self.assign_audio_on_open();
        }
        for index in 0..self.tiles.len() {
            if self.needs_start(index) {
                self.start_tile(index);
            }
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
        true
    }

    fn assign_audio_on_open(&mut self) {
        let mut slots: Vec<Option<TileAudio>> = self.tiles.iter().map(|t| t.audio).collect();
        audio::assign_on_open(&mut slots, &prefs::current().audio);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }
    }

    pub(super) fn start_tile(&mut self, index: usize) {
        match self.tiles.get(index).map(|t| (t.kind, t.video.is_some(), t.is_loading)) {
            Some((TileKind::Video, true, _)) => self.reload_video(index),
            Some((TileKind::Video, false, _)) => self.start_video(index),
            // An image that came with its pixels (a duplicate) has nothing to decode.
            Some((TileKind::Image, _, true)) => self.start_image(index),
            Some((TileKind::Image, _, false)) | None => {}
        }
    }

    /// A video tile took over the player of the tile it replaced: the player plays the new file on its existing surface.
    fn reload_video(&mut self, index: usize) {
        let (media_id, key, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.key, tile.path.clone())
        };
        let Some(video) = self.tiles[index].video.as_ref() else {
            return;
        };
        tile::reload_video(video, key, &path);
        self.drop_filters_for_new_medium(index);
        // The zoom belonged to the file before; the new one gets its own once probed. A failure is tried again.
        self.tiles[index].zoom = None;
        self.tiles[index].error = None;
        tracing::info!(
            media_id,
            index,
            filename = %LogObscurer::filename(&self.tiles[index].filename),
            "[Multiview: Tile] Video tile recycled the player of the tile it replaced"
        );
    }

    fn start_video(&mut self, index: usize) {
        let (media_id, key, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.key, tile.path.clone())
        };
        let created = tile::create_video(&self.sink, &self.me, self.host.surface.window, key, &path);
        match created {
            Ok(video) => {
                video.player.set_background_color(self.effective_video_background());
                let tile = &mut self.tiles[index];
                tile.video = Some(video);
                self.apply_filters_to_new_player(index);
                let tile = &mut self.tiles[index];
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
        let (media_id, key, path) = {
            let tile = &self.tiles[index];
            (tile.media_id, tile.key, tile.path.clone())
        };
        // Large enough for the display and for the area of the tab view (a lone image fills all of it).
        let area = self.host.surface.rect;
        let (max_w, max_h) = (
            self.container.width.max(area.w).max(1).saturating_as::<u32>(),
            self.container.height.max(area.h).max(1).saturating_as::<u32>(),
        );
        let (sink, me) = (self.sink.clone(), self.me.clone());
        let spawned = std::thread::Builder::new()
            .name("multiview-image".into())
            .spawn(move || {
                // The identity is taken before the pixels are read: a change while decoding shows up when saving.
                let identity = FileIdentity::read(&path).ok();
                let result: DecodedImage = image_decode::load_scaled(&path, max_w, max_h);
                let source = result.as_ref().ok().map(|(w, h, _)| {
                    let format = MediaFormat::from_path(&path);
                    EditSource {
                        identity,
                        probe: probe::probe(&path, format).map_err(|e| e.to_string()),
                        format,
                        size: (*w, *h),
                    }
                });
                sink.post(
                    me,
                    PlayerEvent::ImageDecoded {
                        tile: key,
                        result,
                        source,
                    },
                );
            });
        if let Err(e) = spawned {
            tracing::error!(media_id, error = %e, "[Multiview: Tile] Could not start the image decoder thread");
            self.on_image_loaded(key, Err("This image could not be loaded".to_string()), None);
        }
    }

    /// Starts the decoder of every image tile that has neither pixels nor an error yet (a tab that was shown in the fullscreen
    /// while its image was still loading).
    pub fn start_pending_images(&mut self) {
        for index in 0..self.tiles.len() {
            let tile = &self.tiles[index];
            if tile.kind == TileKind::Image && tile.is_loading && tile.pixels.is_none() {
                self.start_tile(index);
            }
        }
    }

    /// The file of a member was refreshed and may have changed: an image is decoded again (the old picture stays until the new one
    /// arrives), a running video loads its file again. A tile that was never started stays as it is.
    pub fn reload_member(&mut self, id: MediaId) {
        if !self.started {
            return;
        }
        for index in 0..self.tiles.len() {
            if self.tiles[index].media_id != id {
                continue;
            }
            // Unsaved edits are never thrown away by a refresh; saving finds out that the file changed.
            if self.tiles[index].edit.as_ref().is_some_and(TileEdit::is_live) {
                tracing::info!(
                    media_id = id,
                    "[ImageEdit: Tile] The file was refreshed while it has unsaved edits; the edits stay"
                );
                continue;
            }
            match self.tiles[index].kind {
                TileKind::Image => {
                    self.tiles[index].error = None;
                    self.start_image(index);
                }
                TileKind::Video if self.tiles[index].video.is_some() => self.reload_video(index),
                TileKind::Video => {}
            }
        }
        self.bump();
    }

    /// Shows `pixels` (a thumbnail the application already holds) in an image tile whose decode is still running, so
    /// browsing does not flash an empty tile. The decoded image replaces it.
    pub fn set_placeholder(&mut self, media_id: MediaId, pixels: &SharedPixelBuffer<Rgb8Pixel>) {
        let mut changed = false;
        for tile in self.tiles.iter_mut().filter(|t| t.media_id == media_id) {
            if tile.kind == TileKind::Image && tile.is_loading && tile.pixels.is_none() {
                tile.pixels = Some(pixels.clone());
                changed = true;
            }
        }
        if changed {
            self.bump();
        }
    }

    /// Result of a background decode. Ignored if the tile left in the meantime.
    pub fn on_image_loaded(&mut self, key: TileKey, result: DecodedImage, source: Option<EditSource>) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.key == key) else {
            return;
        };
        // A picture that is edited keeps what it shows (a decode that was already running when editing started).
        if tile.edit.as_ref().is_some_and(TileEdit::is_live) {
            tile.is_loading = false;
            return;
        }
        tile.edit = None;
        if source.is_some() {
            tile.source = source;
        }
        let media_id = tile.media_id;
        tile.is_loading = false;
        let mut relayout = false;
        match result {
            Ok((width, height, pixels)) => {
                let aspect = aspect_from_size(width, height, tile.aspect);
                relayout = aspect_differs(aspect, tile.aspect);
                tile.aspect = aspect;
                tile.aspect_source = AspectSource::Probed;
                tile.pixels = Some(pixels);
                tile.zoom = Some(ZoomState::new((width, height)));
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
            // A new picture has a new zoom; it needs the size of its tile.
            self.sync_zoom_frames();
            self.bump();
        }
    }
}
