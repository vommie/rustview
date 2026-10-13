//! Where the media canvas of a tab-grid item is in the window. One definition for the native video windows and the Multiview
//! tiles, so both sit exactly where the Slint item leaves room for them.

use super::geometry::MasonryRect;
use crate::settings::schema::{
    TAB_GRID_CONTENT_TOP_LOGICAL_PX, TAB_GRID_ITEM_HEADER_LOGICAL_PX,
    TAB_GRID_MIN_CANVAS_LOGICAL_PX, TAB_GRID_STATUS_BAR_LOGICAL_PX,
};
use crate::viewer::ViewportBounds;

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
pub fn canvas_rect(
    item: &MasonryRect,
    padding_h: f32,
    padding_v: f32,
    scroll_y: f32,
) -> LogicalRect {
    LogicalRect {
        x: item.x + padding_h,
        y: TAB_GRID_CONTENT_TOP_LOGICAL_PX
            + item.y
            + TAB_GRID_ITEM_HEADER_LOGICAL_PX
            + padding_v
            + scroll_y,
        width: (item.width - padding_h * 2.0).max(TAB_GRID_MIN_CANVAS_LOGICAL_PX),
        height: (item.height - TAB_GRID_ITEM_HEADER_LOGICAL_PX - padding_v * 2.0)
            .max(TAB_GRID_MIN_CANVAS_LOGICAL_PX),
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
        height: (((window_height as f32 / scale) - (content_top + TAB_GRID_STATUS_BAR_LOGICAL_PX))
            * scale)
            .round()
            .max(16.0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(x: f32, y: f32, width: f32, height: f32) -> MasonryRect {
        MasonryRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_canvas_sits_below_the_toolbars_header_and_padding() {
        let c = canvas_rect(&item(12.0, 100.0, 400.0, 300.0), 8.0, 6.0, 0.0);
        assert_eq!((c.x, c.y), (20.0, 64.0 + 100.0 + 26.0 + 6.0));
        assert_eq!((c.width, c.height), (384.0, 300.0 - 26.0 - 12.0));
    }

    #[test]
    fn scrolling_moves_the_canvas_with_the_item() {
        let still = canvas_rect(&item(0.0, 500.0, 300.0, 200.0), 8.0, 6.0, 0.0);
        let scrolled = canvas_rect(&item(0.0, 500.0, 300.0, 200.0), 8.0, 6.0, -120.0);
        assert_eq!(scrolled.y, still.y - 120.0);
        assert_eq!(scrolled.height, still.height);
    }

    #[test]
    fn tiny_items_keep_a_usable_canvas() {
        let c = canvas_rect(&item(0.0, 0.0, 10.0, 10.0), 8.0, 6.0, 0.0);
        assert!(c.width >= 16.0 && c.height >= 16.0);
    }

    #[test]
    fn the_viewport_excludes_the_toolbars_and_the_status_bar() {
        let v = viewport_bounds(1920, 1080, 1.0);
        assert_eq!((v.x, v.y, v.width, v.height), (0, 64, 1920, 1080 - 64 - 28));
        let v = viewport_bounds(2400, 1350, 1.25);
        assert_eq!(v.y, 80);
        assert_eq!(v.width, 2400);
        assert_eq!(v.height, ((1350.0f32 / 1.25 - 92.0) * 1.25).round() as u32);
    }
}
