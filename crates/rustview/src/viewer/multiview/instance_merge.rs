//! Instance behavior for merging tabs: a Multiview gives all its tiles away, another one takes them over.
//!
//! A tile keeps everything that is running when it moves: the player and its X11 window (a video, no restart, the player is
//! pointed at its new owner), the decoded pixels (an image), the user's pause and mute choices, the feedback state of its buttons.
//! What belonged to the old arrangement is dropped: the widths the user dragged, the zoom of an image (it shows the fitted
//! picture again; a render that was under way reports to the old owner and would be lost) and the rating feedback.
//!
//! The windows are never destroyed and no player is shut down: `release_tiles` only hides and pauses them like a tab that
//! stops being shown, and the first host of the target places and resumes them.

use super::audio;
use super::container::Container;
use super::instance::MultiviewInstance;
use super::layout::Layout;
use super::prefs;
use super::route::HostMode;
use super::tile::TileState;
use crate::viewer::zoom::state::ZoomState;

/// The tiles of a Multiview that gave them away, with the display it was laid out for.
pub struct ReleasedTiles {
    pub tiles: Vec<TileState>,
    pub container: Container,
}

impl MultiviewInstance {
    /// Hands over every tile (windows hidden, players paused, nothing destroyed). The Multiview is empty afterwards and only
    /// good for being dropped, which tears down nothing as it owns no player any more.
    pub fn release_tiles(&mut self) -> ReleasedTiles {
        self.deactivate();
        self.clear_hover();
        self.drag = None;
        self.row_pins = None;
        self.zoom_slider.clear();
        self.zoom_minimap_bubble.clear();
        self.rename_target = None;
        self.rename_pinned = false;
        self.last_hovered = None;
        let tiles = std::mem::take(&mut self.tiles);
        self.layout = Layout::default();
        self.bump();
        tracing::info!(
            instance = self.id,
            tiles = tiles.len(),
            players = tiles.iter().filter(|t| t.video.is_some()).count(),
            "[Multiview: Merge] Tiles handed over"
        );
        ReleasedTiles {
            tiles,
            container: self.container,
        }
    }

    /// Takes over the tiles of other Multiviews, in the order given, after the tiles it has. The arrangement is automatic
    /// (pinned widths are gone), the sound rules decide who is audible, and a tile that still has to be started (a tab that was
    /// never shown) is started by the first host. Nothing is laid out while no host shows this Multiview: `set_host` does.
    pub fn adopt_tiles(&mut self, tiles: Vec<TileState>, container: Container) {
        if tiles.is_empty() {
            return;
        }
        if self.tiles.is_empty() {
            self.container = container;
        }
        let count = tiles.len();
        let me = self.me.clone();
        for mut tile in tiles {
            tile.pin = None;
            tile.zoom = tile.zoom.take().map(|zoom| ZoomState::new(zoom.image()));
            tile.zoom_minimap_shown = false;
            if let Some(video) = tile.video.as_ref() {
                *video.owner.write() = me.clone();
            }
            self.tiles.push(tile);
        }
        self.row_pins = None;

        let mut slots: Vec<_> = self.tiles.iter().map(|t| t.audio).collect();
        audio::after_merge(&mut slots, &prefs::current().audio);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }
        self.sound_assigned = true;

        if (0..self.tiles.len()).any(|index| self.needs_start(index)) {
            self.started = false;
        }
        if self.host.surface.mode != HostMode::Hidden {
            self.apply_layout();
            if !self.started {
                self.start_all();
            }
        }
        self.apply_audio();
        self.bump();
        tracing::info!(
            instance = self.id,
            adopted = count,
            shown = self.tiles.len(),
            "[Multiview: Merge] Tiles taken over"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::follow::TileKey;
    use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::tile::TileSeed;
    use crate::viewer::multiview::{HostSurface, layout::Rect};
    use slint::{Rgb8Pixel, SharedPixelBuffer};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
    }

    fn seed(media: u64, kind: TileKind) -> TileSeed {
        TileSeed::new(
            media,
            kind,
            1.5,
            PathBuf::from(format!("/nowhere/{media}")),
            format!("m{media}"),
        )
    }

    fn decoded_image(media: u64) -> TileSeed {
        TileSeed {
            pixels: Some(SharedPixelBuffer::<Rgb8Pixel>::new(8, 6)),
            orig_size: Some((800, 600)),
            ..seed(media, TileKind::Image)
        }
    }

    fn instance(seeds: Vec<TileSeed>) -> SharedInstance {
        MultiviewInstance::create(Arc::new(NullSink), seeds)
    }

    fn keys(mv: &MultiviewInstance) -> Vec<TileKey> {
        (0..mv.tile_count()).filter_map(|i| mv.tile_key_at(i)).collect()
    }

    #[test]
    fn releasing_hands_over_every_tile_and_leaves_an_empty_multiview() {
        let source = instance(vec![seed(1, TileKind::Image), seed(2, TileKind::Video)]);
        let before = keys(&source.lock());
        let released = source.lock().release_tiles();
        let after: Vec<TileKey> = released.tiles.iter().map(|t| t.key).collect();
        assert_eq!(after, before, "the tiles keep their identity");
        assert_eq!(source.lock().tile_count(), 0);
        assert_eq!(source.lock().member_ids(), Vec::<u64>::new());
    }

    #[test]
    fn adopting_appends_the_tiles_in_the_order_given_and_keeps_their_keys() {
        let a = instance(vec![seed(1, TileKind::Image)]);
        let b = instance(vec![seed(2, TileKind::Image), seed(3, TileKind::Image)]);
        let target = instance(Vec::new());
        let mut moved = Vec::new();
        let mut container = None;
        let mut expected: Vec<TileKey> = Vec::new();
        for source in [&a, &b] {
            expected.extend(keys(&source.lock()));
            let released = source.lock().release_tiles();
            container.get_or_insert(released.container);
            moved.extend(released.tiles);
        }
        target.lock().adopt_tiles(moved, container.unwrap());
        let mv = target.lock();
        assert_eq!(keys(&mv), expected);
        assert_eq!(mv.member_ids(), vec![1, 2, 3]);
    }

    #[test]
    fn a_medium_in_two_tabs_stays_two_tiles() {
        let a = instance(vec![seed(7, TileKind::Image)]);
        let b = instance(vec![seed(7, TileKind::Image)]);
        let target = instance(Vec::new());
        let mut moved = a.lock().release_tiles().tiles;
        let second = b.lock().release_tiles();
        let container = second.container;
        moved.extend(second.tiles);
        target.lock().adopt_tiles(moved, container);
        let mv = target.lock();
        assert_eq!(mv.tile_count(), 2);
        assert_eq!(mv.member_ids(), vec![7], "one member, two tiles");
        assert_ne!(mv.tile_key_at(0), mv.tile_key_at(1));
    }

    #[test]
    fn the_decoded_picture_moves_along_and_the_zoom_starts_at_the_fit() {
        let source = instance(vec![decoded_image(1)]);
        let pixels_before = source.lock().tiles[0].pixels.clone();
        let released = source.lock().release_tiles();
        let target = instance(Vec::new());
        target.lock().adopt_tiles(released.tiles, released.container);
        let mv = target.lock();
        let tile = &mv.tiles[0];
        assert!(!tile.is_loading, "nothing has to be decoded again");
        assert_eq!(
            tile.pixels.as_ref().map(|p| p.as_bytes().as_ptr()),
            pixels_before.as_ref().map(|p| p.as_bytes().as_ptr())
        );
        let zoom = tile.zoom.as_ref().expect("the zoom exists");
        assert_eq!(zoom.image(), (800, 600));
        assert!(!zoom.is_active(), "zoom 1.0 = fitted");
    }

    #[test]
    fn widths_the_user_dragged_do_not_survive_the_merge() {
        let source = instance(vec![seed(1, TileKind::Image), seed(2, TileKind::Image)]);
        {
            let mut mv = source.lock();
            mv.tiles[0].pin = Some(0.7);
            mv.row_pins = Some([0.5, 0.5]);
        }
        let released = source.lock().release_tiles();
        assert!(source.lock().row_pins.is_none());
        let target = instance(Vec::new());
        target.lock().adopt_tiles(released.tiles, released.container);
        let mv = target.lock();
        assert!(mv.tiles.iter().all(|t| t.pin.is_none()));
        assert!(mv.row_pins.is_none());
    }

    #[test]
    fn tiles_that_never_ran_are_started_by_the_first_host_and_the_sound_rules_are_not_applied_twice() {
        let source = instance(vec![seed(1, TileKind::Video), seed(2, TileKind::Image)]);
        let released = source.lock().release_tiles();
        let target = instance(Vec::new());
        target.lock().adopt_tiles(released.tiles, released.container);
        let mv = target.lock();
        assert!(!mv.is_started(), "the first host starts what is not running");
        assert!(mv.sound_assigned, "the merge applied the sound rules already");
        assert!(mv.needs_start(0) && mv.needs_start(1));
    }

    #[test]
    fn the_first_host_lays_the_merged_tiles_out_and_starts_the_images() {
        let a = instance(vec![decoded_image(1)]);
        let b = instance(vec![decoded_image(2)]);
        let target = instance(Vec::new());
        let mut moved = a.lock().release_tiles().tiles;
        let second = b.lock().release_tiles();
        moved.extend(second.tiles);
        target.lock().adopt_tiles(moved, second.container);
        let surface = HostSurface::tab_view(0, Rect::new(0, 0, 1280, 720), 1.0, Vec::new());
        assert!(target.lock().set_host(&surface));
        let mv = target.lock();
        assert_eq!(mv.layout.tiles.len(), 2);
        assert!(mv.is_started());
        assert!(
            !mv.needs_start(0) && !mv.needs_start(1),
            "decoded images have nothing to start"
        );
    }

    #[test]
    fn adopting_nothing_changes_nothing() {
        let target = instance(vec![seed(1, TileKind::Image)]);
        let revision = target.lock().revision();
        let container = target.lock().container();
        target.lock().adopt_tiles(Vec::new(), container);
        assert_eq!(target.lock().revision(), revision);
        assert_eq!(target.lock().tile_count(), 1);
    }
}
