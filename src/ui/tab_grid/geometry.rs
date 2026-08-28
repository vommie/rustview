use crate::settings::{MIN_TAB_GRID_COLUMNS, MAX_TAB_GRID_COLUMNS};

/// Responsive width breakpoints for automatic column determination in the TabGrid Masonry view.
pub struct ResponsiveBreakpoints;

impl ResponsiveBreakpoints {
    /// Calculates the optimal column count based on available viewport width.
    /// Values are tuned for modern ultra-wide, 4K, 1440p, 1080p, and compact laptop screens.
    pub fn compute_auto_columns(viewport_width: f32) -> usize {
        let cols = if viewport_width < 620.0 {
            1
        } else if viewport_width < 960.0 {
            2
        } else if viewport_width < 1360.0 {
            3
        } else if viewport_width < 1780.0 {
            4
        } else if viewport_width < 2240.0 {
            5
        } else if viewport_width < 2750.0 {
            6
        } else if viewport_width < 3300.0 {
            7
        } else {
            8
        };

        cols.clamp(MIN_TAB_GRID_COLUMNS, MAX_TAB_GRID_COLUMNS)
    }
}

/// Placement calculation output for Masonry positioning.
#[derive(Debug, Clone, PartialEq)]
pub struct MasonryRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Greedy shortest-column Masonry Layout Engine.
/// Computes non-overlapping item coordinates preserving each medium's exact original aspect ratio.
pub struct TabGridMasonryEngine;

impl TabGridMasonryEngine {
    /// Computes individual tile bounding rectangles and total container scroll height.
    pub fn compute_layout(
        items_aspect_ratios: &[f32],
        viewport_width: f32,
        columns_count: usize,
        grid_padding: f32,
        grid_spacing: f32,
        item_padding_v: f32,
        header_height: f32,
    ) -> (Vec<MasonryRect>, f32) {
        let total_items = items_aspect_ratios.len();
        if total_items == 0 {
            return (Vec::new(), 0.0);
        }

        let num_cols = columns_count.clamp(MIN_TAB_GRID_COLUMNS, MAX_TAB_GRID_COLUMNS);
        let usable_width = (viewport_width - (grid_padding * 2.0) - (grid_spacing * (num_cols.saturating_sub(1) as f32))).max(80.0);
        let column_width = (usable_width / num_cols as f32).max(60.0);

        let mut column_heights = vec![0.0f32; num_cols];
        let mut results = Vec::with_capacity(total_items);

        for &ar in items_aspect_ratios {
            let safe_ar = ar.clamp(0.28, 3.5);
            let media_canvas_height = (column_width / safe_ar).round();
            let total_tile_height = media_canvas_height + (item_padding_v * 2.0) + header_height;

            // Greedily find the column with the shortest current height
            let mut min_col = 0;
            let mut min_h = column_heights[0];
            for (col_idx, &h) in column_heights.iter().enumerate().skip(1) {
                if h < min_h {
                    min_h = h;
                    min_col = col_idx;
                }
            }

            let item_x = grid_padding + (min_col as f32 * (column_width + grid_spacing));
            let item_y = grid_padding + min_h;

            results.push(MasonryRect {
                x: item_x,
                y: item_y,
                width: column_width,
                height: total_tile_height,
            });

            column_heights[min_col] += total_tile_height + grid_spacing;
        }

        let max_col_h = column_heights.iter().copied().fold(0.0f32, f32::max);
        let total_content_height = max_col_h + grid_padding;

        (results, total_content_height)
    }
}
