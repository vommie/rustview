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
            match wanted {
                Some(state) => {
                    if rating_overlay::show(&video.player, &state) {
                        video.rating_overlay = Some(state);
                    } else {
                        // Not shown (no picture yet): retried by the next frame.
                        video.rating_overlay = None;
                    }
                }
                None => {
                    rating_overlay::hide(&video.player);
                    video.rating_overlay = None;
                }
            }
        }
    }
}
