//! Instance behavior around library events: a member was renamed, deleted or refreshed. The application tells the
//! instance from the records it already holds in memory; the drive is never touched.

use super::audio::{self, TileAudio};
use super::prefs;
use super::instance::MultiviewInstance;
use super::model::{AspectSource, aspect_differs, aspect_from_size};
use super::teardown::teardown_videos;
use crate::library::models::{MediaId, MediaRecord};
use crate::settings::LogObscurer;

impl MultiviewInstance {
    pub fn member_renamed(&mut self, id: MediaId, new_filename: &str) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.media_id == id) else {
            return;
        };
        if let Some(parent) = tile.path.parent() {
            tile.path = parent.join(new_filename);
        }
        tile.filename = new_filename.to_string();
        tracing::debug!(
            media_id = id,
            filename = %LogObscurer::filename(new_filename),
            "[Multiview: Members] Member renamed"
        );
    }

    /// The file of a member is gone: its player is shut down, its tile shows an error and the sound moves on if it
    /// carried it.
    pub fn member_deleted(&mut self, id: MediaId) {
        let Some(index) = self.tiles.iter().position(|t| t.media_id == id) else {
            return;
        };
        let video = {
            let tile = &mut self.tiles[index];
            tile.error = Some(format!(
                "Media file not found on disk:\n{}",
                tile.path.display()
            ));
            tile.is_loading = false;
            tile.pixels = None;
            tile.audio = None;
            tile.video.take()
        };
        teardown_videos(video.into_iter().collect());
        self.clear_hover();
        let mut slots: Vec<Option<TileAudio>> = self.tiles.iter().map(|t| t.audio).collect();
        audio::after_reconcile(&mut slots, &[], &prefs::current().audio);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }
        self.apply_audio();
        self.bump();
        tracing::info!(
            media_id = id,
            "[Multiview: Members] Member deleted; its tile shows an error"
        );
    }

    /// A member's record changed (refresh): keeps the name and, while no player or decoder has measured the real size
    /// yet, the aspect ratio in line with the record.
    pub fn member_refreshed(&mut self, id: MediaId, record: &MediaRecord) {
        if record.is_deleted {
            self.member_deleted(id);
            return;
        }
        let Some(tile) = self.tiles.iter_mut().find(|t| t.media_id == id) else {
            return;
        };
        tile.filename = record.filename.clone();
        let mut relayout = false;
        if tile.aspect_source == AspectSource::Library {
            let aspect = aspect_from_size(record.width, record.height, tile.aspect);
            if aspect_differs(aspect, tile.aspect) {
                tile.aspect = aspect;
                relayout = true;
            }
        }
        if relayout {
            self.apply_layout();
        }
    }
}
