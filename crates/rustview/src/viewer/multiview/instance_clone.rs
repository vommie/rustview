//! Instance behavior for showing a medium a second time: the duplicate button of the media bar.
//!
//! A duplicate is a tile of its own: its own player and window for a video (it starts from the beginning and, by the sound
//! policy, muted), the same decoded pixels for an image (the buffer is shared, nothing is decoded twice). It takes the slot
//! right next to its source and joins through `apply_follow` like every other change of the tile set, so the arrangement,
//! the sound rules, the player flags and the redraw are not repeated here. It can be made sticky and removed like any tile.

use super::follow::{self, DuplicateRefusal, TileKey};
use super::instance::MultiviewInstance;
use super::media_actions::MediaActionId;
use super::tile::TileSeed;
use crate::ui::button_feedback::FeedbackKind;
use rustview_core::settings::schema::MULTIVIEW_MAX_TILES;

impl MultiviewInstance {
    /// What a duplicate of tile `index` starts from. A sticky tile has a sticky duplicate (it is meant to stay while the
    /// selection changes, and a copy that vanished at the next click would not be what was asked for).
    fn duplicate_seed(&self, index: usize) -> TileSeed {
        let tile = &self.tiles[index];
        TileSeed {
            media_id: tile.media_id,
            kind: tile.kind,
            aspect: tile.aspect,
            path: tile.path.clone(),
            filename: tile.filename.clone(),
            pixels: tile.pixels.clone().filter(|_| !tile.is_loading),
            orig_size: tile.zoom.as_ref().map(|zoom| zoom.image()),
            sticky: tile.sticky,
        }
    }

    /// Shows the medium of tile `key` a second time. Returns whether a tile was added; a refusal (view full, tile broken,
    /// stale click) changes nothing and makes the button flash red.
    pub fn duplicate_tile(&mut self, key: TileKey) -> bool {
        let Some(index) = self.tile_index(key) else {
            return false;
        };
        let broken = self.tiles[index].error.is_some();
        let plan = match follow::duplicate(&self.shown(), key, broken, MULTIVIEW_MAX_TILES) {
            Ok(plan) => plan,
            Err(refusal) => {
                tracing::info!(
                    media_id = self.tiles[index].media_id,
                    ?refusal,
                    "[Multiview: Duplicate] Refused"
                );
                if refusal != DuplicateRefusal::UnknownTile {
                    self.tiles[index]
                        .feedback
                        .entry(MediaActionId::Duplicate)
                        .or_default()
                        .trigger(FeedbackKind::Error);
                    self.bump();
                }
                return false;
            }
        };
        let seed = self.duplicate_seed(index);
        let media_id = seed.media_id;
        self.apply_follow(&plan, vec![Some(seed)]);
        tracing::info!(
            media_id,
            shown = self.tiles.len(),
            "[Multiview: Duplicate] The medium is shown a second time"
        );
        true
    }
}
