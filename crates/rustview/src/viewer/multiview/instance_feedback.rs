//! The feedback pulse of a Multiview (`media_feedback`): the one animation every interaction with a medium uses (a rating
//! given with the keyboard, a saved picture, the mute key; the volume bar keeps the same time, `instance_volume_feedback`). The
//! icon of an image tile is drawn by the host (`TileView::feedback`); a video is drawn over by its native window, so its icon
//! is an overlay of mpv itself (`feedback_overlay`), which also works without a compositor and on Wayland. The host arms one
//! single-shot timer from `feedback_wakeup` while a pulse is shown.

use super::feedback_overlay::{self, OverlayState};
use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::media_feedback::{FeedbackIcon, ICON_EDGE, PulseView};
use super::model::TileKind;
use super::route::HostMode;
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use std::time::{Duration, Instant};

impl MultiviewInstance {
    /// Starts a pulse with `icon` on the tiles `tiles` (a running one is replaced at once). The host draws it and arms its timer
    /// (`MultiviewHandle::feedback_started`).
    pub fn show_feedback(&mut self, tiles: impl IntoIterator<Item = TileKey>, icon: FeedbackIcon, now: Instant) {
        self.feedback.trigger(tiles, icon, now);
        self.feedback_requested = true;
        self.sync_feedback_overlays(now);
        self.bump();
    }

    /// What tile `key` shows of the feedback at `now`.
    pub(super) fn feedback_pulse(&self, key: TileKey, now: Instant) -> Option<PulseView> {
        self.feedback.view(key, now)
    }

    /// Advances the feedback: the pulses that are over go, the overlays of the videos follow. Returns whether the picture
    /// changed (a fade is always a change).
    pub fn poll_feedback(&mut self, now: Instant) -> bool {
        if !self.feedback.is_active() && !self.has_feedback_overlay() && !self.has_volume_feedback() {
            return false;
        }
        self.feedback.expire(now);
        self.volume_feedback.expire(now);
        self.sync_feedback_overlays(now);
        self.bump();
        true
    }

    /// How long a host may sleep before it has to call `poll_feedback`; `None` while no feedback is shown.
    pub fn feedback_wakeup(&self, now: Instant) -> Option<Duration> {
        match (self.feedback.wakeup(now), self.volume_feedback.wakeup(now)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Whether a pulse or a volume bar was started since the last call (by a player key or the wheel, which the host does not
    /// follow itself): the host arms its feedback timer then.
    pub fn take_feedback_request(&mut self) -> bool {
        std::mem::take(&mut self.feedback_requested)
    }

    /// Ends all feedback at once (the Multiview is not shown any more, or its tiles changed).
    pub fn clear_feedback(&mut self) {
        if !self.feedback.is_active() && !self.has_feedback_overlay() && !self.has_volume_feedback() {
            return;
        }
        self.feedback.clear();
        self.volume_feedback.clear();
        self.sync_feedback_overlays(Instant::now());
        self.bump();
    }

    fn has_feedback_overlay(&self) -> bool {
        self.tiles
            .iter()
            .any(|t| t.video.as_ref().is_some_and(|v| v.feedback_overlay.is_some()))
    }

    /// Brings the overlays of the video windows in line with the feedback: shows, updates (fade, new icon, new size of the
    /// window) or removes them. Costs nothing while no feedback runs and no overlay is up.
    pub(super) fn sync_feedback_overlays(&mut self, now: Instant) {
        self.sync_volume_overlays(now);
        self.feedback
            .retain_tiles(|key| self.tiles.iter().any(|t| t.key == key));
        if !self.feedback.is_active() && !self.has_feedback_overlay() {
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
                self.feedback.view(tile.key, now)
            } else {
                None
            };
            let Rect { w, h, .. } = projection.rect(placed.content);
            let wanted =
                pulse.map(|p| OverlayState::new(p, edge, (w.saturating_as::<u32>(), h.saturating_as::<u32>())));
            if video.feedback_overlay == wanted {
                continue;
            }
            if let Some(state) = wanted {
                // When mpv does not take it (no picture yet) the next frame tries again.
                video.feedback_overlay = feedback_overlay::show(&video.player, &state).then_some(state);
            } else {
                feedback_overlay::hide(&video.player);
                video.feedback_overlay = None;
            }
        }
    }
}
