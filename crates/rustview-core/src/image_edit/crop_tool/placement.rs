//! Where the ratio button puts the crop rectangle (pure). The size keeps the area of the rectangle the user made last (drawn,
//! resized, the start selection), so a ratio only changes the shape: a 3:2 selection becomes a 2:3 one of the same area, never a
//! sliver of it, and it is reduced only as far as the picture needs. The position keeps to where the user put the rectangle: an
//! edge of the picture it touches stays touched (a rectangle in a corner stays in that corner), otherwise its center stays.
//! Clicking through the ratios and back gives the same rectangles again: nothing drifts.

use super::super::geometry::{PixelRect, fit_ratio};
use super::super::ratio::AspectRatio;
use crate::util::cast::{Az, FloatToInt};

/// Where a rectangle keeps to on one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Anchor {
    /// At the left (top) edge of the picture.
    Start,
    /// At the right (bottom) edge of the picture.
    End,
    /// Around this center (image pixels).
    Center(f64),
}

impl Anchor {
    fn of(start: u32, length: u32, limit: u32) -> Self {
        let end = start.saturating_add(length);
        match (start == 0, end >= limit) {
            (true, true) => Self::Center(f64::from(limit) / 2.0),
            (true, false) => Self::Start,
            (false, true) => Self::End,
            (false, false) => Self::Center(f64::from(start) + f64::from(length) / 2.0),
        }
    }

    fn start(self, length: u32, limit: u32) -> u32 {
        let free = limit.saturating_sub(length);
        match self {
            Self::Start => 0,
            Self::End => free,
            Self::Center(center) => (center - f64::from(length) / 2.0)
                .round()
                .clamp(0.0, free.az::<f64>())
                .to_int::<u32>(),
        }
    }
}

/// The size and the anchors the ratio button works with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Placement {
    /// The area to keep (square image pixels).
    area: f64,
    x: Anchor,
    y: Anchor,
}

impl Placement {
    /// The user made `rect` (drew it, resized it, or it is where the tool started).
    pub(super) fn of(rect: PixelRect, image: (u32, u32)) -> Self {
        Self {
            area: f64::from(rect.width) * f64::from(rect.height),
            x: Anchor::of(rect.x, rect.width, image.0),
            y: Anchor::of(rect.y, rect.height, image.1),
        }
    }

    /// The user moved the rectangle to `rect`: the area stays, the anchors follow it.
    pub(super) fn moved(self, rect: PixelRect, image: (u32, u32)) -> Self {
        Self {
            x: Anchor::of(rect.x, rect.width, image.0),
            y: Anchor::of(rect.y, rect.height, image.1),
            ..self
        }
    }

    /// The rectangle of `ratio`: the kept area in that shape (at most as large as the picture allows), at the anchors.
    pub(super) fn place(self, ratio: AspectRatio, image: (u32, u32)) -> PixelRect {
        let (rw, rh) = (f64::from(ratio.width), f64::from(ratio.height));
        let width = (self.area * rw / rh).sqrt().round();
        let height = (width * rh / rw).round();
        let (width, height) = (width.max(1.0).to_int::<u32>(), height.max(1.0).to_int::<u32>());
        let (width, height) = if width > image.0 || height > image.1 {
            fit_ratio(PixelRect::full(image), ratio).size()
        } else {
            (width, height)
        };
        PixelRect::new(
            self.x.start(width, image.0),
            self.y.start(height, image.1),
            width,
            height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(w: u32, h: u32) -> AspectRatio {
        AspectRatio::new(w, h).expect("valid ratio")
    }

    #[test]
    fn the_whole_picture_gives_the_largest_rectangle_in_the_middle() {
        let image = (1000, 800);
        let place = Placement::of(PixelRect::full(image), image);
        assert_eq!(place.place(ratio(1, 1), image), PixelRect::new(100, 0, 800, 800));
        assert_eq!(place.place(ratio(16, 9), image), PixelRect::new(0, 119, 1000, 563));
        assert_eq!(place.place(ratio(9, 16), image), PixelRect::new(275, 0, 450, 800));
    }

    #[test]
    fn a_free_rectangle_keeps_its_area_and_its_center() {
        let image = (1000, 1000);
        let place = Placement::of(PixelRect::new(300, 400, 300, 200), image);
        let portrait = place.place(ratio(2, 3), image);
        assert_eq!(portrait, PixelRect::new(350, 350, 200, 300));
        let square = place.place(ratio(1, 1), image);
        assert_eq!(square.size(), (245, 245));
        assert_eq!((square.x + square.width / 2, square.y + square.height / 2), (450, 500));
    }

    #[test]
    fn a_rectangle_in_a_corner_stays_in_that_corner() {
        let image = (1000, 800);
        for (rect, corner) in [
            (PixelRect::new(0, 0, 300, 200), (true, true)),
            (PixelRect::new(700, 0, 300, 200), (false, true)),
            (PixelRect::new(0, 600, 300, 200), (true, false)),
            (PixelRect::new(700, 600, 300, 200), (false, false)),
        ] {
            let place = Placement::of(rect, image);
            for r in [ratio(1, 1), ratio(9, 16), ratio(21, 9)] {
                let placed = place.place(r, image);
                assert_eq!(placed.x == 0, corner.0, "{rect:?} -> {placed:?}");
                assert_eq!(placed.right() == 1000, !corner.0, "{rect:?} -> {placed:?}");
                assert_eq!(placed.y == 0, corner.1, "{rect:?} -> {placed:?}");
                assert_eq!(placed.bottom() == 800, !corner.1, "{rect:?} -> {placed:?}");
            }
        }
    }

    #[test]
    fn a_rectangle_at_an_edge_stays_at_it_and_keeps_its_place_along_it() {
        let image = (1000, 800);
        let place = Placement::of(PixelRect::new(400, 0, 200, 100), image);
        let placed = place.place(ratio(1, 1), image);
        assert_eq!(placed.y, 0);
        assert_eq!(placed.x + placed.width / 2, 500);
    }

    #[test]
    fn a_shape_the_picture_cannot_hold_shrinks_only_as_far_as_needed() {
        let image = (1000, 400);
        let place = Placement::of(PixelRect::new(100, 50, 600, 300), image);
        let tall = place.place(ratio(1, 2), image);
        assert_eq!(tall.size(), (200, 400), "the full height, not a sliver of the area");
        assert!(tall.fits_in(image));
    }

    #[test]
    fn going_through_the_ratios_and_back_gives_the_same_rectangle() {
        let image = (1200, 900);
        let start = PixelRect::new(820, 40, 360, 240);
        let place = Placement::of(start, image);
        let first = place.place(ratio(3, 2), image);
        for r in [ratio(1, 1), ratio(9, 16), ratio(21, 9), ratio(4, 5)] {
            assert!(place.place(r, image).fits_in(image));
        }
        assert_eq!(place.place(ratio(3, 2), image), first);
        assert_eq!(first, start, "the rectangle's own ratio gives it back unchanged");
    }

    #[test]
    fn moving_keeps_the_area_and_takes_the_new_anchors() {
        let image = (1000, 800);
        let place = Placement::of(PixelRect::new(100, 100, 400, 400), image);
        let moved = place.moved(PixelRect::new(600, 400, 400, 400), image);
        let placed = moved.place(ratio(16, 9), image);
        assert_eq!((placed.right(), placed.bottom()), (1000, 800));
        assert_eq!(placed.size(), (533, 300));
    }
}
