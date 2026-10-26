//! The feedback pulse of a Multiview (`media_feedback`): the one animation every interaction with a medium uses (a rating
//! given with the keyboard, a saved picture, the mute key; the volume bar keeps the same time, `instance_volume_feedback`). The
//! canvas of the host draws the icon over every tile, picture or video (`TileView::feedback`). The host arms one single-shot
//! timer from `feedback_wakeup` while a pulse is shown.

use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::media_feedback::{FeedbackIcon, PulseView};
use std::time::{Duration, Instant};

impl MultiviewInstance {
    /// Starts a pulse with `icon` on the tiles `tiles` (a running one is replaced at once). The host draws it and arms its timer
    /// (`MultiviewHandle::feedback_started`).
    pub fn show_feedback(&mut self, tiles: impl IntoIterator<Item = TileKey>, icon: FeedbackIcon, now: Instant) {
        self.feedback.trigger(tiles, icon, now);
        self.feedback_requested = true;
        self.prune_feedback();
        self.bump();
    }

    /// What tile `key` shows of the feedback at `now`.
    pub(super) fn feedback_pulse(&self, key: TileKey, now: Instant) -> Option<PulseView> {
        self.feedback.view(key, now)
    }

    /// Advances the feedback: the pulses that are over go. Returns whether the picture changed (a fade is always a change).
    pub fn poll_feedback(&mut self, now: Instant) -> bool {
        if !self.feedback.is_active() && !self.volume_feedback.is_active() {
            return false;
        }
        self.feedback.expire(now);
        self.volume_feedback.expire(now);
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
        if !self.feedback.is_active() && !self.volume_feedback.is_active() {
            return;
        }
        self.feedback.clear();
        self.volume_feedback.clear();
        self.bump();
    }

    /// Drops the pulses and volume bars of tiles that left.
    pub(super) fn prune_feedback(&mut self) {
        let keys: Vec<TileKey> = self.tiles.iter().map(|t| t.key).collect();
        self.feedback.retain_tiles(|key| keys.contains(&key));
        self.volume_feedback.retain_tiles(|key| keys.contains(&key));
    }
}
