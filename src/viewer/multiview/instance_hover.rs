//! Instance behavior around the pointer: the 50 ms tick, which tile is hovered, which tile the hover bar belongs to and
//! the runtime correction of aspect ratios reported by mpv.
//!
//! mpv swallows the pointer events over a video, so the hovered tile is found by asking the X server for the pointer and
//! testing it against the tile layout. The bar is a window of its own that the host places and reveals; the instance only
//! says which tile it belongs to (the hovered one) and hands over what the host needs to decide whether the pointer is
//! close enough to the bar.

use super::instance::MultiviewInstance;
use super::bar_placement::{BarCorner, near_bar_corner};
use super::model::{AspectSource, aspect_differs, aspect_from_video_params};
use super::prefs;
use super::target_pick::pick_target;
use std::time::Instant;
use super::tile::StaleProbe;
use super::view::MediaBarTarget;
use crate::library::models::MediaId;

impl MultiviewInstance {
    /// Runs every tick while the Multiview is shown. `pointer` is asked for only when there is something to do; it returns
    /// the pointer position in the coordinates of the host window. Sound changes inside mpv and aspect probing only
    /// matter for videos.
    pub fn tick(&mut self, pointer: impl FnOnce() -> Option<(i32, i32)>) {
        self.ticks = self.ticks.wrapping_add(1);
        self.poll_pointer(pointer());
        self.poll_zoom();
        if !self.has_video() {
            return;
        }
        self.probe_aspects();
    }

    /// The tile under a point given in host window coordinates.
    fn tile_at_host(&self, x: i32, y: i32) -> Option<usize> {
        let (cx, cy) = self.host.projection.point_to_container(x, y);
        self.layout.tile_at(cx, cy)
    }

    fn poll_pointer(&mut self, pointer: Option<(i32, i32)>) {
        if pointer != self.pointer || self.last_move.is_none() {
            self.last_move = Some(Instant::now());
        }
        self.pointer = pointer;
        self.hovered = pointer.and_then(|(x, y)| self.tile_at_host(x, y));
        if let Some(tile) = self.hovered.and_then(|i| self.tiles.get(i)) {
            self.last_hovered = Some(tile.media_id);
        }
    }

    /// Forgets the hover (the Multiview is hidden, parked, rearranged or its tiles changed).
    pub(super) fn clear_hover(&mut self) {
        self.hovered = None;
        self.pointer = None;
    }

    /// The bar the host may show now: the hovered tile, where that tile is in the host window (physical pixels), the
    /// pointer and the buttons of the tile. The host reveals the bar once the pointer is within the reveal distance of it.
    /// `None` while no tile is hovered, the pointer is nowhere near the corner that carries the bar, the user switched the
    /// bar off or the tile offers no button.
    pub fn media_bar_target(&self) -> Option<MediaBarTarget> {
        let bar = prefs::current().media_bar;
        let (tile, pointer) = (self.hovered?, self.pointer?);
        let rect = self.host_content(tile)?;
        if !bar.enabled
            || !near_bar_corner(BarCorner::TopRight, rect, pointer, self.host.surface.dpi, bar.reveal_distance_px)
        {
            return None;
        }
        let actions = self.action_views(tile);
        (!actions.is_empty()).then_some(MediaBarTarget {
            tile,
            rect,
            actions,
            pointer,
            reveal_distance_px: bar.reveal_distance_px,
        })
    }

    /// F2 acts on the tile under the pointer, else the last hovered one, else the first. The result becomes the rename
    /// target.
    pub fn rename_hovered(&mut self, pointer: Option<(i32, i32)>) -> Option<MediaId> {
        let under_pointer = pointer.and_then(|(x, y)| self.tile_at_host(x, y));
        let ids = self.slot_media_ids();
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
