//! The volume bar and the mute icon of a video (`volume_feedback`): the wheel and the video player keys start them, the canvas
//! of the host draws them over the picture (`TileView::volume`, `TileView::feedback`), and they keep the time of the feedback
//! pulse, so the host's feedback timer (`feedback_wakeup`, `poll_feedback`) drives them too.

use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::media_feedback::FeedbackIcon;
use super::model::TileKind;
use super::route::HostMode;
use super::view::VolumeBarTileView;
use super::volume_feedback::{MuteInput, VolumeInput, VolumeLevel, fitted_picture, place_bar};
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

    /// The volume bar tile `index` shows at `now` (relative to the tile, whose rectangle is the medium's place): centered in the
    /// lower part of the visible picture (mpv may letterbox a plain video by a pixel or two; a zoomed one is the picture itself).
    pub(super) fn volume_bar_view(&self, index: usize, now: Instant) -> Option<VolumeBarTileView> {
        let tile = self.tiles.get(index)?;
        if !self.volume_feedback_shown() || tile.kind != TileKind::Video {
            return None;
        }
        let view = self.volume_feedback.view(tile.key, now)?;
        let (window, content) = (self.video_placement(index)?.visible()?, self.host_content(index)?);
        let size = (window.w.saturating_as::<u32>(), window.h.saturating_as::<u32>());
        let aspect = if tile.zoom.as_ref().is_some_and(ZoomState::is_active) {
            0.0
        } else {
            tile.aspect
        };
        let mut picture = fitted_picture(size, aspect);
        picture.x += i64::from(window.x - content.x);
        picture.y += i64::from(window.y - content.y);
        let geometry = place_bar(picture, self.host.surface.dpi)?;
        Some(VolumeBarTileView {
            rect: Rect::new(
                geometry.rect.x.saturating_as::<i32>(),
                geometry.rect.y.saturating_as::<i32>(),
                geometry.rect.w.saturating_as::<i32>(),
                geometry.rect.h.saturating_as::<i32>(),
            ),
            gap: geometry.gap.saturating_as::<i32>(),
            share: view.volume.share(),
            icon: view.volume.icon(),
            muted: view.volume.muted,
            alpha: view.alpha,
        })
    }
}
