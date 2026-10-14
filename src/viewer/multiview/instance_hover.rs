//! Instance behavior around the pointer: the 50 ms tick, which tile is hovered, when the hover bar of that tile is shown
//! and the runtime correction of aspect ratios reported by mpv.
//!
//! mpv swallows the pointer events over a video, so the hovered tile is found by asking the X server for the pointer and
//! testing it against the tile layout. The bar is a Slint overlay placed by the host; the instance only decides which
//! tile it belongs to (the hovered tile, while the pointer is active or on the bar) and cuts the bar out of that tile's
//! video window, like the tab context menu is cut out of the videos below it.

use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::model::{AspectSource, aspect_differs, aspect_from_video_params};
use super::target_pick::pick_target;
use super::tile::StaleProbe;
use crate::library::models::MediaId;
use crate::settings::schema::{MULTIVIEW_BAR_IDLE_MS, MULTIVIEW_MUTE_POLL_TICKS};
use std::time::{Duration, Instant};

impl MultiviewInstance {
    /// Runs every tick while the Multiview is shown. `pointer` is asked for only when there is something to do; it returns
    /// the pointer position in the coordinates of the host window. Sound changes inside mpv and aspect probing only
    /// matter for videos.
    pub fn tick(&mut self, pointer: impl FnOnce() -> Option<(i32, i32)>) {
        self.tick_at(Instant::now(), pointer);
    }

    /// `tick` with an explicit clock (tests).
    pub fn tick_at(&mut self, now: Instant, pointer: impl FnOnce() -> Option<(i32, i32)>) {
        self.ticks = self.ticks.wrapping_add(1);
        self.poll_pointer(pointer(), now);
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

    /// Moves the hover bar to another tile (or hides it). The cutout of the tile it leaves goes away at once.
    fn set_bar_tile(&mut self, tile: Option<usize>) {
        if tile == self.bar_tile {
            return;
        }
        let left = std::mem::replace(&mut self.bar_tile, tile);
        if let Some(index) = left {
            self.refresh_shape(index);
        }
        if tile.is_none() {
            self.bar_rect = None;
        }
        self.bump();
    }

    /// The host drew (or removed) the hover bar: `rect` is its place in the host rectangle in physical pixels. The
    /// video window below it is cut open there.
    pub fn set_bar_overlay(&mut self, rect: Option<Rect>) {
        let origin = self.host.surface.rect;
        let placed = rect.map(|r| Rect::new(r.x + origin.x, r.y + origin.y, r.w, r.h));
        if placed == self.bar_rect {
            return;
        }
        self.bar_rect = placed;
        if let Some(index) = self.bar_tile {
            self.apply_cutouts(index);
        }
    }

    /// Resets the shape of a video window (the shape is relative to the window, so it has to go before the window moves or
    /// resizes) and applies the cutouts it needs now.
    pub(super) fn refresh_shape(&mut self, index: usize) {
        if let Some(video) = self.tiles.get_mut(index).and_then(|t| t.video.as_mut()) {
            video.window.reset_clip_shape();
        }
        self.apply_cutouts(index);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::instance::SharedInstance;
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::tile::TileSeed;
    use std::path::PathBuf;

    fn instance(count: u64) -> SharedInstance {
        let seeds = (1..=count)
            .map(|id| TileSeed {
                media_id: id,
                kind: TileKind::Image,
                aspect: 1.78,
                path: PathBuf::new(),
                filename: format!("{id}.png"),
            })
            .collect();
        let shared = MultiviewInstance::detached(seeds);
        shared
            .lock()
            .set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        shared
    }

    fn centre(mv: &MultiviewInstance, tile: usize) -> (i32, i32) {
        let r = mv.layout.tiles[tile].content;
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    #[test]
    fn the_bar_follows_the_pointer_and_hides_when_it_rests() {
        let shared = instance(2);
        let mut mv = shared.lock();
        let t0 = Instant::now();
        let (first, second) = (centre(&mv, 0), centre(&mv, 1));

        mv.tick_at(t0, || Some(first));
        assert_eq!(mv.bar_tile(), Some(0));
        mv.tick_at(t0 + Duration::from_millis(500), || Some(second));
        assert_eq!(mv.bar_tile(), Some(1), "the bar moves with the pointer");

        let rests = t0 + Duration::from_millis(500 + MULTIVIEW_BAR_IDLE_MS + 1);
        mv.tick_at(rests, || Some(second));
        assert_eq!(mv.bar_tile(), None, "a resting pointer hides the bar");
        mv.tick_at(rests + Duration::from_millis(50), || Some((second.0 + 3, second.1)));
        assert_eq!(mv.bar_tile(), Some(1), "any movement shows it again");
    }

    #[test]
    fn the_bar_stays_while_the_pointer_rests_on_it() {
        let shared = instance(1);
        let mut mv = shared.lock();
        let t0 = Instant::now();
        let spot = centre(&mv, 0);
        mv.tick_at(t0, || Some(spot));
        mv.set_bar_overlay(Some(Rect::new(spot.0 - 40, spot.1 - 20, 120, 34)));
        let later = t0 + Duration::from_millis(MULTIVIEW_BAR_IDLE_MS * 4);
        mv.tick_at(later, || Some(spot));
        assert_eq!(mv.bar_tile(), Some(0), "the pointer is on the bar");
        mv.tick_at(later + Duration::from_millis(50), || Some((spot.0 + 200, spot.1)));
        mv.tick_at(later + Duration::from_millis(60 + MULTIVIEW_BAR_IDLE_MS), || {
            Some((spot.0 + 200, spot.1))
        });
        assert_eq!(mv.bar_tile(), None, "off the bar, the idle rule applies again");
    }

    #[test]
    fn gutters_and_the_outside_have_no_bar() {
        let shared = instance(2);
        let mut mv = shared.lock();
        let t0 = Instant::now();
        let gutter = mv.layout.gutters[0].rect;
        mv.tick_at(t0, || Some((gutter.x + gutter.w / 2, gutter.y + gutter.h / 2)));
        assert_eq!(mv.bar_tile(), None);
        mv.tick_at(t0, || None);
        assert_eq!(mv.bar_tile(), None);
    }

    #[test]
    fn the_bar_overlay_is_reported_relative_to_the_host() {
        let shared = instance(1);
        let mut mv = shared.lock();
        mv.set_bar_overlay(Some(Rect::new(10, 20, 100, 30)));
        assert_eq!(mv.bar_rect, Some(Rect::new(10, 20, 100, 30)));
        mv.set_bar_overlay(None);
        assert_eq!(mv.bar_rect, None);
    }
}
