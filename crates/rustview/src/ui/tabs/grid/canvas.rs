//! Where the media canvas of a tab-grid item is in the window. One definition for the native video windows and the Multiview
//! tiles, so both sit exactly where the Slint item leaves room for them.

use super::geometry::MasonryRect;
use crate::viewer::ViewportBounds;
use rustview_core::settings::schema::{
    TAB_GRID_CONTENT_TOP_LOGICAL_PX, TAB_GRID_ITEM_HEADER_LOGICAL_PX, TAB_GRID_MIN_CANVAS_LOGICAL_PX,
    TAB_GRID_STATUS_BAR_LOGICAL_PX,
};

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
pub fn viewport_bounds(window_width: u32, window_height: u32, scale: f32) -> ViewportBounds {
    let viewport_w = (window_width as f32 / scale).max(100.0);
    let content_top = TAB_GRID_CONTENT_TOP_LOGICAL_PX;
    ViewportBounds {
        x: 0,
        y: (content_top * scale).round() as i32,
        width: (viewport_w * scale).round().max(16.0) as u32,
        height: (((window_height as f32 / scale) - (content_top + TAB_GRID_STATUS_BAR_LOGICAL_PX)) * scale)
            .round()
            .max(16.0) as u32,
    }
}
