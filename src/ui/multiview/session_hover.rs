//! Session behavior around the pointer: the 20 Hz tick, which tile is hovered, the mute badge (a Slint button shown in a
//! hole punched into the hovered video's X11 window) and the runtime correction of aspect ratios reported by mpv.

use super::layout::{self, Rect};
use super::model::{AspectSource, aspect_differs, aspect_from_video_params};
use super::session::Session;
use super::views;
use crate::library::models::MediaId;
use crate::settings::schema::{
    MULTIVIEW_BADGE_INSET_LOGICAL_PX, MULTIVIEW_BADGE_LOGICAL_PX,
    MULTIVIEW_BADGE_MIN_TILE_HEIGHT_LOGICAL_PX, MULTIVIEW_BADGE_MIN_TILE_WIDTH_LOGICAL_PX,
    MULTIVIEW_MUTE_POLL_TICKS,
};
use crate::viewer::XRectangle;

impl Session {
    /// Only videos need the tick (badge, mute changes inside mpv, aspect probing); image-only sessions stay idle.
    pub(super) fn on_tick(&mut self) {
        if !self.tiles.iter().any(|t| t.video.is_some()) {
            return;
        }
        self.ticks = self.ticks.wrapping_add(1);
        self.open_pending_hole();
        self.poll_pointer();
        if self.ticks.is_multiple_of(MULTIVIEW_MUTE_POLL_TICKS) && self.poll_mute() {
            self.push_badge();
        }
        self.probe_aspects();
    }

    /// The tile under the pointer right now, asking the X server (cheap, and independent of which window owns the pointer).
    pub(super) fn tile_under_pointer(&self) -> Option<usize> {
        let (root_x, root_y) = self.probe.as_ref().and_then(|p| p.position())?;
        self.layout
            .tile_at(root_x - self.monitor.x, root_y - self.monitor.y)
    }

    fn poll_pointer(&mut self) {
        let hovered = self.tile_under_pointer();
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
        let content = self.layout.tiles.get(index)?.content;
        let rect = layout::badge_rect(
            content,
            self.scale,
            MULTIVIEW_BADGE_LOGICAL_PX,
            MULTIVIEW_BADGE_INSET_LOGICAL_PX,
            MULTIVIEW_BADGE_MIN_TILE_WIDTH_LOGICAL_PX,
            MULTIVIEW_BADGE_MIN_TILE_HEIGHT_LOGICAL_PX,
        )?;
        Some((index, rect))
    }

    /// Shows the badge on the hovered tile (or hides it). The Slint button is drawn first; the hole in the video window
    /// follows on the next tick so the background never shows through an empty hole.
    pub(super) fn refresh_badge(&mut self) {
        let wanted = self.hovered.and_then(|i| self.badge_for(i));
        if wanted == self.badge {
            return;
        }
        self.drop_badge();
        self.badge = wanted;
        self.hole_pending = wanted.is_some();
        self.push_badge();
    }

    fn open_pending_hole(&mut self) {
        if !self.hole_pending {
            return;
        }
        self.hole_pending = false;
        let Some((index, rect)) = self.badge else {
            return;
        };
        let hole = XRectangle {
            x: rect.x.clamp(0, i16::MAX as i32) as i16,
            y: rect.y.clamp(0, i16::MAX as i32) as i16,
            width: rect.w.clamp(1, u16::MAX as i32) as u16,
            height: rect.h.clamp(1, u16::MAX as i32) as u16,
        };
        if let Some(video) = self.tiles.get_mut(index).and_then(|t| t.video.as_mut()) {
            video.window.clip_cutout_rectangles(&[hole]);
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
            self.push_badge();
        }
    }

    pub(super) fn clear_hover(&mut self) {
        self.drop_badge();
        self.hovered = None;
    }

    /// Mirrors the badge state into the Slint window; the icon shows the player's actual mute state.
    pub(super) fn push_badge(&self) {
        let Some(win) = self.window.upgrade() else {
            return;
        };
        let muted = self
            .badge
            .and_then(|(i, _)| self.tiles.get(i))
            .and_then(|t| t.video.as_ref())
            .is_some_and(|v| v.last_mute);
        win.set_badge(views::badge_view(
            self.badge.map(|(_, rect)| rect),
            muted,
            self.scale,
        ));
    }

    /// F2 acts on the tile under the pointer, else the last hovered one, else the first.
    pub(super) fn rename_hovered(&self) {
        let still_shown = |id: &MediaId| self.tiles.iter().any(|t| t.media_id == *id);
        let target = self
            .tile_under_pointer()
            .or(self.hovered)
            .and_then(|i| self.tiles.get(i))
            .map(|t| t.media_id)
            .or(self.last_hovered.filter(still_shown))
            .or_else(|| self.tiles.first().map(|t| t.media_id));
        if let Some(media_id) = target {
            self.handle.request_rename(media_id);
        }
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
