//! Instance behavior around ratings given with the keyboard: which media a rating reaches and the feedback on the tiles.
//!
//! A rating applies to every medium the Multiview shows, sticky tiles and tiles that are not selected in the Browse view
//! included; a medium shown by several tiles is rated once, but each of those tiles shows the feedback. The icon of an image
//! tile is drawn by the host (`TileView::rating`); a video is drawn over by its native window, so its icon is an overlay of
//! mpv itself (`rating_overlay`), which also works without a compositor and on Wayland.

use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::model::TileKind;
use super::rating_feedback::{ICON_EDGE, PulseView};
use super::rating_overlay::{self, OverlayState};
use super::route::HostMode;
use rustview_core::library::models::MediaId;
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use std::collections::HashSet;
use std::time::{Duration, Instant};

impl MultiviewInstance {
    /// The media a rating reaches: every medium shown, each once, in slot order.
    pub fn rating_targets(&self) -> Vec<MediaId> {
        self.member_ids()
    }

    /// A gutter is being dragged (a rating is ignored meanwhile).
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The keys of the tiles whose medium is in `media`.
    pub fn tiles_of_media(&self, media: &HashSet<MediaId>) -> Vec<TileKey> {
        self.tiles
            .iter()
            .filter(|t| media.contains(&t.media_id))
            .map(|t| t.key)
            .collect()
    }

    /// `media` were rated `rating`: every tile that shows one of them starts its feedback (a running one is cancelled at once).
    /// Returns whether any tile did.
    pub fn show_rating_feedback(&mut self, media: &HashSet<MediaId>, rating: u8, now: Instant) -> bool {
        let tiles = self.tiles_of_media(media);
        if tiles.is_empty() {
            return false;
        }
        tracing::debug!(
            instance = self.id,
            tiles = tiles.len(),
            rating,
            "[Multiview: Rating] Feedback started"
        );
        self.rating_feedback.trigger(tiles, rating, now);
        self.sync_rating_overlays(now);
        self.bump();
        true
    }

    /// What tile `key` shows of the feedback at `now`.
    pub(super) fn rating_pulse(&self, key: TileKey, now: Instant) -> Option<PulseView> {
        self.rating_feedback.view(key, now)
    }

    /// Advances the feedback: the pulses that are over go, the overlays of the videos follow. Returns whether the picture
    /// changed (a fade is always a change).
    pub fn poll_rating_feedback(&mut self, now: Instant) -> bool {
        if !self.rating_feedback.is_active() && !self.has_rating_overlay() {
            return false;
        }
        self.rating_feedback.expire(now);
        self.sync_rating_overlays(now);
        self.bump();
        true
    }

    /// How long a host may sleep before it has to call `poll_rating_feedback`; `None` while no feedback is shown.
    pub fn rating_wakeup(&self, now: Instant) -> Option<Duration> {
        self.rating_feedback.wakeup(now)
    }

    /// Ends all feedback at once (the Multiview is not shown any more, or its tiles changed).
    pub fn clear_rating_feedback(&mut self) {
        if !self.rating_feedback.is_active() && !self.has_rating_overlay() {
            return;
        }
        self.rating_feedback.clear();
        self.sync_rating_overlays(Instant::now());
        self.bump();
    }

    fn has_rating_overlay(&self) -> bool {
        self.tiles
            .iter()
            .any(|t| t.video.as_ref().is_some_and(|v| v.rating_overlay.is_some()))
    }

    /// Brings the overlays of the video windows in line with the feedback: shows, updates (fade, new rating, new size of the
    /// window) or removes them. Costs nothing while no feedback runs and no overlay is up.
    pub(super) fn sync_rating_overlays(&mut self, now: Instant) {
        self.rating_feedback
            .retain_tiles(|key| self.tiles.iter().any(|t| t.key == key));
        if !self.rating_feedback.is_active() && !self.has_rating_overlay() {
            return;
        }
        let shown = matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView);
        let dpi = self.host.surface.dpi;
        let edge = (f64::from(ICON_EDGE) * f64::from(dpi)).round().to_int::<u32>().max(8);
        let projection = self.host.projection;
        for (tile, placed) in self.tiles.iter_mut().zip(&self.layout.tiles) {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            let pulse = if shown && tile.kind == TileKind::Video {
                self.rating_feedback.view(tile.key, now)
            } else {
                None
            };
            let Rect { w, h, .. } = projection.rect(placed.content);
            let wanted =
                pulse.map(|p| OverlayState::new(p, edge, (w.saturating_as::<u32>(), h.saturating_as::<u32>())));
            if video.rating_overlay == wanted {
                continue;
            }
            if let Some(state) = wanted {
                // When mpv does not take it (no picture yet) the next frame tries again.
                video.rating_overlay = rating_overlay::show(&video.player, &state).then_some(state);
            } else {
                rating_overlay::hide(&video.player);
                video.rating_overlay = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::instance::WeakInstance;
    use crate::viewer::multiview::rating_feedback::{FADE, HOLD};
    use crate::viewer::multiview::tile::TileSeed;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
    }

    fn seed(media: MediaId, kind: TileKind) -> TileSeed {
        TileSeed::new(
            media,
            kind,
            1.5,
            PathBuf::from(format!("/nowhere/{media}")),
            format!("m{media}"),
        )
    }

    /// Tiles for media 1, 2, 2 (a duplicate of 2), 3 (a video); sticky/selection state is irrelevant to a rating.
    fn instance() -> crate::viewer::multiview::instance::SharedInstance {
        MultiviewInstance::create(
            Arc::new(NullSink),
            vec![
                seed(1, TileKind::Image),
                seed(2, TileKind::Image),
                seed(2, TileKind::Image),
                seed(3, TileKind::Video),
            ],
        )
    }

    #[test]
    fn every_shown_medium_is_a_target_once_whatever_the_browse_selection_is() {
        let shared = instance();
        assert_eq!(shared.lock().rating_targets(), vec![1, 2, 3]);
    }

    #[test]
    fn a_duplicated_medium_is_rated_once_but_both_tiles_show_the_feedback() {
        let shared = instance();
        let mut mv = shared.lock();
        let keys: Vec<TileKey> = (0..4).filter_map(|i| mv.tile_key_at(i)).collect();
        let now = Instant::now();
        assert!(mv.show_rating_feedback(&HashSet::from([2]), 4, now));
        assert!(mv.rating_pulse(keys[0], now).is_none());
        assert_eq!(mv.rating_pulse(keys[1], now).map(|p| p.rating), Some(4));
        assert_eq!(mv.rating_pulse(keys[2], now).map(|p| p.rating), Some(4));
        assert!(mv.rating_pulse(keys[3], now).is_none());
    }

    #[test]
    fn media_that_were_not_rated_start_nothing() {
        let shared = instance();
        let mut mv = shared.lock();
        assert!(!mv.show_rating_feedback(&HashSet::from([99]), 3, Instant::now()));
        assert!(mv.rating_wakeup(Instant::now()).is_none());
    }

    #[test]
    fn a_new_rating_replaces_the_running_one_on_every_tile() {
        let shared = instance();
        let mut mv = shared.lock();
        let t0 = Instant::now();
        let all = HashSet::from([1, 2, 3]);
        mv.show_rating_feedback(&all, 1, t0);
        let later = t0 + HOLD + FADE / 2;
        assert!(mv.rating_pulse(mv.tile_key_at(0).unwrap(), later).unwrap().alpha < 1.0);
        mv.show_rating_feedback(&all, 5, later);
        for i in 0..4 {
            let pulse = mv.rating_pulse(mv.tile_key_at(i).unwrap(), later).unwrap();
            assert_eq!((pulse.rating, pulse.alpha), (5, 1.0));
        }
    }

    #[test]
    fn the_feedback_ends_by_itself_and_clearing_ends_it_at_once() {
        let shared = instance();
        let mut mv = shared.lock();
        let t0 = Instant::now();
        mv.show_rating_feedback(&HashSet::from([1]), 2, t0);
        assert!(mv.rating_wakeup(t0).is_some());
        assert!(mv.poll_rating_feedback(t0 + HOLD + FADE));
        assert!(mv.rating_wakeup(t0 + HOLD + FADE).is_none());
        assert!(!mv.poll_rating_feedback(t0 + HOLD + FADE));
        mv.show_rating_feedback(&HashSet::from([1, 2, 3]), 0, t0);
        mv.clear_rating_feedback();
        assert!(mv.rating_wakeup(t0).is_none());
    }

    #[test]
    fn deleting_media_while_the_feedback_runs_drops_only_their_pulses() {
        let shared = instance();
        let mut mv = shared.lock();
        let t0 = Instant::now();
        mv.show_rating_feedback(&HashSet::from([1, 2, 3]), 3, t0);
        let keys: Vec<TileKey> = (0..4).filter_map(|i| mv.tile_key_at(i)).collect();
        // medium 2 is deleted: both of its tiles leave and the others are laid out again
        mv.drop_deleted(&[2]);
        assert_eq!(mv.tile_count(), 2);
        let mut left: Vec<TileKey> = mv.rating_feedback.tiles().collect();
        left.sort_unstable();
        assert_eq!(left, vec![keys[0], keys[3]]);
        assert_eq!(mv.rating_pulse(keys[0], t0).map(|p| p.rating), Some(3));
        assert!(mv.rating_wakeup(t0).is_some());
    }

    #[test]
    fn switching_the_only_medium_ends_the_old_feedback_and_starts_none_on_the_new_one() {
        let shared = MultiviewInstance::create(Arc::new(NullSink), vec![seed(1, TileKind::Image)]);
        let mut mv = shared.lock();
        let t0 = Instant::now();
        mv.show_rating_feedback(&HashSet::from([1]), 5, t0);
        let old = mv.tile_key_at(0).unwrap();
        assert!(mv.replace_solo(seed(2, TileKind::Image)));
        let new = mv.tile_key_at(0).unwrap();
        assert_ne!(old, new);
        assert!(mv.rating_pulse(old, t0).is_none());
        assert!(mv.rating_pulse(new, t0).is_none());
        assert!(!mv.rating_feedback.is_active());
        assert!(mv.rating_wakeup(t0).is_none());
    }

    #[test]
    fn a_gutter_drag_is_reported_so_that_a_rating_can_be_ignored() {
        let shared = instance();
        assert!(!shared.lock().is_dragging());
    }
}
