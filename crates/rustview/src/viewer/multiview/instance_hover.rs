//! Instance behavior around the pointer: the 50 ms tick, which tile is hovered, which tile the hover bar belongs to and
//! the runtime correction of aspect ratios reported by mpv.
//!
//! The host hands in the pointer from the pointer events of its window (`ui::multiview::pointer_events`), in window
//! coordinates; the hovered tile is found by testing it against the tile layout. The action bars are drawn by the canvas; the
//! instance says which bars show (the tile under the pointer near the corner of the bar, plus the bars that stay) and where.

use super::bar_placement::{BarCorner, bar_size_logical, in_reveal_zone, place_bar};
use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::model::{AspectSource, aspect_differs, aspect_from_video_params};
use super::prefs;
use super::tile::StaleProbe;
use super::view::MediaBarTarget;
use std::time::Instant;

impl MultiviewInstance {
    /// Runs every tick while the Multiview is shown. `pointer` is the last pointer position the host saw over its window
    /// (window coordinates, `None` outside it); the hovered tile is found again, the layout may have changed. Sound changes
    /// inside mpv and aspect probing only matter for videos.
    pub fn tick(&mut self, pointer: Option<(i32, i32)>) {
        self.ticks = self.ticks.wrapping_add(1);
        self.pointer_moved(pointer);
        self.poll_zoom();
        self.poll_mark_reveal(Instant::now());
        if !self.has_video() {
            return;
        }
        self.probe_aspects();
        // A mark mpv did not take yet (no picture) is tried again.
        self.sync_focus_overlays();
    }

    /// The tile under a point given in host window coordinates.
    fn tile_at_host(&self, x: i32, y: i32) -> Option<usize> {
        // A zoomed picture fills its whole room, which counts as the tile.
        let zoomed = (0..self.tiles.len()).find(|&i| {
            self.host_tile_rect(i).is_some_and(|r| {
                self.tiles[i]
                    .zoom
                    .as_ref()
                    .is_some_and(super::super::zoom::state::ZoomState::is_active)
                    && r.contains(x, y)
            })
        });
        if zoomed.is_some() {
            return zoomed;
        }
        let (cx, cy) = self.host.projection.point_to_container(x, y);
        self.layout.tile_at(cx, cy)
    }

    /// The pointer moved over the host window, or left it (`None`); window coordinates (physical pixels).
    pub fn pointer_moved(&mut self, pointer: Option<(i32, i32)>) {
        if pointer != self.pointer || self.last_move.is_none() {
            self.last_move = Some(Instant::now());
        }
        self.pointer = pointer;
        self.hovered = pointer.and_then(|(x, y)| self.tile_at_host(x, y));
    }

    /// Forgets the hover (the Multiview is hidden, parked, rearranged or its tiles changed).
    pub(super) fn clear_hover(&mut self) {
        self.hovered = None;
        self.pointer = None;
    }

    /// The tile whose canvas (its room, see `host_room`) the pointer is in: the hovered medium, or the free space around it.
    fn canvas_tile_at(&self, pointer: (i32, i32)) -> Option<usize> {
        self.hovered.or_else(|| {
            (0..self.tiles.len()).find(|&i| {
                self.host_room(i)
                    .is_some_and(|room| room.contains(pointer.0, pointer.1))
            })
        })
    }

    /// The media bars the canvas of the host draws now. Every tile has a bar of its own:
    /// - the tile whose canvas the pointer is in gets its bar while the pointer is within the reveal distance of it (the top right
    ///   of the canvas of the medium, at the height of the primary bar, whatever the medium's size or zoom);
    /// - a tile that has its zoom slider open, or whose key is in `keep` (the host: a drop-down is open there), keeps its bar
    ///   whatever the pointer does, and the bar follows the tile when media come and go or the layout changes.
    ///
    /// Nothing while the user switched the bars off. A tile that offers no button, or whose room is too small for the bar, has no
    /// bar.
    pub fn media_bar_targets(&self, keep: &[TileKey]) -> Vec<MediaBarTarget> {
        let prefs = prefs::current().media_bar;
        if !prefs.enabled {
            return Vec::new();
        }
        let mut wanted: Vec<(usize, bool)> = self
            .pointer
            .and_then(|pointer| self.canvas_tile_at(pointer))
            .map(|tile| (tile, false))
            .into_iter()
            .collect();
        for key in self.zoom_slider.iter().chain(keep) {
            if let Some(tile) = self.tile_index(*key) {
                match wanted.iter_mut().find(|(t, _)| *t == tile) {
                    Some(entry) => entry.1 = true,
                    None => wanted.push((tile, true)),
                }
            }
        }
        let scale = self.host.surface.dpi;
        let origin = (self.host.surface.rect.x, self.host.surface.rect.y);
        wanted
            .into_iter()
            .filter_map(|(tile, forced)| {
                let room = self.host_room(tile)?;
                let actions = self.action_views(tile);
                if actions.is_empty() {
                    return None;
                }
                let split = actions.iter().any(|a| a.split);
                let bar = place_bar(BarCorner::TopRight, room, bar_size_logical(actions.len(), split), scale)?;
                let revealed = forced
                    || self
                        .pointer
                        .is_some_and(|p| in_reveal_zone(bar, p, scale, prefs.reveal_distance_px));
                revealed.then(|| MediaBarTarget {
                    tile,
                    key: self.tiles[tile].key,
                    bar: Rect::new(bar.x - origin.0, bar.y - origin.1, bar.w, bar.h),
                    actions,
                    forced,
                })
            })
            .collect()
    }

    /// Videos start with the aspect ratio of the library record; once mpv knows the real display size, correct it.
    pub(super) fn probe_aspects(&mut self) {
        let mut changed = false;
        let mut sizes: Vec<(usize, (u32, u32))> = Vec::new();
        for (index, tile) in self.tiles.iter_mut().enumerate() {
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
            // The zoom works on the picture as displayed: a rotated video has its sides swapped.
            let (w, h) = (u32::try_from(width).unwrap_or(0), u32::try_from(height).unwrap_or(0));
            sizes.push((index, if rotate.rem_euclid(180) == 90 { (h, w) } else { (w, h) }));
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
        for (index, size) in sizes {
            self.ensure_video_zoom(index, size);
        }
    }
}
