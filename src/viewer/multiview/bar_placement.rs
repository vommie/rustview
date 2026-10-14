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
