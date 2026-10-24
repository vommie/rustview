//! The video tiles of a Multiview in a tab grid item: the small control bar of the video under the pointer (play/pause and
//! the progress bar) and the time badge of every other video (position and length, drawn by mpv over the picture).

use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::route::HostMode;
use super::video_bar::placement;
use super::video_bar::prefs::{RevealMode, VideoBarPrefs};
use super::video_bar::time_text;
use super::video_bar::view::VideoBarTarget;
use crate::viewer::mpv_engine::OsdText;
use rustview_core::settings::schema::{MULTIVIEW_TIME_BADGE_FONT_LOGICAL_PX, MULTIVIEW_TIME_BADGE_MARGIN_LOGICAL_PX};
use rustview_core::util::cast::FloatToInt;
use std::time::Instant;

impl MultiviewInstance {
    /// The small bars of this tab grid item for the `pointer` (coordinates of the main window); none while the Multiview is
    /// not in the grid, a dialog or menu is open, or the bar would leave the visible part of the grid.
    pub fn grid_video_bar_targets(
        &mut self,
        pointer: Option<(i32, i32)>,
        prefs: &VideoBarPrefs,
        now: Instant,
    ) -> Vec<VideoBarTarget> {
        self.pointer = pointer;
        // A video that started in the grid (its tab was never shown) lets the pointer through like the others.
        self.apply_pointer_passthrough();
        let allowed = self.host.surface.mode == HostMode::TabGrid && !self.host.modal_locked && !self.host.menu_open;
        let scale = self.host.surface.dpi.max(0.5);
        let clip = self.host.surface.clip;
        self.collect_bar_targets(allowed, prefs, (RevealMode::Hover, true), now, |rect| {
            placement::place_mini(rect, scale).filter(|place| clip.is_none_or(|clip| clip.contains_rect(place.window)))
        })
    }

    /// Shows the time badge of every video while the Multiview is in the tab grid, except where a small bar shows
    /// (`bar_keys`), and takes them away everywhere else. mpv is told only what changed.
    pub fn sync_time_badges(&mut self, bar_keys: &[TileKey]) {
        let grid = self.host.surface.mode == HostMode::TabGrid;
        let dpi = self.host.surface.dpi.max(0.5);
        let px = |logical: f32| (logical * dpi).round().to_int::<i32>();
        for index in 0..self.tiles.len() {
            let rect = self.host_content(index);
            let tile = &mut self.tiles[index];
            let (key, broken) = (tile.key, tile.error.is_some());
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            let wanted = rect
                .filter(|_| grid && !broken && !bar_keys.contains(&key))
                .and_then(|rect| {
                    let playback = video.player.playback();
                    time_text::badge(playback.position, playback.duration).map(|text| OsdText {
                        text,
                        area: (rect.w, rect.h),
                        size: px(MULTIVIEW_TIME_BADGE_FONT_LOGICAL_PX),
                        margin: px(MULTIVIEW_TIME_BADGE_MARGIN_LOGICAL_PX),
                    })
                });
            if video.time_badge != wanted {
                video.player.set_osd_text(wanted.as_ref());
                video.time_badge = wanted;
            }
        }
    }
}
