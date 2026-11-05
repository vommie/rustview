//! The video filters of the tiles (`video_filter`): a change from a menu or a key reaches the tile's player as the mpv
//! settings that differ; a rotation also lays the tile out anew (its displayed size swaps). Filters live as long as the
//! tile shows its medium: a duplicate takes them over, a tile that takes over another medium's player starts without.

use super::instance::MultiviewInstance;
use super::video_filter::{FilterChange, VideoFilters, mpv};
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
            active = filters.active_count(),
            "[Multiview: Filters] Changed"
        );
        self.set_filters(index, filters)
    }

    /// Gives tile `index` the filters `filters`: its player gets what differs. Returns whether anything changed.
    pub(super) fn set_filters(&mut self, index: usize, filters: VideoFilters) -> bool {
        let Some(tile) = self.tiles.get_mut(index) else {
            return false;
        };
        let before = tile.filters;
        if before == filters {
            return false;
        }
        tile.filters = filters;
        if let Some(video) = tile.video.as_ref() {
            video.player.apply_filter_settings(&mpv::changes(&before, &filters));
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

    /// The player of tile `index` was just made: it gets the filters the tile has already (a duplicate's).
    pub(super) fn apply_filters_to_new_player(&self, index: usize) {
        let Some(tile) = self.tiles.get(index) else {
            return;
        };
        if let Some(video) = tile.video.as_ref()
            && !tile.filters.is_default()
        {
            video
                .player
                .apply_filter_settings(&mpv::changes(&VideoFilters::default(), &tile.filters));
            tracing::debug!(
                media_id = tile.media_id,
                active = tile.filters.active_count(),
                "[Multiview: Filters] Taken over by the new player"
            );
        }
    }

    /// The tile `index` took over a player for another medium: the filters of the old one go.
    pub(super) fn drop_filters_for_new_medium(&mut self, index: usize) {
        if self.tiles.get(index).is_some_and(|tile| !tile.filters.is_default()) {
            self.set_filters(index, VideoFilters::default());
            tracing::debug!(index, "[Multiview: Filters] Dropped for the new medium");
        }
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
