//! Pixel rectangles of the image editor (pure): the crop rectangle, fitting a ratio into a rectangle and aligning a rectangle to
//! the block grid of a JPEG (the condition for a lossless crop).

use super::ratio::AspectRatio;
use crate::util::cast::{Az, FloatToInt};

/// A rectangle in whole pixels of an image (its top left corner and its size).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// The whole image.
    pub const fn full((width, height): (u32, u32)) -> Self {
        Self::new(0, 0, width, height)
    }

    pub const fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub const fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub const fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The rectangle covers the whole image of `size`.
    pub fn is_full(self, size: (u32, u32)) -> bool {
        self == Self::full(size)
    }

    /// Lies completely inside an image of `size`.
    pub fn fits_in(self, (width, height): (u32, u32)) -> bool {
        !self.is_empty() && self.right() <= width && self.bottom() <= height
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && x <= f64::from(self.right())
            && y >= f64::from(self.y)
            && y <= f64::from(self.bottom())
    }

    /// The rectangle moved and, if needed, shrunk until it lies inside an image of `size` (at least one pixel).
    pub fn clamped_into(self, (width, height): (u32, u32)) -> Self {
        let w = self.width.clamp(1, width.max(1));
        let h = self.height.clamp(1, height.max(1));
        Self::new(
            self.x.min(width.saturating_sub(w)),
            self.y.min(height.saturating_sub(h)),
            w,
            h,
        )
    }

    /// `inner` is relative to this rectangle (a crop of the picture this rectangle cut out): the same rectangle in the
    /// coordinates this one is given in.
    pub fn compose(self, inner: Self) -> Self {
        Self::new(
            self.x.saturating_add(inner.x),
            self.y.saturating_add(inner.y),
            inner.width,
            inner.height,
        )
    }
}

/// The largest rectangle of `ratio` that fits into `bounds`, centered in it (whole pixels, at least one pixel per side).
pub fn fit_ratio(bounds: PixelRect, ratio: AspectRatio) -> PixelRect {
    let (bw, bh) = (u64::from(bounds.width), u64::from(bounds.height));
    let (rw, rh) = (u64::from(ratio.width), u64::from(ratio.height));
    if bw == 0 || bh == 0 || rw == 0 || rh == 0 {
        return bounds;
    }
    let (w, h) = if bw * rh >= bh * rw {
        // Limited by the height: the full height, the width rounded to the nearest pixel (never wider than the bounds).
        (rounded_div(bh * rw, rh).clamp(1, bw), bh)
    } else {
        (bw, rounded_div(bw * rh, rw).clamp(1, bh))
    };
    let (w, h) = (narrow(w), narrow(h));
    PixelRect::new(
        bounds.x + (bounds.width - w) / 2,
        bounds.y + (bounds.height - h) / 2,
        w,
        h,
    )
}

fn rounded_div(numerator: u64, denominator: u64) -> u64 {
    (numerator + denominator / 2) / denominator
}

fn narrow(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Whether the top left corner lies on the block grid (both coordinates are multiples of the grid's cell size).
pub fn origin_on_grid(rect: PixelRect, (grid_w, grid_h): (u32, u32)) -> bool {
    grid_w > 0 && grid_h > 0 && rect.x.is_multiple_of(grid_w) && rect.y.is_multiple_of(grid_h)
}

/// Moves the left and the top edge to the nearest grid line while the right and the bottom edge stay where they are; an edge is
/// never pushed past the opposite one (it goes to the grid line before it then).
pub fn align_origin(rect: PixelRect, (grid_w, grid_h): (u32, u32)) -> PixelRect {
    let snap = |start: u32, end: u32, cell: u32| -> u32 {
        if cell <= 1 {
            return start;
        }
        let nearest = rounded_div(u64::from(start), u64::from(cell)) * u64::from(cell);
        let nearest = narrow(nearest);
        if nearest < end {
            nearest
        } else {
            (end.saturating_sub(1) / cell) * cell
        }
    };
    let x = snap(rect.x, rect.right(), grid_w);
    let y = snap(rect.y, rect.bottom(), grid_h);
    PixelRect::new(x, y, rect.right() - x, rect.bottom() - y)
}

/// Moves the whole rectangle (its size stays) so that its top left corner lies on the grid, as close as possible to where it was
/// and still inside an image of `size`.
pub fn align_position(rect: PixelRect, (grid_w, grid_h): (u32, u32), (width, height): (u32, u32)) -> PixelRect {
    let place = |start: u32, length: u32, cell: u32, limit: u32| -> u32 {
        if cell <= 1 {
            return start;
        }
        let max_start = limit.saturating_sub(length);
        let nearest = narrow(rounded_div(u64::from(start), u64::from(cell)) * u64::from(cell));
        if nearest <= max_start {
            nearest
        } else {
            (max_start / cell) * cell
        }
    };
    PixelRect::new(
        place(rect.x, rect.width, grid_w, width),
        place(rect.y, rect.height, grid_h, height),
        rect.width,
        rect.height,
    )
}

/// Rounds a position in image pixels (a pointer, which falls between pixels) to the nearest pixel edge inside `0..=limit`.
pub fn snap_coordinate(value: f64, limit: u32) -> u32 {
    value.round().clamp(0.0, limit.az::<f64>()).to_int::<u32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(w: u32, h: u32) -> AspectRatio {
        AspectRatio::new(w, h).expect("valid ratio")
    }

    #[test]
    fn a_ratio_fills_the_bounds_like_the_reference_pictures() {
        // The three reference shots of the user: a 893 x 1221 picture with "O", "4:5" and "5:4".
        let image = PixelRect::full((893, 1221));
        assert_eq!(fit_ratio(image, ratio(893, 1221)).size(), (893, 1221));
        assert_eq!(fit_ratio(image, ratio(4, 5)).size(), (893, 1116));
        assert_eq!(fit_ratio(image, ratio(5, 4)).size(), (893, 714));
    }

    #[test]
    fn a_fitted_rectangle_is_centered_and_never_leaves_its_bounds() {
        let bounds = PixelRect::new(10, 20, 300, 100);
        let fitted = fit_ratio(bounds, ratio(1, 1));
        assert_eq!(fitted, PixelRect::new(110, 20, 100, 100));
        for (w, h) in [(16, 9), (9, 16), (21, 9), (1, 1), (3, 2), (1000, 1), (1, 1000)] {
            let r = fit_ratio(bounds, ratio(w, h));
            assert!(r.x >= bounds.x && r.y >= bounds.y && r.right() <= bounds.right() && r.bottom() <= bounds.bottom());
            assert!(r.width >= 1 && r.height >= 1);
        }
    }

    #[test]
    fn clamping_moves_first_and_shrinks_only_what_does_not_fit() {
        assert_eq!(
            PixelRect::new(90, 90, 20, 20).clamped_into((100, 100)),
            PixelRect::new(80, 80, 20, 20)
        );
        assert_eq!(
            PixelRect::new(5, 5, 500, 50).clamped_into((100, 100)),
            PixelRect::new(0, 5, 100, 50)
        );
        assert_eq!(PixelRect::new(5, 5, 0, 0).clamped_into((10, 10)).size(), (1, 1));
    }

    #[test]
    fn composed_crops_add_their_offsets() {
        let outer = PixelRect::new(100, 50, 400, 300);
        assert_eq!(
            outer.compose(PixelRect::new(10, 20, 30, 40)),
            PixelRect::new(110, 70, 30, 40)
        );
    }

    #[test]
    fn aligning_the_origin_keeps_the_far_edges() {
        let rect = PixelRect::new(13, 7, 100, 60);
        let aligned = align_origin(rect, (16, 16));
        assert_eq!(aligned, PixelRect::new(16, 0, 97, 67));
        assert!(origin_on_grid(aligned, (16, 16)));
        assert_eq!(aligned.right(), rect.right());
        assert_eq!(aligned.bottom(), rect.bottom());
        // The nearest line may leave a single pixel; a line past the far edge is never taken.
        let tiny = PixelRect::new(30, 30, 3, 3);
        let aligned = align_origin(tiny, (16, 16));
        assert_eq!((aligned.x, aligned.y, aligned.right()), (32, 32, 33));
        let aligned = align_origin(PixelRect::new(40, 40, 3, 3), (16, 16));
        assert_eq!((aligned.x, aligned.y, aligned.right()), (32, 32, 43));
        // A grid of one pixel is no grid.
        assert_eq!(align_origin(rect, (1, 1)), rect);
    }

    #[test]
    fn aligning_the_position_keeps_the_size_and_stays_inside() {
        let image = (100, 100);
        assert_eq!(
            align_position(PixelRect::new(13, 9, 20, 20), (8, 8), image),
            PixelRect::new(16, 8, 20, 20)
        );
        assert_eq!(
            align_position(PixelRect::new(78, 78, 22, 22), (16, 16), image),
            PixelRect::new(64, 64, 22, 22)
        );
    }

    #[test]
    fn pointer_positions_snap_to_pixel_edges_inside_the_image() {
        assert_eq!(snap_coordinate(-3.2, 100), 0);
        assert_eq!(snap_coordinate(10.49, 100), 10);
        assert_eq!(snap_coordinate(10.5, 100), 11);
        assert_eq!(snap_coordinate(150.0, 100), 100);
        assert_eq!(snap_coordinate(f64::NAN, 100), 0);
    }
}
