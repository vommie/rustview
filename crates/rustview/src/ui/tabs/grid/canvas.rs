//! Where the media canvas of a tab-grid item is in the window, so the Multiview tiles sit exactly where the Slint item leaves room
//! for them.

use super::geometry::MasonryRect;
use crate::viewer::multiview::layout::Rect;
use rustview_core::settings::schema::{
    TAB_GRID_CONTENT_TOP_LOGICAL_PX, TAB_GRID_ITEM_HEADER_LOGICAL_PX, TAB_GRID_MIN_CANVAS_LOGICAL_PX,
    TAB_GRID_STATUS_BAR_LOGICAL_PX,
};
use rustview_core::util::cast::{Az, FloatToInt};

/// A rectangle in logical window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The media canvas of the grid item `item` (grid coordinates) in window coordinates: below the toolbars, the header of the
/// item and its padding, shifted by the scroll offset.
pub fn canvas_rect(item: &MasonryRect, padding_h: f32, padding_v: f32, scroll_y: f32) -> LogicalRect {
    LogicalRect {
        x: item.x + padding_h,
        y: TAB_GRID_CONTENT_TOP_LOGICAL_PX + item.y + TAB_GRID_ITEM_HEADER_LOGICAL_PX + padding_v + scroll_y,
        width: (item.width - padding_h * 2.0).max(TAB_GRID_MIN_CANVAS_LOGICAL_PX),
        height: (item.height - TAB_GRID_ITEM_HEADER_LOGICAL_PX - padding_v * 2.0).max(TAB_GRID_MIN_CANVAS_LOGICAL_PX),
    }
}

/// The visible part of the grid in physical window pixels (both sidebars are hidden in the grid).
pub fn visible_grid(window_width: u32, window_height: u32, scale: f32) -> Rect {
    let viewport_w = (window_width.az::<f32>() / scale).max(100.0);
    let content_top = TAB_GRID_CONTENT_TOP_LOGICAL_PX;
    Rect::new(
        0,
        (content_top * scale).round().to_int::<i32>(),
        (viewport_w * scale).round().max(16.0).to_int::<i32>(),
        ((((window_height.az::<f32>() / scale) - (content_top + TAB_GRID_STATUS_BAR_LOGICAL_PX)) * scale)
            .round()
            .max(16.0))
        .to_int::<i32>(),
    )
}
