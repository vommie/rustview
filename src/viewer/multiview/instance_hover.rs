//! Instance behavior around the pointer: the 20 Hz tick, which tile is hovered, the mute badge (a button drawn by the host
//! and a hole punched into the hovered video's X11 window) and the runtime correction of aspect ratios reported by mpv.

use super::instance::MultiviewInstance;
use super::layout::{self, Rect};
use super::model::{AspectSource, aspect_differs, aspect_from_video_params};
use super::target_pick::pick_target;
use crate::library::models::MediaId;
use crate::settings::schema::{
    MULTIVIEW_BADGE_INSET_LOGICAL_PX, MULTIVIEW_BADGE_LOGICAL_PX,
    MULTIVIEW_BADGE_MIN_TILE_HEIGHT_LOGICAL_PX, MULTIVIEW_BADGE_MIN_TILE_WIDTH_LOGICAL_PX,
    MULTIVIEW_MUTE_POLL_TICKS,
};

impl MultiviewInstance {
    /// Only videos need the tick (badge, mute changes inside mpv, aspect probing); image-only instances stay idle.
    /// `pointer` is asked for only when there is something to do; it returns the pointer position in the coordinates of
    /// the host window.
    pub fn tick(&mut self, pointer: impl FnOnce() -> Option<(i32, i32)>) {
        if !self.has_video() {
            return;
        }
        self.ticks = self.ticks.wrapping_add(1);
        self.open_pending_hole();
        self.poll_pointer(pointer());
        if self.ticks.is_multiple_of(MULTIVIEW_MUTE_POLL_TICKS) && self.poll_mute() {
            self.bump();
        }
        self.probe_aspects();
    }

    /// The tile under a point given in host window coordinates.
    fn tile_at_host(&self, x: i32, y: i32) -> Option<usize> {
        let (cx, cy) = self.host.projection.point_to_container(x, y);
        self.layout.tile_at(cx, cy)
    }

    fn poll_pointer(&mut self, pointer: Option<(i32, i32)>) {
        let hovered = pointer.and_then(|(x, y)| self.tile_at_host(x, y));
        if hovered == self.hovered {
            return;
        }
        self.hovered = hovered;
        if let Some(tile) = hovered.and_then(|i| self.tiles.get(i)) {
            self.last_hovered = Some(tile.media_id);
        }
        self.refresh_badge();
    }

    /// The badge exists only on live video tiles that are large enough for it.
    fn badge_for(&self, index: usize) -> Option<(usize, Rect)> {
        self.tiles.get(index)?.video.as_ref()?;
        let content = self.host_content(index)?;
        let rect = layout::badge_rect(
            content,
            self.host.surface.dpi,
            MULTIVIEW_BADGE_LOGICAL_PX,
            MULTIVIEW_BADGE_INSET_LOGICAL_PX,
            MULTIVIEW_BADGE_MIN_TILE_WIDTH_LOGICAL_PX,
            MULTIVIEW_BADGE_MIN_TILE_HEIGHT_LOGICAL_PX,
        )?;
        Some((index, rect))
    }

    /// Shows the badge on the hovered tile (or hides it). The host draws the button first; the hole in the video window
    /// follows on the next tick so the background never shows through an empty hole.
    pub(super) fn refresh_badge(&mut self) {
        let wanted = self.hovered.and_then(|i| self.badge_for(i));
        if wanted == self.badge {
            return;
        }
        self.drop_badge();
        self.badge = wanted;
        self.hole_pending = wanted.is_some();
        self.bump();
    }

    /// Opens the hole of the badge in the video window (together with the cutouts of the host).
    fn open_pending_hole(&mut self) {
        if !self.hole_pending {
            return;
        }
        self.hole_pending = false;
        if let Some((index, _)) = self.badge {
            self.apply_cutouts(index);
        }
    }

    /// Removes the badge and its hole (the window shape is relative to the window, so it must go before the window moves
    /// or resizes).
    pub(super) fn drop_badge(&mut self) {
        self.hole_pending = false;
        if let Some((index, _)) = self.badge.take() {
            if let Some(video) = self.tiles.get_mut(index).and_then(|t| t.video.as_mut()) {
                video.window.reset_clip_shape();
            }
            self.apply_cutouts(index);
            self.bump();
        }
    }

    pub(super) fn clear_hover(&mut self) {
        self.drop_badge();
        self.hovered = None;
    }

    /// F2 acts on the tile under the pointer, else the last hovered one, else the first. The result becomes the rename
    /// target.
    pub fn rename_hovered(&mut self, pointer: Option<(i32, i32)>) -> Option<MediaId> {
        let under_pointer = pointer.and_then(|(x, y)| self.tile_at_host(x, y));
        let ids = self.member_ids();
        let target = pick_target(under_pointer, self.hovered, self.last_hovered, &ids);
        if let Some(id) = target {
            self.rename_target = Some(id);
        }
        target
    }

    /// Videos start with the aspect ratio of the library record; once mpv knows the real display size, correct it.
    pub(super) fn probe_aspects(&mut self) {
        let mut changed = false;
        for tile in &mut self.tiles {
            if tile.aspect_source == AspectSource::Probed {
                continue;
            }
            let Some(video) = tile.video.as_ref() else {
                continue;
            };
            let Some((width, height, rotate)) = video.player.video_display_params() else {
                continue;
            };
            let Some(aspect) = aspect_from_video_params(width, height, rotate) else {
                continue;
            };
            tile.aspect_source = AspectSource::Probed;
            if aspect_differs(aspect, tile.aspect) {
                tracing::info!(
                    media_id = tile.media_id,
                    library = tile.aspect,
                    probed = aspect,
                    "[Multiview: Aspect] Corrected from mpv's video parameters"
                );
                tile.aspect = aspect;
                changed = true;
            }
        }
        if changed {
            self.apply_layout();
        }
    }
}
