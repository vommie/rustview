//! Which list positions the tile pool must hold for a viewport (pure).

use std::ops::Range;

/// Rows kept above and below the visible rows of the grid so that ordinary scrolling never shows an empty tile.
pub const GRID_MARGIN_ROWS: usize = 3;
/// The same for the table, whose rows are short.
pub const TABLE_MARGIN_ROWS: usize = 12;
/// Guesses for the very first fill, before the view has reported its real size.
pub const DEFAULT_GRID_COLUMNS: usize = 6;
pub const DEFAULT_VISIBLE_ROWS: usize = 6;

pub fn margin_rows_for(is_table: bool) -> usize {
    if is_table { TABLE_MARGIN_ROWS } else { GRID_MARGIN_ROWS }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewGeometry {
    /// Items per row (1 for the table).
    pub columns: usize,
    /// Rows that intersect the viewport, partially visible ones included.
    pub visible_rows: usize,
    pub margin_rows: usize,
}

impl ViewGeometry {
    pub fn new(columns: usize, visible_rows: usize, margin_rows: usize) -> Self {
        Self {
            columns: columns.max(1),
            visible_rows: visible_rows.max(1),
            margin_rows,
        }
    }

    pub fn pool_rows(&self) -> usize {
        self.visible_rows + 2 * self.margin_rows
    }

    /// Slots the pool needs for a list of `total` items.
    pub fn pool_size(&self, total: usize) -> usize {
        (self.pool_rows() * self.columns).min(total)
    }

    /// The window for a viewport whose first visible row is `first_row`: whole rows, `pool_rows` of them, kept inside the list.
    pub fn window(&self, first_row: usize, total: usize) -> Range<usize> {
        if total == 0 {
            return 0..0;
        }
        let total_rows = total.div_ceil(self.columns);
        let pool_rows = self.pool_rows().min(total_rows);
        let latest_start = total_rows - pool_rows;
        let start_row = first_row.saturating_sub(self.margin_rows).min(latest_start);
        let start = start_row * self.columns;
        let end = ((start_row + pool_rows) * self.columns).min(total);
        start..end
    }
}
