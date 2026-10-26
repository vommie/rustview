//! The video tiles of a Multiview in a tab grid item: the small control bar of the video under the pointer (play/pause and
//! the progress bar) and the time badge of every other video (position and length, drawn by the canvas of the grid item).

use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::route::HostMode;
use super::video_bar::placement;
use super::video_bar::prefs::{RevealMode, VideoBarPrefs};
use super::video_bar::time_text;
use super::video_bar::view::VideoBarTarget;
use std::time::Instant;

/// The time badge of a video in the tab grid: the tile it belongs to, the medium's place (physical pixels relative to the
/// host rectangle; the badge sits in its bottom right corner) and "0:03 / 0:08".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBadgeTarget {
    pub key: TileKey,
    pub rect: Rect,
    pub text: String,
}

impl MultiviewInstance {
    /// The small bars of this tab grid item for the `pointer`; none while the Multiview is not in the grid, a dialog or menu is
    /// open, or the bar would leave `visible`, the part of the grid on screen. Both are in grid content coordinates (physical
    /// pixels, like the host rectangle), so scrolling changes them and never the Multiview.
    pub fn grid_video_bar_targets(
        &mut self,
        pointer: Option<(i32, i32)>,
        visible: Rect,
        prefs: &VideoBarPrefs,
        now: Instant,
    ) -> Vec<VideoBarTarget> {
        self.pointer = pointer;
        let allowed = self.host.surface.mode == HostMode::TabGrid && !self.host.modal_locked && !self.host.menu_open;
        let scale = self.host.surface.dpi.max(0.5);
        self.collect_bar_targets(allowed, prefs, (RevealMode::Hover, true), now, |rect| {
            placement::place_mini(rect, scale).filter(|place| visible.contains_rect(place.area))
        })
    }

    /// The time badges of the videos while the Multiview is in the tab grid, except where a small bar shows (`bar_keys`) or the
    /// video is broken; none anywhere else.
    pub fn grid_time_badges(&self, bar_keys: &[TileKey]) -> Vec<TimeBadgeTarget> {
        if self.host.surface.mode != HostMode::TabGrid {
            return Vec::new();
        }
        let (origin_x, origin_y) = (self.host.surface.rect.x, self.host.surface.rect.y);
        (0..self.tiles.len())
            .filter_map(|index| {
                let tile = &self.tiles[index];
                if tile.error.is_some() || bar_keys.contains(&tile.key) {
                    return None;
                }
                let playback = tile.video.as_ref()?.player.playback();
                let text = time_text::badge(playback.position, playback.duration)?;
                let rect = self.host_content(index)?;
                Some(TimeBadgeTarget {
                    key: tile.key,
                    rect: Rect::new(rect.x - origin_x, rect.y - origin_y, rect.w, rect.h),
                    text,
                })
            })
            .collect()
    }
}
