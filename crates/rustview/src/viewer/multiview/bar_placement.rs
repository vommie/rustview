//! Where the floating action bar goes, and what shape its window has (pure).
//!
//! The bar lives in a small window of its own that floats above the Multiview window: a native video window is a child
//! window of the Multiview window, and nothing drawn inside the parent can blend with it, but a window of its own can be
//! blended with everything below it by the compositing manager. The window is a little larger than the bar so that the
//! tooltip of a button fits below it; the bar sits in its top right corner, and that corner goes into the top right
//! corner of the hovered tile. All rectangles are physical pixels on the screen (root window coordinates).

use super::layout::Rect;
use crate::viewer::XRectangle;
use rustview_core::settings::schema::{
    MULTIVIEW_BAR_MARGIN_LOGICAL_PX, MULTIVIEW_OVERLAY_HEIGHT_LOGICAL_PX, MULTIVIEW_OVERLAY_WIDTH_LOGICAL_PX,
};
use rustview_core::util::cast::{Az, FloatToInt, SaturatingAs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarPlacement {
    /// The overlay window.
    pub window: Rect,
    /// The bar inside it.
    pub bar: Rect,
}

/// The corner of its area a bar sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarCorner {
    TopRight,
    TopLeft,
}

/// Places the bar (`bar_logical` = its size in logical pixels) in the top right corner of `tile`. `None` if the tile is
/// too small to carry the bar with its margin: then there is no bar for that tile.
pub fn place_bar(tile: Rect, bar_logical: (f32, f32), scale: f32) -> Option<BarPlacement> {
    place_bar_in(BarCorner::TopRight, tile, bar_logical, scale)
}

/// Places a bar in `corner` of `area` (the hovered tile for the media bar, the whole Multiview for the primary bar).
pub fn place_bar_in(corner: BarCorner, area: Rect, bar_logical: (f32, f32), scale: f32) -> Option<BarPlacement> {
    let scale = scale.max(0.5);
    let px = |logical: f32| (logical * scale).round().to_int::<i32>();
    let margin = px(MULTIVIEW_BAR_MARGIN_LOGICAL_PX);
    let (bar_w, bar_h) = (px(bar_logical.0), px(bar_logical.1));
    if bar_w <= 0 || bar_h <= 0 || area.w < bar_w + 2 * margin || area.h < bar_h + 2 * margin {
        return None;
    }
    let window_w = px(MULTIVIEW_OVERLAY_WIDTH_LOGICAL_PX).max(bar_w);
    let window_h = px(MULTIVIEW_OVERLAY_HEIGHT_LOGICAL_PX).max(bar_h);
    let window_x = match corner {
        BarCorner::TopRight => area.x + area.w - margin - window_w,
        BarCorner::TopLeft => area.x + margin,
    };
    let window = Rect::new(window_x, area.y + margin, window_w, window_h);
    let bar_x = match corner {
        BarCorner::TopRight => window.x + window_w - bar_w,
        BarCorner::TopLeft => window.x,
    };
    let bar = Rect::new(bar_x, window.y, bar_w, bar_h);
    Some(BarPlacement { window, bar })
}

impl BarPlacement {
    /// The same placement moved by (`dx`, `dy`): from coordinates of the host window to coordinates of the screen.
    pub fn translated(self, dx: i32, dy: i32) -> Self {
        let shift = |r: Rect| Rect::new(r.x + dx, r.y + dy, r.w, r.h);
        Self {
            window: shift(self.window),
            bar: shift(self.bar),
        }
    }
}

/// Cheap superset test that needs no bar size: can the pointer be within `reveal_px` (logical pixels) of the bar in `corner`
/// of `area`? The bar always lies inside the overlay window, which sits in that corner, so the corner region (window size
/// plus margin plus reveal distance) covers every position from which the bar can be revealed.
pub fn near_bar_corner(corner: BarCorner, area: Rect, pointer: (i32, i32), scale: f32, reveal_px: u32) -> bool {
    let scale = scale.max(0.5);
    let px = |logical: f32| (logical * scale).round().to_int::<i32>();
    let reveal = px(reveal_px.az::<f32>());
    let reach_x = px(MULTIVIEW_BAR_MARGIN_LOGICAL_PX + MULTIVIEW_OVERLAY_WIDTH_LOGICAL_PX) + reveal;
    let reach_y = px(MULTIVIEW_BAR_MARGIN_LOGICAL_PX + MULTIVIEW_OVERLAY_HEIGHT_LOGICAL_PX) + reveal;
    let in_x = match corner {
        BarCorner::TopRight => pointer.0 >= area.x + area.w - reach_x,
        BarCorner::TopLeft => pointer.0 <= area.x + reach_x,
    };
    in_x && pointer.1 <= area.y + reach_y
}

/// The pointer is on the bar or within `reveal_px` (logical pixels) of its edge.
pub fn in_reveal_zone(bar: Rect, pointer: (i32, i32), scale: f32, reveal_px: u32) -> bool {
    let reveal = (reveal_px.az::<f32>() * scale.max(0.5)).round().to_int::<i32>();
    Rect::new(bar.x - reveal, bar.y - reveal, bar.w + 2 * reveal, bar.h + 2 * reveal).contains(pointer.0, pointer.1)
}

/// `rect` (relative to its window, so never negative) as the X protocol wants it.
pub fn xrectangle(rect: Rect) -> XRectangle {
    XRectangle {
        x: rect.x.clamp(0, i32::from(i16::MAX)).saturating_as::<i16>(),
        y: rect.y.clamp(0, i32::from(i16::MAX)).saturating_as::<i16>(),
        width: rect.w.clamp(1, i32::from(u16::MAX)).saturating_as::<u16>(),
        height: rect.h.clamp(1, i32::from(u16::MAX)).saturating_as::<u16>(),
    }
}

/// The tab of saved A-B repeats above a control bar (`ui/multiview_video_bar_window.slint`, the strip's `Path`) as rectangles for
/// the X shape extension, one per pixel row: `rect` is the box of the path (its bottom row touches the bar), `flank` how far the
/// concave flank at each side reaches out into the bar, `corner` the radius of the top corners. The curves are the path's own
/// quadratic curves, so the shape matches what is drawn.
pub fn tab_region(rect: Rect, flank: i32, corner: i32) -> Vec<XRectangle> {
    let rows = rect.h.max(1);
    let reach = f64::from(flank.clamp(0, rect.w / 4));
    // The path: the flank from (0, height) over (reach, height) to (reach, round), the corner on to (reach + round, 0).
    let round = f64::from(corner.clamp(0, rows - 1));
    let height = f64::from(rows);
    (0..rows)
        .filter_map(|row| {
            let mid = f64::from(row) + 0.5;
            // Where the tab starts at this height (the right side is the mirror image).
            let inset = if mid < round {
                // The top corner: from (reach, round) over (reach, 0) to (reach + round, 0).
                reach + round * (1.0 - (mid / round).sqrt()).powi(2)
            } else {
                // The flank: from (0, height) over (reach, height) to (reach, round).
                let along = ((height - mid) / (height - round).max(1.0)).clamp(0.0, 1.0).sqrt();
                reach * (1.0 - (1.0 - along).powi(2))
            };
            let inset = inset.round().to_int::<i32>();
            let width = rect.w - 2 * inset;
            (width > 0).then(|| xrectangle(Rect::new(rect.x + inset, rect.y + row, width, 1)))
        })
        .collect()
}

/// `rect` with rounded corners as the list of rectangles the X shape extension wants (one per pixel row of the corners,
/// plus the straight middle). Used when no compositing manager blends the window: the corners must not show as black.
pub fn rounded_region(rect: Rect, radius: i32) -> Vec<XRectangle> {
    let r = radius.clamp(0, rect.w.min(rect.h) / 2);
    let mut rects = Vec::with_capacity(2 * r.saturating_as::<usize>() + 1);
    let mut push = |x: i32, y: i32, w: i32, h: i32| {
        if w > 0 && h > 0 {
            rects.push(XRectangle {
                x: x.clamp(i32::from(i16::MIN), i32::from(i16::MAX)).saturating_as::<i16>(),
                y: y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)).saturating_as::<i16>(),
                width: w.min(i32::from(u16::MAX)).saturating_as::<u16>(),
                height: h.min(i32::from(u16::MAX)).saturating_as::<u16>(),
            });
        }
    };
    for row in 0..r {
        // Distance of the row's center from the center line of the corner circle, in pixels.
        let dy = f64::from(r - row) - 0.5;
        let reach = (f64::from(r * r) - dy * dy).max(0.0).sqrt();
        let inset = r - reach.round().to_int::<i32>();
        push(rect.x + inset, rect.y + row, rect.w - 2 * inset, 1);
        push(rect.x + inset, rect.y + rect.h - 1 - row, rect.w - 2 * inset, 1);
    }
    push(rect.x, rect.y + r, rect.w, rect.h - 2 * r);
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_flares_out_at_the_bottom_and_rounds_at_the_top() {
        let rows = tab_region(Rect::new(10, 0, 200, 12), 12, 6);
        assert_eq!(rows.len(), 12);
        let width = |r: &XRectangle| i32::from(r.width);
        // Top row: inside the flanks and corners; bottom row: the flanks reach out half way in their last pixel row.
        assert!(width(&rows[0]) < 200 - 2 * 12);
        assert!(width(&rows[11]) >= 200 - 12);
        assert!(
            rows.windows(2).all(|pair| width(&pair[0]) <= width(&pair[1])),
            "it only grows downward"
        );
        assert!(
            rows.iter().all(|r| i32::from(r.x) - 10 == (200 - width(r)) / 2),
            "symmetric"
        );
    }
}
