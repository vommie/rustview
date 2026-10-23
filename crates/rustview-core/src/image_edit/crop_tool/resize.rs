//! The geometry of resizing and drawing the crop rectangle (pure): handles, Shift with a ratio, the minimum size.

use super::{Handle, MIN_CROP_SIZE};
use crate::image_edit::geometry::{PixelRect, snap_coordinate};
use crate::util::cast::FloatToInt;

fn min_side(image: (u32, u32)) -> (u32, u32) {
    (MIN_CROP_SIZE.min(image.0).max(1), MIN_CROP_SIZE.min(image.1).max(1))
}

/// Moves the edges the handle stands for to the pointer. With `ratio` the proportions stay: a corner keeps the opposite
/// corner, an edge keeps the opposite edge and the middle of the other axis.
pub(super) fn resize(
    start: PixelRect,
    handle: Handle,
    (px, py): (f64, f64),
    image: (u32, u32),
    ratio: Option<f64>,
) -> PixelRect {
    let (min_w, min_h) = min_side(image);
    if let Some(r) = ratio {
        if handle.is_corner() {
            let ax = if handle.moves_right() { start.x } else { start.right() };
            let ay = if handle.moves_bottom() { start.y } else { start.bottom() };
            return corner_with_ratio(
                (ax, ay),
                handle.moves_right(),
                handle.moves_bottom(),
                (px, py),
                image,
                r,
            );
        }
        return edge_with_ratio(start, handle, (px, py), image, r);
    }
    let (mut left, mut top, mut right, mut bottom) = (start.x, start.y, start.right(), start.bottom());
    if handle.moves_left() {
        left = snap_coordinate(px, image.0).min(right.saturating_sub(min_w));
    }
    if handle.moves_right() {
        right = snap_coordinate(px, image.0).clamp(left.saturating_add(min_w).min(image.0), image.0);
    }
    if handle.moves_top() {
        top = snap_coordinate(py, image.1).min(bottom.saturating_sub(min_h));
    }
    if handle.moves_bottom() {
        bottom = snap_coordinate(py, image.1).clamp(top.saturating_add(min_h).min(image.1), image.1);
    }
    PixelRect::new(left, top, right - left, bottom - top)
}

/// A new rectangle from the press point to the pointer, in whatever direction the pointer went.
pub(super) fn draw(anchor: (u32, u32), (px, py): (f64, f64), image: (u32, u32), ratio: Option<f64>) -> PixelRect {
    // Towards the pointer, unless the press was on the picture's border and there is no room that way.
    let rightwards = (px >= f64::from(anchor.0) && anchor.0 < image.0) || anchor.0 == 0;
    let downwards = (py >= f64::from(anchor.1) && anchor.1 < image.1) || anchor.1 == 0;
    if let Some(r) = ratio {
        return corner_with_ratio(anchor, rightwards, downwards, (px, py), image, r);
    }
    let (min_w, min_h) = min_side(image);
    let x = snap_coordinate(px, image.0);
    let y = snap_coordinate(py, image.1);
    let (left, right) = span(anchor.0, x, rightwards, min_w, image.0);
    let (top, bottom) = span(anchor.1, y, downwards, min_h, image.1);
    PixelRect::new(left, top, right - left, bottom - top)
}

/// The interval from `anchor` towards `to`, at least `min` long, inside `0..=limit`.
fn span(anchor: u32, to: u32, forwards: bool, min: u32, limit: u32) -> (u32, u32) {
    if forwards {
        let end = to.max(anchor.saturating_add(min)).min(limit);
        (end.saturating_sub(min).min(anchor), end)
    } else {
        let start = to.min(anchor.saturating_sub(min));
        (start, start.saturating_add(min).max(anchor).min(limit))
    }
}

/// A rectangle of ratio `r` (width / height) with one corner at `anchor`, growing towards the pointer, as large as the pointer
/// asks (the larger of its two distances decides) and as the picture allows.
fn corner_with_ratio(
    anchor: (u32, u32),
    rightwards: bool,
    downwards: bool,
    (px, py): (f64, f64),
    image: (u32, u32),
    ratio: f64,
) -> PixelRect {
    let (ax, ay) = (f64::from(anchor.0), f64::from(anchor.1));
    let room_x = if rightwards { f64::from(image.0) - ax } else { ax };
    let room_y = if downwards { f64::from(image.1) - ay } else { ay };
    let dx = (if rightwards { px - ax } else { ax - px }).max(0.0);
    let dy = (if downwards { py - ay } else { ay - py }).max(0.0);
    let (min_w, min_h) = min_side(image);
    let max_w = room_x.min(room_y * ratio);
    let min_w = f64::from(min_w).max(f64::from(min_h) * ratio).min(max_w);
    let width = dx.max(dy * ratio).clamp(min_w.max(0.0), max_w.max(0.0));
    let (w, h) = whole_size(width, width / ratio, (room_x, room_y));
    let x = if rightwards {
        anchor.0
    } else {
        anchor.0.saturating_sub(w)
    };
    let y = if downwards {
        anchor.1
    } else {
        anchor.1.saturating_sub(h)
    };
    PixelRect::new(x, y, w, h)
}

/// An edge handle with Shift: the dragged axis follows the pointer, the other axis follows the ratio around its middle.
fn edge_with_ratio(start: PixelRect, handle: Handle, (px, py): (f64, f64), image: (u32, u32), ratio: f64) -> PixelRect {
    let (min_w, min_h) = min_side(image);
    let (iw, ih) = (f64::from(image.0), f64::from(image.1));
    let horizontal = handle.moves_left() || handle.moves_right();
    if horizontal {
        let anchor = if handle.moves_right() { start.x } else { start.right() };
        let room = if handle.moves_right() {
            iw - f64::from(anchor)
        } else {
            f64::from(anchor)
        };
        let reach = if handle.moves_right() {
            px - f64::from(anchor)
        } else {
            f64::from(anchor) - px
        };
        let max_w = room.min(ih * ratio);
        let min = f64::from(min_w).max(f64::from(min_h) * ratio).min(max_w);
        let width = reach.clamp(min.max(0.0), max_w.max(0.0));
        let (w, h) = whole_size(width, width / ratio, (room, ih));
        let x = if handle.moves_right() {
            anchor
        } else {
            anchor.saturating_sub(w)
        };
        let center = f64::from(start.y) + f64::from(start.height) / 2.0;
        let y = (center - f64::from(h) / 2.0)
            .round()
            .clamp(0.0, f64::from(image.1.saturating_sub(h)))
            .to_int::<u32>();
        PixelRect::new(x, y, w, h)
    } else {
        let anchor = if handle.moves_bottom() { start.y } else { start.bottom() };
        let room = if handle.moves_bottom() {
            ih - f64::from(anchor)
        } else {
            f64::from(anchor)
        };
        let reach = if handle.moves_bottom() {
            py - f64::from(anchor)
        } else {
            f64::from(anchor) - py
        };
        let max_h = room.min(iw / ratio);
        let min = f64::from(min_h).max(f64::from(min_w) / ratio).min(max_h);
        let height = reach.clamp(min.max(0.0), max_h.max(0.0));
        let (w, h) = whole_size(height * ratio, height, (iw, room));
        let y = if handle.moves_bottom() {
            anchor
        } else {
            anchor.saturating_sub(h)
        };
        let center = f64::from(start.x) + f64::from(start.width) / 2.0;
        let x = (center - f64::from(w) / 2.0)
            .round()
            .clamp(0.0, f64::from(image.0.saturating_sub(w)))
            .to_int::<u32>();
        PixelRect::new(x, y, w, h)
    }
}

/// Rounds a size to whole pixels, at least one and at most the room.
fn whole_size(w: f64, h: f64, (room_w, room_h): (f64, f64)) -> (u32, u32) {
    let w = w.round().clamp(1.0, room_w.max(1.0)).to_int::<u32>();
    let h = h.round().clamp(1.0, room_h.max(1.0)).to_int::<u32>();
    (w, h)
}
