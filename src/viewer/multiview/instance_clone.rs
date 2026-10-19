//! Instance behavior for showing a medium a second time: the duplicate button of the media bar.
//!
//! A duplicate is a tile of its own: its own player and window for a video (it starts from the beginning and, by the sound
//! policy, muted), the same decoded pixels for an image (the buffer is shared, nothing is decoded twice). It takes the slot
//! right next to its source and joins through `apply_follow` like every other change of the tile set, so the arrangement,
//! the sound rules, the player flags and the redraw are not repeated here. It can be made sticky and removed like any tile.

use super::follow::{self, DuplicateRefusal, TileKey};
use super::instance::MultiviewInstance;
use super::tile::TileSeed;
use crate::settings::schema::MULTIVIEW_MAX_TILES;
use crate::ui::button_feedback::FeedbackKind;
use super::media_actions::MediaActionId;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
    use crate::viewer::multiview::media_actions::BarClick;
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::route::{HostMode, OriginKind, UiAction};
    use slint::{Rgb8Pixel, SharedPixelBuffer};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _: WeakInstance, _: PlayerEvent) {}
    }

    fn seed(media: u64) -> TileSeed {
        TileSeed::new(media, TileKind::Image, 16.0 / 9.0, PathBuf::from(format!("/nonexistent/{media}.jpg")), format!("{media}.jpg"))
    }

    /// An instance whose host is the fullscreen (the bar only exists in an interactive host).
    fn instance(media: &[u64]) -> SharedInstance {
        let instance = MultiviewInstance::create(Arc::new(NullSink), media.iter().map(|&m| seed(m)).collect());
        instance.lock().host.surface.mode = HostMode::Fullscreen;
        instance
    }

    fn decoded(instance: &SharedInstance, index: usize) {
        let mut guard = instance.lock();
        let key = guard.tile_key_at(index).expect("tile");
        guard.on_image_loaded(key, Ok((1920, 1080, SharedPixelBuffer::<Rgb8Pixel>::new(8, 4))));
    }

    fn media(instance: &SharedInstance) -> Vec<u64> {
        instance.lock().slot_media_ids()
    }

    fn click_duplicate(instance: &SharedInstance, index: usize) -> UiAction {
        let mut guard = instance.lock();
        let key = guard.tile_key_at(index).expect("tile");
        guard.perform_click(key, BarClick::Action(MediaActionId::Duplicate))
    }

    #[test]
    fn a_lone_image_is_shown_twice_with_shared_pixels() {
        let x = instance(&[10]);
        decoded(&x, 0);
        assert_eq!(click_duplicate(&x, 0), UiAction::None);
        let guard = x.lock();
        assert_eq!(guard.slot_media_ids(), [10, 10]);
        assert_eq!(guard.member_ids(), [10]);
        let (a, b) = (&guard.tiles[0], &guard.tiles[1]);
        assert_ne!(a.key, b.key);
        assert!(!b.is_loading);
        let shared = |t: &crate::viewer::multiview::tile::TileState| t.pixels.as_ref().map(|p| p.as_bytes().as_ptr());
        assert_eq!(shared(a), shared(b), "the decoded buffer is shared, not decoded again");
        assert_eq!(guard.layout.tiles.len(), 2);
        assert_eq!(guard.row_count(), 1);
    }

    #[test]
    fn the_duplicate_sits_right_next_to_its_source() {
        let x = instance(&[1, 2, 3]);
        click_duplicate(&x, 1);
        assert_eq!(media(&x), [1, 2, 2, 3]);
    }

    #[test]
    fn four_is_the_limit_and_a_fifth_is_refused() {
        let x = instance(&[7]);
        for expected in 2..=4 {
            click_duplicate(&x, 0);
            assert_eq!(x.lock().tile_count(), expected);
        }
        click_duplicate(&x, 0);
        assert_eq!(x.lock().tile_count(), 4);
        let key = x.lock().tile_key_at(0).unwrap();
        assert!(!x.lock().duplicate_tile(key));
    }

    #[test]
    fn four_widescreen_copies_form_a_grid_and_removing_one_goes_back_to_a_row() {
        let x = instance(&[7]);
        for _ in 0..3 {
            click_duplicate(&x, 0);
        }
        assert_eq!(x.lock().row_count(), 2);
        let key = x.lock().tile_key_at(3).unwrap();
        x.lock().perform_click(key, BarClick::Action(MediaActionId::Remove));
        assert_eq!(x.lock().tile_count(), 3);
        assert_eq!(x.lock().row_count(), 1);
        assert_eq!(x.lock().layout.tiles.len(), 3);
    }

    #[test]
    fn a_sticky_tile_has_a_sticky_duplicate_and_the_width_stays_with_the_source() {
        let x = instance(&[1, 2]);
        {
            let mut guard = x.lock();
            guard.tiles[0].sticky = true;
            guard.tiles[0].pin = Some(0.7);
        }
        click_duplicate(&x, 0);
        let guard = x.lock();
        assert_eq!(guard.slot_media_ids(), [1, 1, 2]);
        assert!(guard.tiles[0].sticky && guard.tiles[1].sticky && !guard.tiles[2].sticky);
        assert_eq!(guard.tiles[0].pin, Some(0.7));
        assert_eq!(guard.tiles[1].pin, None);
    }

    #[test]
    fn removing_one_of_two_copies_keeps_the_selection_and_the_other_copy() {
        let x = instance(&[1, 2]);
        click_duplicate(&x, 0);
        let key = x.lock().tile_key_at(1).unwrap();
        let outcome = x.lock().perform_click(key, BarClick::Action(MediaActionId::Remove));
        assert_eq!(outcome, UiAction::None, "medium 1 is still shown, so it stays selected");
        assert_eq!(media(&x), [1, 2]);
    }

    #[test]
    fn removing_the_last_copy_deselects_in_browse_and_reports_to_a_tab() {
        let x = instance(&[1, 2]);
        let key = x.lock().tile_key_at(1).unwrap();
        let outcome = x.lock().perform_click(key, BarClick::Action(MediaActionId::Remove));
        assert_eq!(outcome, UiAction::DeselectInBrowse(2));

        let tab = instance(&[1, 2]);
        tab.lock().set_origin(OriginKind::Tab);
        let key = tab.lock().tile_key_at(1).unwrap();
        let outcome = tab.lock().perform_click(key, BarClick::Action(MediaActionId::Remove));
        assert_eq!(outcome, UiAction::TabMemberRemoved);
    }

    #[test]
    fn deleting_a_medium_removes_all_its_copies_and_the_last_tile_closes() {
        let x = instance(&[1, 2]);
        click_duplicate(&x, 0);
        assert_eq!(x.lock().drop_deleted(&[1]), UiAction::None);
        assert_eq!(media(&x), [2]);

        let only = instance(&[1]);
        click_duplicate(&only, 0);
        assert_eq!(only.lock().drop_deleted(&[1]), UiAction::CloseFullscreen);
        assert_eq!(only.lock().tile_count(), 2, "nothing is removed when the whole view closes");
    }

    #[test]
    fn library_events_reach_every_copy() {
        let x = instance(&[1]);
        click_duplicate(&x, 0);
        x.lock().member_renamed(1, "renamed.jpg");
        assert!(x.lock().tiles.iter().all(|t| t.filename == "renamed.jpg"));
        x.lock().member_deleted(1);
        assert!(x.lock().tiles.iter().all(|t| t.error.is_some()));
    }

    fn duplicate_view(x: &SharedInstance, index: usize) -> crate::viewer::multiview::view::MediaActionView {
        x.lock()
            .action_views(index)
            .into_iter()
            .find(|a| a.id == MediaActionId::Duplicate)
            .expect("the duplicate button is offered")
    }

    #[test]
    fn the_button_is_disabled_with_a_reason_when_the_view_is_full_or_the_tile_is_broken() {
        let x = instance(&[1]);
        let view = duplicate_view(&x, 0);
        assert!(view.enabled);
        assert!(!view.tooltip.contains("disabled"));
        for _ in 0..3 {
            click_duplicate(&x, 0);
        }
        let full = duplicate_view(&x, 0);
        assert!(!full.enabled);
        assert!(full.tooltip.contains("maximum of 4"), "{}", full.tooltip);

        let broken = instance(&[1]);
        broken.lock().host.surface.mode = HostMode::TabView;
        broken.lock().member_deleted(1);
        let view = duplicate_view(&broken, 0);
        assert!(!view.enabled);
        assert!(view.tooltip.contains("cannot be shown"), "{}", view.tooltip);
    }

    #[test]
    fn a_disabled_button_does_nothing_when_it_is_clicked() {
        let x = instance(&[1]);
        for _ in 0..3 {
            click_duplicate(&x, 0);
        }
        let keys: Vec<_> = x.lock().tiles.iter().map(|t| t.key).collect();
        click_duplicate(&x, 2);
        let after: Vec<_> = x.lock().tiles.iter().map(|t| t.key).collect();
        assert_eq!(keys, after);
    }

    #[test]
    fn the_button_is_not_offered_to_the_fullscreen_of_a_normal_tab() {
        let x = instance(&[1]);
        x.lock().set_single_tab(true);
        assert!(x.lock().action_views(0).iter().all(|a| a.id != MediaActionId::Duplicate));
    }

    #[test]
    fn a_decode_result_reaches_only_the_tile_it_was_started_for() {
        let x = instance(&[1]);
        click_duplicate(&x, 0);
        {
            let mut guard = x.lock();
            for tile in &mut guard.tiles {
                tile.is_loading = true;
                tile.pixels = None;
            }
        }
        decoded(&x, 1);
        let guard = x.lock();
        assert!(guard.tiles[0].is_loading && guard.tiles[0].pixels.is_none());
        assert!(!guard.tiles[1].is_loading && guard.tiles[1].pixels.is_some());
    }

    #[test]
    fn a_duplicate_of_a_loading_image_decodes_for_itself() {
        let x = instance(&[1]);
        click_duplicate(&x, 0);
        let guard = x.lock();
        assert!(guard.tiles[1].is_loading && guard.tiles[1].pixels.is_none());
    }

    #[test]
    fn a_placeholder_thumbnail_serves_every_loading_copy() {
        let x = instance(&[1]);
        click_duplicate(&x, 0);
        x.lock().set_placeholder(1, SharedPixelBuffer::<Rgb8Pixel>::new(2, 2));
        assert!(x.lock().tiles.iter().all(|t| t.pixels.is_some()));
    }

    #[test]
    fn the_selection_keeps_both_copies_while_their_medium_is_selected_and_drops_them_together_otherwise() {
        use crate::viewer::multiview::follow::Wanted;
        let x = instance(&[1, 2]);
        click_duplicate(&x, 0);
        let keep = x.lock().plan_follow(&[Wanted { id: 1, is_video: false }, Wanted { id: 2, is_video: false }]);
        assert!(keep.is_noop());

        let swap = x.lock().plan_follow(&[Wanted { id: 3, is_video: false }]);
        assert_eq!(swap.remove.len(), 3);
        x.lock().apply_follow(&swap, vec![Some(seed(3))]);
        assert_eq!(media(&x), [3]);
    }

    #[test]
    fn keys_are_never_reused() {
        let x = instance(&[1]);
        let keys = |x: &SharedInstance| -> Vec<u64> { x.lock().tiles.iter().map(|t| t.key).collect() };
        let mut ever: std::collections::HashSet<u64> = keys(&x).into_iter().collect();
        for _ in 0..6 {
            click_duplicate(&x, 0);
            for key in keys(&x) {
                ever.insert(key);
            }
            if x.lock().tile_count() == 4 {
                let key = x.lock().tile_key_at(3).unwrap();
                x.lock().perform_click(key, BarClick::Action(MediaActionId::Remove));
            }
        }
        let current = keys(&x);
        let distinct: std::collections::HashSet<_> = current.iter().copied().collect();
        assert_eq!(distinct.len(), current.len());
        assert!(ever.len() > current.len(), "removed tiles never give their key to a later tile");
    }
}
