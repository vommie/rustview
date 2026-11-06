//! The video filters of the tiles (`video_filter`): a change from a menu or a key reaches the tile's player as the mpv
//! settings that differ; a rotation also lays the tile out anew (its displayed size swaps). Filters live as long as the
//! tile shows its medium: a duplicate takes them over, a tile that takes over another medium's player starts without.

use super::instance::MultiviewInstance;
use super::video_filter::mpv::{self, MpvContext};
use super::video_filter::{FilterChange, Normalize, VideoFilters, default_normalize};
use crate::viewer::multiview::follow::TileKey;

impl MultiviewInstance {
    /// Changes the filters of tile `index`. Returns whether anything changed.
    pub(super) fn change_filters(&mut self, index: usize, change: FilterChange) -> bool {
        let Some(tile) = self.tiles.get(index) else {
            return false;
        };
        let mut filters = tile.filters;
        change.apply(&mut filters);
        tracing::info!(
            media_id = tile.media_id,
            ?change,
            active = filters.active_count(default_normalize()),
            "[Multiview: Filters] Changed"
        );
        self.set_filters(index, filters)
    }

    /// What the mpv settings of tile `index` depend on besides its filters.
    fn mpv_context(&self, index: usize, default: Normalize) -> MpvContext {
        MpvContext {
            default_normalize: default,
            file_rotation: self
                .tiles
                .get(index)
                .and_then(|tile| tile.video.as_ref())
                .map_or(0, |video| video.player.file_rotation()),
        }
    }

    /// Gives tile `index` the filters `filters`: its player gets what differs. Returns whether anything changed.
    pub(super) fn set_filters(&mut self, index: usize, filters: VideoFilters) -> bool {
        let context = self.mpv_context(index, default_normalize());
        let Some(tile) = self.tiles.get_mut(index) else {
            return false;
        };
        let before = tile.filters;
        if before == filters {
            return false;
        }
        tile.filters = filters;
        if let Some(video) = tile.video.as_ref() {
            video
                .player
                .apply_filter_settings(&mpv::changes((&before, context), (&filters, context)));
        }
        if before.rotation() != filters.rotation() && tile.video.is_some() {
            // The displayed size changes: the aspect and the zoom are probed again when mpv reports its new parameters
            // (`PlayerEvent::VideoParams`; until then the old ones are stale).
            tile.aspect_source = super::model::AspectSource::Library;
            tile.zoom = None;
        }
        self.bump();
        true
    }

    /// The player of tile `index` was just made: it gets the filters the tile has already (a duplicate's) and the default
    /// normalization.
    pub(super) fn apply_filters_to_new_player(&self, index: usize) {
        let context = self.mpv_context(index, default_normalize());
        let Some(tile) = self.tiles.get(index) else {
            return;
        };
        let Some(video) = tile.video.as_ref() else {
            return;
        };
        // A new player has no filter yet (and no default normalization either).
        let fresh = MpvContext {
            default_normalize: Normalize::Off,
            ..context
        };
        let changes = mpv::changes((&VideoFilters::default(), fresh), (&tile.filters, context));
        if !changes.is_empty() {
            video.player.apply_filter_settings(&changes);
            tracing::debug!(
                media_id = tile.media_id,
                active = tile.filters.active_count(context.default_normalize),
                "[Multiview: Filters] Applied to the new player"
            );
        }
    }

    /// The tile `index` took over a player for another medium: the filters of the old one go.
    pub(super) fn drop_filters_for_new_medium(&mut self, index: usize) {
        if self
            .tiles
            .get(index)
            .is_some_and(|tile| tile.filters != VideoFilters::default())
        {
            self.set_filters(index, VideoFilters::default());
            tracing::debug!(index, "[Multiview: Filters] Dropped for the new medium");
        }
    }

    /// The default normalization of Settings > Multiview > Player changed from `before`: every video without a choice of its
    /// own plays the new one.
    pub fn default_normalize_changed(&mut self, before: Normalize) {
        let after = default_normalize();
        for index in 0..self.tiles.len() {
            let old = self.mpv_context(index, before);
            let new = MpvContext {
                default_normalize: after,
                ..old
            };
            let tile = &self.tiles[index];
            if let Some(video) = tile.video.as_ref() {
                let changes = mpv::changes((&tile.filters, old), (&tile.filters, new));
                if !changes.is_empty() {
                    video.player.apply_filter_settings(&changes);
                }
            }
        }
        self.bump();
    }

    /// The new tile `copy` shows the medium of `source` a second time: it takes over its filters (user decision 2026-11-05).
    pub(super) fn copy_filters(&mut self, source: VideoFilters, copy: TileKey) {
        if let Some(index) = self.tile_index(copy) {
            self.set_filters(index, source);
        }
    }

    /// The filters of tile `key` (a medium without filters, or no such tile: none).
    pub fn filters_of(&self, key: TileKey) -> VideoFilters {
        self.tile_index(key)
            .map(|index| self.tiles[index].filters)
            .unwrap_or_default()
    }
}
