//! Where the floating action bar goes, and what shape its window has (pure).
//!
//! The bar lives in a small window of its own that floats above the Multiview window: a native video window is a child
//! window of the Multiview window, and nothing drawn inside the parent can blend with it, but a window of its own can be
//! blended with everything below it by the compositing manager. The window is a little larger than the bar so that the
//! tooltip of a button fits below it; the bar sits in its top right corner, and that corner goes into the top right
//! corner of the hovered tile. All rectangles are physical pixels on the screen (root window coordinates).

use super::layout::Rect;
use crate::settings::schema::{
    MULTIVIEW_BAR_MARGIN_LOGICAL_PX, MULTIVIEW_OVERLAY_HEIGHT_LOGICAL_PX,
    MULTIVIEW_OVERLAY_WIDTH_LOGICAL_PX,
};
use crate::viewer::XRectangle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarPlacement {
    /// The overlay window.
    pub window: Rect,
    /// The bar inside it.
    pub bar: Rect,
}

/// Places the bar (`bar_logical` = its size in logical pixels) in the top right corner of `tile`. `None` if the tile is
/// too small to carry the bar with its margin: then there is no bar for that tile.
pub fn place_bar(tile: Rect, bar_logical: (f32, f32), scale: f32) -> Option<BarPlacement> {
    let scale = scale.max(0.5);
    let px = |logical: f32| (logical * scale).round() as i32;
    let margin = px(MULTIVIEW_BAR_MARGIN_LOGICAL_PX);
    let (bar_w, bar_h) = (px(bar_logical.0), px(bar_logical.1));
    if bar_w <= 0 || bar_h <= 0 || tile.w < bar_w + 2 * margin || tile.h < bar_h + 2 * margin {
        return None;
    }
    let window_w = px(MULTIVIEW_OVERLAY_WIDTH_LOGICAL_PX).max(bar_w);
    let window_h = px(MULTIVIEW_OVERLAY_HEIGHT_LOGICAL_PX).max(bar_h);
    let window = Rect::new(tile.x + tile.w - margin - window_w, tile.y + margin, window_w, window_h);
    let bar = Rect::new(window.x + window_w - bar_w, window.y, bar_w, bar_h);
    Some(BarPlacement { window, bar })
}

/// `rect` with rounded corners as the list of rectangles the X shape extension wants (one per pixel row of the corners,
/// plus the straight middle). Used when no compositing manager blends the window: the corners must not show as black.
pub fn rounded_region(rect: Rect, radius: i32) -> Vec<XRectangle> {
    let r = radius.clamp(0, rect.w.min(rect.h) / 2);
    let mut rects = Vec::with_capacity(2 * r as usize + 1);
    let mut push = |x: i32, y: i32, w: i32, h: i32| {
        if w > 0 && h > 0 {
            rects.push(XRectangle {
                x: x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                y: y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                width: w.min(u16::MAX as i32) as u16,
                height: h.min(u16::MAX as i32) as u16,
            });
        }
    };
    for row in 0..r {
        // Distance of the row's center from the center line of the corner circle, in pixels.
        let dy = f64::from(r - row) - 0.5;
        let reach = (f64::from(r * r) - dy * dy).max(0.0).sqrt();
        let inset = r - reach.round() as i32;
        push(rect.x + inset, rect.y + row, rect.w - 2 * inset, 1);
        push(rect.x + inset, rect.y + rect.h - 1 - row, rect.w - 2 * inset, 1);
    }
    push(rect.x, rect.y + r, rect.w, rect.h - 2 * r);
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covered(rects: &[XRectangle], x: i32, y: i32) -> bool {
        rects.iter().any(|r| {
            x >= i32::from(r.x)
                && x < i32::from(r.x) + i32::from(r.width)
                && y >= i32::from(r.y)
                && y < i32::from(r.y) + i32::from(r.height)
        })
    }

    #[test]
    fn the_bar_goes_into_the_top_right_corner_of_the_tile() {
        let tile = Rect::new(100, 50, 800, 450);
        let placed = place_bar(tile, (100.0, 34.0), 1.0).expect("the tile is large enough");
        assert_eq!(placed.bar, Rect::new(100 + 800 - 8 - 100, 50 + 8, 100, 34));
        assert_eq!(placed.window.y, placed.bar.y, "the window starts at the top of the bar");
        assert_eq!(placed.window.x + placed.window.w, placed.bar.x + placed.bar.w, "right edges agree");
        assert_eq!((placed.window.w, placed.window.h), (360, 88));
    }

    #[test]
    fn everything_scales_with_the_display() {
        let tile = Rect::new(0, 0, 2000, 1000);
        let placed = place_bar(tile, (100.0, 34.0), 1.5).expect("large enough");
        assert_eq!((placed.bar.w, placed.bar.h), (150, 51));
        assert_eq!(placed.bar.x + placed.bar.w, 2000 - 12, "the margin is 8 logical pixels");
        assert_eq!(placed.bar.y, 12);
        assert_eq!((placed.window.w, placed.window.h), (540, 132));
    }

    #[test]
    fn a_tile_that_cannot_carry_the_bar_gets_none() {
        let bar = (100.0, 34.0);
        assert!(place_bar(Rect::new(0, 0, 115, 400), bar, 1.0).is_none(), "too narrow");
        assert!(place_bar(Rect::new(0, 0, 400, 49), bar, 1.0).is_none(), "too low");
        assert!(place_bar(Rect::new(0, 0, 116, 50), bar, 1.0).is_some(), "just enough");
        assert!(place_bar(Rect::new(0, 0, 400, 400), (0.0, 0.0), 1.0).is_none(), "an empty bar is no bar");
    }

    #[test]
    fn a_narrow_tile_lets_the_transparent_window_reach_over_its_neighbors() {
        let placed = place_bar(Rect::new(500, 0, 200, 300), (100.0, 34.0), 1.0).expect("fits");
        assert!(placed.window.x < 500, "the window is wider than the tile; only the bar takes input");
        assert_eq!(placed.bar.x + placed.bar.w, 700 - 8);
    }

    #[test]
    fn rounded_corners_are_cut_and_the_middle_is_kept() {
        let rect = Rect::new(0, 0, 100, 34);
        let region = rounded_region(rect, 8);
        assert!(!covered(&region, 0, 0), "the corner pixel is outside");
        assert!(!covered(&region, 99, 0));
        assert!(!covered(&region, 0, 33));
        assert!(!covered(&region, 99, 33));
        assert!(covered(&region, 50, 0), "the straight top edge is inside");
        assert!(covered(&region, 0, 17), "the straight left edge is inside");
        assert!(covered(&region, 8, 8), "the point where the arc starts is inside");
        for y in 0..34 {
            for x in 0..100 {
                let outside = !covered(&region, x, y);
                let in_corner = !(8..92).contains(&x) && !(8..26).contains(&y);
                assert!(!outside || in_corner, "({x}, {y}) is cut away outside a corner");
            }
        }
    }

    #[test]
    fn a_region_is_symmetric_and_never_leaves_its_rectangle() {
        let rect = Rect::new(20, 40, 61, 27);
        let region = rounded_region(rect, 10);
        for y in 0..27 {
            for x in 0..61 {
                assert_eq!(
                    covered(&region, 20 + x, 40 + y),
                    covered(&region, 20 + 60 - x, 40 + y),
                    "left/right mirror at ({x}, {y})"
                );
                assert_eq!(
                    covered(&region, 20 + x, 40 + y),
                    covered(&region, 20 + x, 40 + 26 - y),
                    "top/bottom mirror at ({x}, {y})"
                );
            }
        }
        for r in &region {
            assert!(i32::from(r.x) >= 20 && i32::from(r.y) >= 40);
            assert!(i32::from(r.x) + i32::from(r.width) <= 81);
            assert!(i32::from(r.y) + i32::from(r.height) <= 67);
        }
    }

    #[test]
    fn a_radius_larger_than_the_rectangle_and_zero_radius_are_safe() {
        assert!(!rounded_region(Rect::new(0, 0, 10, 10), 100).is_empty());
        let square = rounded_region(Rect::new(5, 5, 30, 20), 0);
        assert_eq!(square.len(), 1);
        assert_eq!((square[0].width, square[0].height), (30, 20));
        assert!(rounded_region(Rect::new(0, 0, 0, 0), 8).is_empty());
    }
}
