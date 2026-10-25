//! The volume bar and the mute icon of a video (`volume_feedback`): the wheel and the video player keys start them, the
//! overlays of mpv draw them (`volume_overlay`, `feedback_overlay`), and they keep the time of the feedback pulse, so the host's
//! feedback timer (`feedback_wakeup`, `poll_feedback`) drives them too.

use super::instance::MultiviewInstance;
use super::media_feedback::FeedbackIcon;
use super::model::TileKind;
use super::route::HostMode;
use super::volume_feedback::{MuteInput, VolumeInput, VolumeLevel, fitted_picture, place_bar};
use super::volume_overlay::{self, VolumeOverlayState};
use crate::viewer::zoom::state::ZoomState;
use rustview_core::util::cast::SaturatingAs;
use std::time::Instant;

impl MultiviewInstance {
    /// Feedback shows only where the user works with the media (not in the tab grid's miniature).
    fn volume_feedback_shown(&self) -> bool {
        matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView)
    }

    /// The volume of the video of tile `index` was changed by `input`: its bar shows the new level (not for a slider).
    pub(super) fn show_volume_feedback(&mut self, index: usize, input: VolumeInput, now: Instant) {
        if !input.shows_bar() || !self.volume_feedback_shown() {
            return;
        }
        let Some(tile) = self.tiles.get(index) else {
            return;
        };
        let Some(video) = tile.video.as_ref() else {
            return;
        };
        let volume = VolumeLevel {
            level: video.player.get_volume(),
            muted: video.player.get_mute(),
        };
        tracing::debug!(
            media_id = tile.media_id,
            level = volume.level,
            muted = volume.muted,
            ?input,
            "[Multiview: VolumeBar] Shown"
        );
        self.volume_feedback.trigger(tile.key, volume, now);
        self.feedback_requested = true;
        self.sync_volume_overlays(now);
        self.bump();
    }

    /// The video of tile `index` was muted or unmuted by `input`: the centered speaker icon shows the new state (only for the
    /// key).
    pub(super) fn show_mute_feedback(&mut self, index: usize, input: MuteInput, now: Instant) {
        if !input.shows_icon() || !self.volume_feedback_shown() {
            return;
        }
        let Some((key, muted)) = self
            .tiles
            .get(index)
            .and_then(|tile| tile.video.as_ref().map(|video| (tile.key, video.last_mute)))
        else {
            return;
        };
        self.show_feedback([key], FeedbackIcon::Sound { muted }, now);
    }

    pub(super) fn has_volume_feedback(&self) -> bool {
        self.volume_feedback.is_active()
            || self
                .tiles
                .iter()
                .any(|t| t.video.as_ref().is_some_and(|v| v.volume_overlay.is_some()))
    }

    /// Brings the volume bars that mpv draws in line with `volume_feedback`: shows, updates (fade, new level, new size of the
    /// window) or removes them. Costs nothing while no bar runs and none is up.
    pub(super) fn sync_volume_overlays(&mut self, now: Instant) {
        self.volume_feedback
            .retain_tiles(|key| self.tiles.iter().any(|t| t.key == key));
        if !self.has_volume_feedback() {
            return;
        }
        let shown = self.volume_feedback_shown();
        let dpi = self.host.surface.dpi;
        let wanted: Vec<Option<VolumeOverlayState>> = (0..self.tiles.len())
            .map(|index| {
                let tile = &self.tiles[index];
                if !shown || tile.kind != TileKind::Video {
                    return None;
                }
                let view = self.volume_feedback.view(tile.key, now)?;
                let window = self.video_window_rect(index)?;
                let size = (window.w.saturating_as::<u32>(), window.h.saturating_as::<u32>());
                // A zoomed video fills its window; otherwise mpv letterboxes the picture inside it.
                let picture = if tile.zoom.as_ref().is_some_and(ZoomState::is_active) {
                    fitted_picture(size, 0.0)
                } else {
                    fitted_picture(size, tile.aspect)
                };
                place_bar(picture, dpi).map(|geometry| VolumeOverlayState::new(view, geometry))
            })
            .collect();
        for (tile, wanted) in self.tiles.iter_mut().zip(wanted) {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            if video.volume_overlay == wanted {
                continue;
            }
            if let Some(state) = wanted {
                // When mpv does not take it (no picture yet) the next frame tries again.
                video.volume_overlay = volume_overlay::show(&video.player, &state).then_some(state);
            } else {
                volume_overlay::hide(&video.player);
                video.volume_overlay = None;
            }
        }
    }
}
