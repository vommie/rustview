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
        let (w, h) = (
            (f64::from(image.0) * scale).round(),
            (f64::from(image.1) * scale).round(),
        );
        if w < MIN_W_LOGICAL * dpi || h < MIN_H_LOGICAL * dpi || w + 2.0 * margin > tile_w || h + 2.0 * margin > tile_h
        {
            return None;
        }
        let x = match corner {
            MinimapCorner::BottomLeft => margin,
            MinimapCorner::BottomRight => tile_w - margin - w,
        };
        let y = tile_h - margin - h;
        Some(Self {
            rect: IRect {
                x: x as i32,
                y: y as i32,
                w: w as i32,
                h: h as i32,
            },
            dpi,
        })
    }

    fn rect_f(&self) -> RectF {
        RectF::new(
            f64::from(self.rect.x),
            f64::from(self.rect.y),
            f64::from(self.rect.w),
            f64::from(self.rect.h),
        )
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
            Self {
                dx: nx - (viewport.x + viewport.w / 2.0),
                dy: ny - (viewport.y + viewport.h / 2.0),
            }
        } else {
            Self { dx: 0.0, dy: 0.0 }
        }
    }

    /// The middle the rectangle gets when the pointer is at (`nx`, `ny`).
    pub fn middle_for(&self, nx: f64, ny: f64) -> (f64, f64) {
        (nx - self.dx, ny - self.dy)
    }
}

/// Screen pixels the picture may move per pixel of pointer movement while the minimap is dragged slowly, however far in it is.
/// Dragged 1:1 (the rectangle under the pointer) a picture zoomed 20x moves by hundreds of pixels per pointer pixel; this keeps
/// a slow drag controllable and lets the rectangle lag behind the pointer instead.
const TARGET_SCREEN_PER_POINTER_PX: f64 = 8.0;
/// The finest the drag gets (the rectangle moves this share of the pointer's way).
const MIN_GAIN: f64 = 0.03;
/// The pointer speed (minimap pixels per second) from which a drag gets coarser again, and where it is 1:1 again: a flick
/// crosses the whole minimap, a careful movement is precise ("precision with acceleration").
const FAST_FROM_PX_S: f64 = 250.0;
const FAST_TO_PX_S: f64 = 1000.0;

/// How much of the pointer's movement on the minimap the rectangle follows. `screen_per_pointer_px` is how many screen pixels
/// the picture moves for one pixel of rectangle movement (the size of the zoomed image divided by the size of the minimap),
/// `speed` the smoothed pointer speed in minimap pixels per second. 1.0 = the rectangle stays under the pointer.
pub fn drag_gain(screen_per_pointer_px: f64, speed: f64) -> f64 {
    let slow = if screen_per_pointer_px.is_finite() && screen_per_pointer_px > 0.0 {
        (TARGET_SCREEN_PER_POINTER_PX / screen_per_pointer_px).clamp(MIN_GAIN, 1.0)
    } else {
        1.0
    };
    let t = if speed.is_finite() {
        ((speed - FAST_FROM_PX_S) / (FAST_TO_PX_S - FAST_FROM_PX_S)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let smooth = t * t * (3.0 - 2.0 * t);
    slow + (1.0 - slow) * smooth
}
