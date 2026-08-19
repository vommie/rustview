use super::MainWindow;
use super::bridge_state::UiState;
use crate::telemetry::BenchmarkReportFormatter;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{Image, Model, Timer};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

/// Subsystem managing background thumbnail worker draining and UI status bar reflection.
pub struct BridgeThumbnails;

impl BridgeThumbnails {
    /// Starts high-performance 60 FPS repeated timer to drain pre-converted SIMD thumbnails into RAM cache and visible rows.
    pub fn start_drain_timer(
        thumbnails: Arc<ThumbnailManager>,
        state: Arc<RwLock<UiState>>,
        window_weak: slint::Weak<MainWindow>,
    ) -> Timer {
        let timer = Timer::default();
        let thumb_ref = thumbnails.clone();
        let st_ref = state.clone();

        // 16ms cadence (~60 FPS) with dynamic adaptive batch sizing
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(16),
            move || {
                let Some(window) = window_weak.upgrade() else {
                    return;
                };

                let t_drain_start = Instant::now();
                let batch = thumb_ref.drain_ready_batch(512);

                let mut row_set_us = 0u64;
                let items_drained = batch.len();
                let is_table = window.get_is_table_view();

                if !batch.is_empty() {
                    let mut st = st_ref.write();
                    let media_model = window.get_media_items();
                    let start_window = window.get_visible_start_index() as usize;
                    let visible_count = media_model.row_count();

                    for item_data in batch {
                        if !st.is_query_active
                            && st.current_folder_id != Some(item_data.folder_id)
                        {
                            continue;
                        }

                        // Store pre-converted SharedPixelBuffer into bounded LRU cache
                        st.folder_thumbnails.insert(item_data.media_id, item_data.pixel_buffer.clone());

                        // Table View Optimization: Skip GUI row mutation churn if not viewing grid thumbnails
                        if is_table {
                            continue;
                        }

                        let t_row = Instant::now();

                        // Exact Local Viewport Translation: Maps global record index to Slint's active window slice
                        if let Some(&global_row_idx) = st.media_row_map.get(&item_data.media_id)
                            && global_row_idx >= start_window
                            && global_row_idx < start_window + visible_count
                        {
                            let local_idx = global_row_idx - start_window;
                            if let Some(mut item) = media_model.row_data(local_idx)
                                && item.id == item_data.media_id as i32
                            {
                                item.thumbnail = Image::from_rgb8(item_data.pixel_buffer);
                                media_model.set_row_data(local_idx, item);
                            }
                        }
                        row_set_us += t_row.elapsed().as_micros() as u64;
                    }
                }

                let total_drain_us = t_drain_start.elapsed().as_micros() as u64;

                if items_drained > 0
                    && let Some(bench) = thumb_ref.active_benchmark()
                {
                    bench.record_drain_cycle(
                        items_drained,
                        row_set_us,
                        total_drain_us,
                    );
                }

                let progress = thumb_ref.progress();
                window.set_is_preloading(progress.is_preloading);
                window.set_preload_current(progress.preload_current as i32);
                window.set_preload_total(progress.preload_total as i32);

                window.set_is_generating(progress.is_generating);
                window.set_generate_current(progress.generate_current as i32);
                window.set_generate_total(progress.generate_total as i32);
                window.set_active_workers(progress.active_workers as i32);

                // Final Benchmark Completion Check for P0 Initial Viewport Loading
                if let Some(bench) = thumb_ref.active_benchmark() {
                    let total_targets = bench.total_targets.load(Ordering::Acquire);
                    let drained_items = bench.drained_items.load(Ordering::Acquire);

                    let is_active_started = progress.preload_total > 0 || progress.generate_total > 0;
                    let is_done = total_targets > 0
                        && ((drained_items >= total_targets)
                            || (is_active_started
                                && progress.preload_current >= progress.preload_total
                                && progress.generate_current >= progress.generate_total
                                && progress.active_workers == 0
                                && thumb_ref.is_drain_queue_empty()));

                    if is_done
                        && !bench.is_finalized.swap(true, Ordering::SeqCst)
                    {
                        let folder_bench = st_ref.read().active_folder_bench.clone();
                        let storage_stats = thumb_ref.storage_stats();
                        let summary_str = BenchmarkReportFormatter::render_summary(
                            folder_bench.as_deref(),
                            &bench,
                            &storage_stats,
                        );
                        tracing::info!("{}", summary_str);
                    }
                }
            },
        );
        timer
    }
}
