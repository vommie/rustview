//! State questions and view-level events of the Browse list: scan/busy state, grid/table toggle and viewport changes.

use super::BridgeMedia;
use crate::ui::MainWindow;
use crate::ui::browse_window::{GRID_MARGIN_ROWS, TABLE_MARGIN_ROWS, ViewGeometry, sync_window, timed};
use crate::ui::state::UiState;
use parking_lot::RwLock;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::SaturatingAs;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

impl BridgeMedia {
    /// A scan, refresh or indexing run is going on (the thumbnail pipeline is not counted). Operations that move files must not run
    /// next to it: the scan would index files while they change place.
    pub fn is_scan_running(window: &MainWindow, state: &RwLock<UiState>) -> bool {
        if let Some(st) = state.try_read()
            && (st.is_refreshing_media || st.is_refreshing_folders)
        {
            return true;
        }
        window.get_is_indexing_media() || window.get_is_refreshing_folders()
    }

    pub fn is_system_busy(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>) -> bool {
        if let Some(st) = state.try_read()
            && (st.is_refreshing_media || st.is_refreshing_folders)
        {
            return true;
        }

        if window.get_is_indexing_media() || window.get_is_refreshing_folders() {
            return true;
        }

        let progress = thumbnails.progress();
        progress.is_preloading || progress.is_generating || progress.active_workers > 0
    }

    pub fn handle_view_mode_toggled(
        is_table: bool,
        anchor_item_idx: usize,
        window: &MainWindow,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t0 = Instant::now();

        {
            let mut app_settings = settings.get();
            app_settings.view.is_table_view = is_table;
            settings.save_quiet(&app_settings);
        }

        {
            let mut st = state.write();
            st.reset_view_window();
            st.scroll_bench.is_table_view.store(is_table, Ordering::Relaxed);
        }

        window.set_is_table_view(is_table);
        window.invoke_scroll_media_to_index(anchor_item_idx.saturating_as::<i32>(), is_table);

        Self::refill_view(window, thumbnails, state, anchor_item_idx);

        let elapsed_us = t0.elapsed().as_micros();
        tracing::info!(
            target_view = if is_table { "TableView" } else { "GridView" },
            anchor_index = anchor_item_idx,
            transition_time_us = elapsed_us,
            "[ViewTransition: Sync] View mode transition completed with scroll synchronization"
        );
    }

    /// The viewport of the grid or the table moved or changed size: keeps the tiles around it filled.
    pub fn handle_viewport_changed(
        first_row: usize,
        visible_rows: usize,
        columns: usize,
        total: usize,
        window: &MainWindow,
        state: &RwLock<UiState>,
        thumbnails: &ThumbnailManager,
    ) {
        if total == 0 {
            return;
        }

        let is_table = window.get_is_table_view();
        let margin = if is_table { TABLE_MARGIN_ROWS } else { GRID_MARGIN_ROWS };
        let geometry = ViewGeometry::new(columns, visible_rows, margin);

        let mut st = state.write();
        let scroll_bench = st.scroll_bench.clone();
        scroll_bench.record_event();
        scroll_bench.is_table_view.store(is_table, Ordering::Relaxed);

        let unchanged = st.view_geometry == Some(geometry)
            && st.ring.range() == geometry.window(first_row, total)
            && st.ring.pool() == geometry.pool_size(total);
        if unchanged {
            scroll_bench.record_hysteresis_hit();
            return;
        }

        let (outcome, slice_us) = timed(|| sync_window(window, &mut st, thumbnails, geometry, first_row, total, false));
        scroll_bench.record_slice_reload(slice_us);
        tracing::trace!(
            first_row,
            rows_written = outcome.rows_written,
            rebuilt = outcome.rebuilt,
            requested = outcome.requested,
            elapsed_us = slice_us,
            "[BrowseWindow: Sync] Tiles updated for the viewport"
        );
    }
}
