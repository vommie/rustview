//! Where the minimap sits inside a tile and how a pointer position on it maps to the image (pure, physical pixels).
//!
//! The minimap is a small copy of the whole image with a rectangle for the part that is visible. It sits in a bottom corner of
//! the tile, keeps the aspect ratio of the image, never takes more than a share of the tile and is left out where the tile is
//! too small to carry it.

use super::geometry::{IRect, RectF};
use super::prefs::MinimapCorner;

/// The largest the minimap gets, in logical pixels, and the largest share of the tile it may take.
const MAX_W_LOGICAL: f64 = 200.0;
const MAX_H_LOGICAL: f64 = 150.0;
const MAX_SHARE: f64 = 0.4;
/// Smaller than this (logical pixels) it is no use, it is left out.
const MIN_W_LOGICAL: f64 = 56.0;
const MIN_H_LOGICAL: f64 = 24.0;
/// Distance to the edges of the tile, logical pixels.
const MARGIN_LOGICAL: f64 = 12.0;
/// The pointer reveals the minimap this far (logical pixels) around it.
const REVEAL_LOGICAL: f64 = 14.0;

/// The minimap of one tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapLayout {
    /// The thumbnail rectangle in tile pixels.
    pub rect: IRect,
    dpi: f64,
}

impl MinimapLayout {
    /// `tile` and the result are in physical pixels; `dpi` is the scale factor of the window. `None` when the tile is too small.
    pub fn compute(tile: (i32, i32), image: (u32, u32), corner: MinimapCorner, dpi: f32) -> Option<Self> {
        let dpi = f64::from(dpi).max(0.5);
        if tile.0 <= 0 || tile.1 <= 0 || image.0 == 0 || image.1 == 0 {
            return None;
        }
        let (tile_w, tile_h) = (f64::from(tile.0), f64::from(tile.1));
        let margin = (MARGIN_LOGICAL * dpi).round();
        let box_w = (MAX_W_LOGICAL * dpi).min(tile_w * MAX_SHARE);
        let box_h = (MAX_H_LOGICAL * dpi).min(tile_h * MAX_SHARE);
        let scale = (box_w / f64::from(image.0)).min(box_h / f64::from(image.1));
        let (w, h) = ((f64::from(image.0) * scale).round(), (f64::from(image.1) * scale).round());
        if w < MIN_W_LOGICAL * dpi || h < MIN_H_LOGICAL * dpi || w + 2.0 * margin > tile_w || h + 2.0 * margin > tile_h {
            return None;
        }
        let x = match corner {
            MinimapCorner::BottomLeft => margin,
            MinimapCorner::BottomRight => tile_w - margin - w,
        };
        let y = tile_h - margin - h;
        Some(Self { rect: IRect { x: x as i32, y: y as i32, w: w as i32, h: h as i32 }, dpi })
    }

    fn rect_f(&self) -> RectF {
        RectF::new(f64::from(self.rect.x), f64::from(self.rect.y), f64::from(self.rect.w), f64::from(self.rect.h))
    }

    /// The pointer (tile pixels) is on the minimap or in the margin around it that reveals it.
    pub fn reveals(&self, x: f64, y: f64) -> bool {
        let slack = REVEAL_LOGICAL * self.dpi;
        let r = self.rect_f();
        x >= r.x - slack && y >= r.y - slack && x <= r.right() + slack && y <= r.bottom() + slack
    }

    /// A point on the thumbnail given in tile pixels as shares of the image (0..1), clamped to the image.
    pub fn to_norm(&self, x: f64, y: f64) -> (f64, f64) {
        let r = self.rect_f();
        (((x - r.x) / r.w).clamp(0.0, 1.0), ((y - r.y) / r.h).clamp(0.0, 1.0))
    }
}

/// What a press on the minimap grabs: the rectangle of the visible part (keeping where in it the press was), or nothing (the
/// rectangle jumps to the press and is dragged from its middle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grab {
    /// Offset of the press from the middle of the rectangle, shares of the image.
    pub dx: f64,
    pub dy: f64,
}

impl Grab {
    /// A press at (`nx`, `ny`) with `viewport` (shares of the image) visible.
    pub fn at(nx: f64, ny: f64, viewport: RectF) -> Self {
        if viewport.contains(nx, ny) {
            Self { dx: nx - (viewport.x + viewport.w / 2.0), dy: ny - (viewport.y + viewport.h / 2.0) }
        } else {
            Self { dx: 0.0, dy: 0.0 }
        }
    }

    /// The middle the rectangle gets when the pointer is at (`nx`, `ny`).
    pub fn middle_for(&self, nx: f64, ny: f64) -> (f64, f64) {
        (nx - self.dx, ny - self.dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_sits_in_the_chosen_corner_with_the_aspect_of_the_image() {
        let tile = (1600, 900);
        let left = MinimapLayout::compute(tile, (4000, 3000), MinimapCorner::BottomLeft, 1.0).unwrap();
        assert_eq!((left.rect.w, left.rect.h), (200, 150));
        assert_eq!((left.rect.x, left.rect.y), (12, 900 - 12 - 150));
        let right = MinimapLayout::compute(tile, (4000, 3000), MinimapCorner::BottomRight, 1.0).unwrap();
        assert_eq!(right.rect.x, 1600 - 12 - 200);
        assert_eq!(right.rect.y, left.rect.y);
        // A panorama is wide and flat.
        let wide = MinimapLayout::compute(tile, (6000, 1000), MinimapCorner::BottomLeft, 1.0).unwrap();
        assert_eq!(wide.rect.w, 200);
        assert!(wide.rect.h < 40 && wide.rect.h >= 24);
    }

    #[test]
    fn it_scales_with_the_dpi_and_never_takes_more_than_its_share_of_a_small_tile() {
        let big = MinimapLayout::compute((3200, 1800), (4000, 3000), MinimapCorner::BottomLeft, 2.0).unwrap();
        assert_eq!((big.rect.w, big.rect.h), (400, 300));
        let small = MinimapLayout::compute((400, 300), (4000, 3000), MinimapCorner::BottomLeft, 1.0).unwrap();
        assert!(f64::from(small.rect.w) <= 400.0 * 0.4 + 1.0 && f64::from(small.rect.h) <= 300.0 * 0.4 + 1.0);
        assert!(small.rect.x >= 0 && small.rect.y >= 0);
    }

    #[test]
    fn a_tile_that_is_too_small_gets_none() {
        assert!(MinimapLayout::compute((100, 80), (4000, 3000), MinimapCorner::BottomLeft, 1.0).is_none());
        assert!(MinimapLayout::compute((0, 80), (4000, 3000), MinimapCorner::BottomLeft, 1.0).is_none());
        assert!(MinimapLayout::compute((800, 600), (0, 10), MinimapCorner::BottomLeft, 1.0).is_none());
        // A hairline image: the thumbnail would be a line.
        assert!(MinimapLayout::compute((800, 600), (100_000, 10), MinimapCorner::BottomLeft, 1.0).is_none());
    }

    #[test]
    fn the_pointer_reveals_it_on_and_just_around_it() {
        let m = MinimapLayout::compute((1600, 900), (4000, 3000), MinimapCorner::BottomLeft, 1.0).unwrap();
        let (x, y) = (f64::from(m.rect.x), f64::from(m.rect.y));
        assert!(m.reveals(x + 50.0, y + 50.0));
        assert!(m.reveals(x - 10.0, y - 10.0));
        assert!(!m.reveals(x - 40.0, y + 50.0));
        assert!(!m.reveals(800.0, 100.0));
    }

    #[test]
    fn pointer_positions_map_to_shares_of_the_image_and_are_clamped() {
        let m = MinimapLayout::compute((1600, 900), (4000, 3000), MinimapCorner::BottomLeft, 1.0).unwrap();
        let (x, y) = (f64::from(m.rect.x), f64::from(m.rect.y));
        assert_eq!(m.to_norm(x, y), (0.0, 0.0));
        let (nx, ny) = m.to_norm(x + 100.0, y + 75.0);
        assert!((nx - 0.5).abs() < 1e-9 && (ny - 0.5).abs() < 1e-9);
        assert_eq!(m.to_norm(x - 500.0, y + 9999.0), (0.0, 1.0));
    }

    #[test]
    fn a_press_inside_the_rectangle_keeps_its_place_a_press_outside_centres_it() {
        let viewport = RectF::new(0.2, 0.2, 0.4, 0.4);
        let inside = Grab::at(0.3, 0.5, viewport);
        assert!((inside.dx - (-0.1)).abs() < 1e-9 && (inside.dy - 0.1).abs() < 1e-9);
        // Dragging by 0.2 to the right moves the middle by 0.2 from where it was.
        let (mx, my) = inside.middle_for(0.5, 0.5);
        assert!((mx - 0.6).abs() < 1e-9 && (my - 0.4).abs() < 1e-9);
        let outside = Grab::at(0.9, 0.9, viewport);
        assert_eq!(outside.middle_for(0.9, 0.9), (0.9, 0.9));
    }
}
