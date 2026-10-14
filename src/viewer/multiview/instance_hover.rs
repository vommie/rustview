//! Instance behavior around the pointer: the 50 ms tick, which tile is hovered, when the hover bar of that tile is shown
//! and the runtime correction of aspect ratios reported by mpv.
//!
//! mpv swallows the pointer events over a video, so the hovered tile is found by asking the X server for the pointer and
//! testing it against the tile layout. The bar is a window of its own that the host places; the instance only decides
//! which tile it belongs to (the hovered tile, while the pointer is active or on the bar).

use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::model::{AspectSource, aspect_differs, aspect_from_video_params};
use super::target_pick::pick_target;
use super::tile::StaleProbe;
use super::view::BarTarget;
use crate::library::models::MediaId;
use crate::settings::schema::{MULTIVIEW_BAR_IDLE_MS, MULTIVIEW_MUTE_POLL_TICKS};
use std::time::{Duration, Instant};

impl MultiviewInstance {
    /// Runs every tick while the Multiview is shown. `pointer` is asked for only when there is something to do; it returns
    /// the pointer position in the coordinates of the host window. Sound changes inside mpv and aspect probing only
    /// matter for videos.
    pub fn tick(&mut self, pointer: impl FnOnce() -> Option<(i32, i32)>) {
        self.ticks = self.ticks.wrapping_add(1);
        self.poll_pointer(pointer(), Instant::now());
        if !self.has_video() {
            return;
        }
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

    fn poll_pointer(&mut self, pointer: Option<(i32, i32)>, now: Instant) {
        if pointer != self.pointer {
            self.pointer = pointer;
            self.last_move = Some(now);
        }
        let hovered = pointer.and_then(|(x, y)| self.tile_at_host(x, y));
        self.hovered = hovered;
        if let Some(tile) = hovered.and_then(|i| self.tiles.get(i)) {
            self.last_hovered = Some(tile.media_id);
        }
        let active = self
            .last_move
            .is_some_and(|at| now.duration_since(at) < Duration::from_millis(MULTIVIEW_BAR_IDLE_MS));
        let on_bar = matches!(
            (pointer, self.bar_rect),
            (Some((x, y)), Some(bar)) if bar.contains(x, y)
        );
        self.set_bar_tile(hovered.filter(|_| active || on_bar));
    }

    /// Moves the hover bar to another tile (or hides it).
    fn set_bar_tile(&mut self, tile: Option<usize>) {
        if tile == self.bar_tile {
            return;
        }
        self.bar_tile = tile;
        if tile.is_none() {
            self.bar_rect = None;
        }
        self.bump();
    }

    /// The host placed (or removed) the hover bar: `rect` is its place in the host window in physical pixels. While the
    /// pointer rests on it, the bar stays.
    pub fn set_bar_overlay(&mut self, rect: Option<Rect>) {
        self.bar_rect = rect;
    }

    /// Forgets the hover (the Multiview is hidden, parked, rearranged or its tiles changed).
    pub(super) fn clear_hover(&mut self) {
        self.hovered = None;
        self.pointer = None;
        self.last_move = None;
        self.set_bar_tile(None);
    }

    /// The hovered tile's bar belongs to this tile (for the view).
    pub fn bar_tile(&self) -> Option<usize> {
        self.bar_tile
    }

    /// The bar the host shows now: its tile, the place of that tile in the host window and the buttons of the tile. `None`
    /// while no tile is hovered or the tile offers no button.
    pub fn bar_target(&self) -> Option<BarTarget> {
        let tile = self.bar_tile?;
        let rect = self.host_content(tile)?;
        let actions = self.action_views(tile);
        (!actions.is_empty()).then_some(BarTarget { tile, rect, actions })
    }

    /// F2 acts on the tile under the pointer, else the last hovered one, else the first. The result becomes the rename
    /// target.
    pub fn rename_hovered(&mut self, pointer: Option<(i32, i32)>) -> Option<MediaId> {
        let under_pointer = pointer.and_then(|(x, y)| self.tile_at_host(x, y));
        let ids = self.member_ids();
        let target = pick_target(under_pointer, self.hovered, self.last_hovered, &ids);
        if let Some(id) = target {
            self.rename_target = Some(id);
            self.rename_pinned = false;
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
            if let Some(stale) = tile.stale_probe {
                if stale.params == (width, height, rotate) && stale.ticks_left > 0 {
                    tile.stale_probe = Some(StaleProbe {
                        ticks_left: stale.ticks_left - 1,
                        ..stale
                    });
                    continue;
                }
                tile.stale_probe = None;
            }
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
