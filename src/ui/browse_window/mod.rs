//! Windowing of the Browse view (grid and table): which list positions are alive as Slint tiles and how a scroll step updates them.

pub mod geometry;
pub mod ring;
pub mod rows;
pub mod view_builder;
pub mod window_sync;

pub use geometry::{
    DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, GRID_MARGIN_ROWS, TABLE_MARGIN_ROWS, ViewGeometry, margin_rows_for,
};
pub use ring::RingWindow;
pub use rows::{slot_of_media, update_tile};
pub use view_builder::{apply_record, build_view};
pub use window_sync::{SyncOutcome, sync_window, timed};
