//! Where the floating action bar goes, and what shape its window has (pure).
//!
//! The bar lives in a small window of its own that floats above the Multiview window: a native video window is a child
//! window of the Multiview window, and nothing drawn inside the parent can blend with it, but a window of its own can be
//! blended with everything below it by the compositing manager. The window is a little larger than the bar so that the
//! tooltip of a button fits below it; the bar sits in its top right corner, and that corner goes into the top right
//! corner of the hovered tile. All rectangles are physical pixels on the screen (root window coordinates).

use super::layout::Rect;
use rustview_core::settings::schema::{
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
    let px = |logical: f32| (logical * scale).round() as i32;
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
    let px = |logical: f32| (logical * scale).round() as i32;
    let reveal = px(reveal_px as f32);
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
    let reveal = (reveal_px as f32 * scale.max(0.5)).round() as i32;
    Rect::new(
        bar.x - reveal,
        bar.y - reveal,
        bar.w + 2 * reveal,
        bar.h + 2 * reveal,
    )
    .contains(pointer.0, pointer.1)
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
