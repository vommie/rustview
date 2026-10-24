//! Instance behavior around the pointer: the 50 ms tick, which tile is hovered, which tile the hover bar belongs to and
//! the runtime correction of aspect ratios reported by mpv.
//!
//! mpv swallows the pointer events over a video, so the hovered tile is found by asking the X server for the pointer and
//! testing it against the tile layout. The bar is a window of its own that the host places and reveals; the instance only
//! says which tile it belongs to (the hovered one) and hands over what the host needs to decide whether the pointer is
//! close enough to the bar.

use super::bar_placement::{BarCorner, near_bar_corner};
use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::model::{AspectSource, aspect_differs, aspect_from_video_params};
use super::prefs;
use super::tile::StaleProbe;
use super::view::MediaBarTarget;
use std::time::Instant;

impl MultiviewInstance {
    /// Runs every tick while the Multiview is shown. `pointer` is asked for only when there is something to do; it returns
    /// the pointer position in the coordinates of the host window. Sound changes inside mpv and aspect probing only
    /// matter for videos.
    pub fn tick(&mut self, pointer: impl FnOnce() -> Option<(i32, i32)>) {
        self.ticks = self.ticks.wrapping_add(1);
        self.poll_pointer(pointer());
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

    fn poll_pointer(&mut self, pointer: Option<(i32, i32)>) {
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

    /// The media bars the host may show now. Every tile has a bar of its own:
    /// - the tile whose canvas the pointer is in gets its bar when the pointer is near the corner that carries it (the top right
    ///   of the canvas of the medium, at the height of the primary bar, whatever the medium's size or zoom);
    /// - a tile that has its zoom slider open, or whose key is in `keep` (the host: a menu is open there), keeps its bar whatever the
    ///   pointer does, and the bar follows the tile when media come and go or the layout changes.
    ///
    /// Nothing while the user switched the bars off. A tile that offers no button has no bar.
    pub fn media_bar_targets(&self, keep: &[TileKey]) -> Vec<MediaBarTarget> {
        let bar = prefs::current().media_bar;
        if !bar.enabled {
            return Vec::new();
        }
        let hovered = self.pointer.and_then(|pointer| {
            let tile = self.canvas_tile_at(pointer)?;
            let rect = self.host_room(tile)?;
            near_bar_corner(
                BarCorner::TopRight,
                rect,
                pointer,
                self.host.surface.dpi,
                bar.reveal_distance_px,
            )
            .then_some(tile)
        });
        let mut wanted: Vec<(usize, bool)> = hovered.map(|tile| (tile, false)).into_iter().collect();
        for key in self.zoom_slider.iter().chain(keep) {
            if let Some(tile) = self.tile_index(*key) {
                match wanted.iter_mut().find(|(t, _)| *t == tile) {
                    Some(entry) => entry.1 = true,
                    None => wanted.push((tile, true)),
                }
            }
        }
        wanted
            .into_iter()
            .filter_map(|(tile, forced)| {
                let rect = self.host_room(tile)?;
                let actions = self.action_views(tile);
                (!actions.is_empty()).then(|| MediaBarTarget {
                    tile,
                    key: self.tiles[tile].key,
                    rect,
                    actions,
                    pointer: self.pointer,
                    reveal_distance_px: bar.reveal_distance_px,
                    forced,
                })
            })
            .collect()
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
