use crate::util::cast::Az;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

mod report;
pub use report::BenchmarkReportFormatter;
pub use report::format_bytes_metric;
pub use report::format_count;
pub use report::format_duration_us;

/// System RAM metrics reading directly from Arch Linux /proc pseudo-filesystem.
#[derive(Debug, Clone, Default)]
pub struct SystemMemoryMetrics {
    pub process_rss_mb: f64,
    pub process_vms_mb: f64,
    pub system_available_mb: f64,
    pub system_total_mb: f64,
}

pub fn get_system_memory_metrics() -> SystemMemoryMetrics {
    use procfs::{Current, Meminfo, process::Process};
    const MB: f64 = 1024.0 * 1024.0;

    let mut metrics = SystemMemoryMetrics::default();

    if let Ok(statm) = Process::myself().and_then(|process| process.statm()) {
        let page_size = procfs::page_size().az::<f64>();
        metrics.process_vms_mb = statm.size.az::<f64>() * page_size / MB;
        metrics.process_rss_mb = statm.resident.az::<f64>() * page_size / MB;
    }

    if let Ok(meminfo) = Meminfo::current() {
        metrics.system_total_mb = meminfo.mem_total.az::<f64>() / MB;
        metrics.system_available_mb = meminfo.mem_available.unwrap_or(0).az::<f64>() / MB;
    }

    metrics
}

/// Thread-safe atomic metrics accumulator recording view scrolling and virtualization telemetry.
#[derive(Debug)]
pub struct ViewScrollBenchmark {
    pub is_table_view: AtomicBool,
    pub scroll_events: AtomicUsize,
    pub hysteresis_hits: AtomicUsize,
    pub slice_reloads: AtomicUsize,
    pub total_slice_time_us: AtomicU64,
    pub min_slice_time_us: AtomicU64,
    pub max_slice_time_us: AtomicU64,
    pub last_logged_instant: parking_lot::Mutex<Instant>,
}

impl Default for ViewScrollBenchmark {
    fn default() -> Self {
        Self::new(false)
    }
}

impl ViewScrollBenchmark {
    pub fn new(is_table_view: bool) -> Self {
        Self {
            is_table_view: AtomicBool::new(is_table_view),
            scroll_events: AtomicUsize::new(0),
            hysteresis_hits: AtomicUsize::new(0),
            slice_reloads: AtomicUsize::new(0),
            total_slice_time_us: AtomicU64::new(0),
            min_slice_time_us: AtomicU64::new(0),
            max_slice_time_us: AtomicU64::new(0),
            last_logged_instant: parking_lot::Mutex::new(Instant::now()),
        }
    }

    #[inline]
    pub fn record_event(&self) {
        self.scroll_events.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_hysteresis_hit(&self) {
        self.hysteresis_hits.fetch_add(1, Ordering::Relaxed);
        self.check_periodic_log();
    }

    #[inline]
    pub fn record_slice_reload(&self, duration_us: u64) {
        self.slice_reloads.fetch_add(1, Ordering::Relaxed);
        self.total_slice_time_us.fetch_add(duration_us, Ordering::Relaxed);
        update_min(&self.min_slice_time_us, duration_us);
        update_max(&self.max_slice_time_us, duration_us);
        self.check_periodic_log();
    }

    fn check_periodic_log(&self) {
        let mut last = self.last_logged_instant.lock();
        if last.elapsed().as_millis() >= 1200 {
            let total_events = self.scroll_events.load(Ordering::Relaxed);
            let hits = self.hysteresis_hits.load(Ordering::Relaxed);
            let reloads = self.slice_reloads.load(Ordering::Relaxed);
            let total_reqs = hits + reloads;

            if total_reqs > 0 {
                let hit_pct = (hits.az::<f64>() / total_reqs.az::<f64>()) * 100.0;
                let avg_slice_us = if reloads > 0 {
                    self.total_slice_time_us.load(Ordering::Relaxed).az::<f64>() / reloads.az::<f64>()
                } else {
                    0.0
                };
                let view_str = if self.is_table_view.load(Ordering::Relaxed) {
                    "TableView"
                } else {
                    "GridView"
                };

                tracing::info!(
                    view = view_str,
                    scroll_events = total_events,
                    bypass_hits = hits,
                    slice_reloads = reloads,
                    hysteresis_hit_ratio = format!("{:.1}%", hit_pct),
                    avg_slice_latency_us = format!("{:.1} µs", avg_slice_us),
                    min_slice_us = self.min_slice_time_us.load(Ordering::Relaxed),
                    max_slice_us = self.max_slice_time_us.load(Ordering::Relaxed),
                    "[Benchmark: ViewScroll] High-FPS smooth virtualization window metrics"
                );
            }
            *last = Instant::now();
        }
    }
}

/// Thread-safe atomic metrics accumulator recording granular thumbnail pipeline performance.
#[derive(Debug)]
pub struct ThumbnailPipelineBenchmark {
    pub pipeline_id: usize,
    pub folder_id: u64,
    pub total_targets: AtomicUsize,
    pub start_time: Instant,

    // L1 RAM Cache Metrics
    pub l1_hits: AtomicUsize,
    pub l1_misses: AtomicUsize,

    // Phase 1: L2 PackStore Preload Metrics
    pub l2_pack_reads: AtomicUsize,
    pub l2_pack_bytes_read: AtomicU64,
    pub l2_pack_read_time_us: AtomicU64,

    pub decompress_count: AtomicUsize,
    pub decompress_webp_count: AtomicUsize,
    pub decompress_jpeg_count: AtomicUsize,
    pub decompress_raw_bytes: AtomicU64,
    pub decompress_time_us: AtomicU64,
    pub decompress_min_us: AtomicU64,
    pub decompress_max_us: AtomicU64,
    pub corrupted_repaired: AtomicUsize,
    pub preload_duration_us: AtomicU64,

    // Phase 2: Missing Thumbnail Generation Metrics
    pub image_decode_resize_count: AtomicUsize,
    pub image_decode_resize_time_us: AtomicU64,
    pub image_decode_min_us: AtomicU64,
    pub image_decode_max_us: AtomicU64,
    pub video_extract_count: AtomicUsize,
    pub video_extract_time_us: AtomicU64,
    pub compress_count: AtomicUsize,
    pub compress_bytes: AtomicU64,
    pub compress_time_us: AtomicU64,
    pub compress_min_us: AtomicU64,
    pub compress_max_us: AtomicU64,
    pub pack_appends: AtomicUsize,
    pub pack_append_bytes: AtomicU64,
    pub pack_append_time_us: AtomicU64,
    pub lmdb_commit_batches: AtomicUsize,
    pub lmdb_committed_records: AtomicUsize,
    pub lmdb_commit_time_us: AtomicU64,
    pub generate_duration_us: AtomicU64,

    // Stage 4: UI Draining & Slint Model Feed Metrics
    pub drain_ticks: AtomicUsize,
    pub drained_items: AtomicUsize,
    pub drain_time_us: AtomicU64,
    pub slint_set_row_data_us: AtomicU64,
    pub peak_drain_queue_depth: AtomicUsize,

    // Lifecycle Status
    pub is_finalized: AtomicBool,
}

impl ThumbnailPipelineBenchmark {
    pub fn new(pipeline_id: usize, folder_id: u64, total_targets: usize) -> Self {
        Self {
            pipeline_id,
            folder_id,
            total_targets: AtomicUsize::new(total_targets),
            start_time: Instant::now(),

            l1_hits: AtomicUsize::new(0),
            l1_misses: AtomicUsize::new(0),

            l2_pack_reads: AtomicUsize::new(0),
            l2_pack_bytes_read: AtomicU64::new(0),
            l2_pack_read_time_us: AtomicU64::new(0),

            decompress_count: AtomicUsize::new(0),
            decompress_webp_count: AtomicUsize::new(0),
            decompress_jpeg_count: AtomicUsize::new(0),
            decompress_raw_bytes: AtomicU64::new(0),
            decompress_time_us: AtomicU64::new(0),
            decompress_min_us: AtomicU64::new(0),
            decompress_max_us: AtomicU64::new(0),
            corrupted_repaired: AtomicUsize::new(0),
            preload_duration_us: AtomicU64::new(0),

            image_decode_resize_count: AtomicUsize::new(0),
            image_decode_resize_time_us: AtomicU64::new(0),
            image_decode_min_us: AtomicU64::new(0),
            image_decode_max_us: AtomicU64::new(0),
            video_extract_count: AtomicUsize::new(0),
            video_extract_time_us: AtomicU64::new(0),
            compress_count: AtomicUsize::new(0),
            compress_bytes: AtomicU64::new(0),
            compress_time_us: AtomicU64::new(0),
            compress_min_us: AtomicU64::new(0),
            compress_max_us: AtomicU64::new(0),
            pack_appends: AtomicUsize::new(0),
            pack_append_bytes: AtomicU64::new(0),
            pack_append_time_us: AtomicU64::new(0),
            lmdb_commit_batches: AtomicUsize::new(0),
            lmdb_committed_records: AtomicUsize::new(0),
            lmdb_commit_time_us: AtomicU64::new(0),
            generate_duration_us: AtomicU64::new(0),

            drain_ticks: AtomicUsize::new(0),
            drained_items: AtomicUsize::new(0),
            drain_time_us: AtomicU64::new(0),
            slint_set_row_data_us: AtomicU64::new(0),
            peak_drain_queue_depth: AtomicUsize::new(0),

            is_finalized: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn record_l1_hit(&self) {
        self.l1_hits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_l1_miss(&self) {
        self.l1_misses.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_pack_read(&self, bytes: u64, duration_us: u64) {
        self.l2_pack_reads.fetch_add(1, Ordering::Relaxed);
        self.l2_pack_bytes_read.fetch_add(bytes, Ordering::Relaxed);
        self.l2_pack_read_time_us.fetch_add(duration_us, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_decompress(&self, is_webp: bool, raw_bytes: u64, duration_us: u64) {
        self.decompress_count.fetch_add(1, Ordering::Relaxed);
        if is_webp {
            self.decompress_webp_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.decompress_jpeg_count.fetch_add(1, Ordering::Relaxed);
        }
        self.decompress_raw_bytes.fetch_add(raw_bytes, Ordering::Relaxed);
        self.decompress_time_us.fetch_add(duration_us, Ordering::Relaxed);
        update_min(&self.decompress_min_us, duration_us);
        update_max(&self.decompress_max_us, duration_us);
    }

    #[inline]
    pub fn record_corrupted_repaired(&self, count: usize) {
        self.corrupted_repaired.fetch_add(count, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_image_decode_resize(&self, duration_us: u64) {
        self.image_decode_resize_count.fetch_add(1, Ordering::Relaxed);
        self.image_decode_resize_time_us
            .fetch_add(duration_us, Ordering::Relaxed);
        update_min(&self.image_decode_min_us, duration_us);
        update_max(&self.image_decode_max_us, duration_us);
    }

    #[inline]
    pub fn record_video_extract(&self, duration_us: u64) {
        self.video_extract_count.fetch_add(1, Ordering::Relaxed);
        self.video_extract_time_us.fetch_add(duration_us, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_compress(&self, bytes: u64, duration_us: u64) {
        self.compress_count.fetch_add(1, Ordering::Relaxed);
        self.compress_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.compress_time_us.fetch_add(duration_us, Ordering::Relaxed);
        update_min(&self.compress_min_us, duration_us);
        update_max(&self.compress_max_us, duration_us);
    }

    #[inline]
    pub fn record_pack_append(&self, bytes: u64, duration_us: u64) {
        self.pack_appends.fetch_add(1, Ordering::Relaxed);
        self.pack_append_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.pack_append_time_us.fetch_add(duration_us, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_lmdb_commit(&self, records_count: usize, duration_us: u64) {
        self.lmdb_commit_batches.fetch_add(1, Ordering::Relaxed);
        self.lmdb_committed_records.fetch_add(records_count, Ordering::Relaxed);
        self.lmdb_commit_time_us.fetch_add(duration_us, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_drain_cycle(&self, items_count: usize, row_set_us: u64, total_drain_us: u64) {
        self.drain_ticks.fetch_add(1, Ordering::Relaxed);
        self.drained_items.fetch_add(items_count, Ordering::Relaxed);
        self.slint_set_row_data_us.fetch_add(row_set_us, Ordering::Relaxed);
        self.drain_time_us.fetch_add(total_drain_us, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_queue_depth(&self, current_depth: usize) {
        let mut current_peak = self.peak_drain_queue_depth.load(Ordering::Relaxed);
        while current_depth > current_peak {
            match self.peak_drain_queue_depth.compare_exchange_weak(
                current_peak,
                current_depth,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_peak = actual,
            }
        }
    }

    #[inline]
    pub fn set_preload_duration(&self, duration_us: u64) {
        self.preload_duration_us.store(duration_us, Ordering::Release);
    }

    #[inline]
    pub fn set_generate_duration(&self, duration_us: u64) {
        self.generate_duration_us.store(duration_us, Ordering::Release);
    }
}

/// Comprehensive benchmark tracking the complete folder load lifecycle.
#[derive(Debug)]
pub struct FolderLoadBenchmark {
    pub folder_id: u64,
    pub folder_path: String,
    pub start_time: Instant,

    // Stage 1: LMDB Database Fetch & Pre-indexing
    pub lmdb_id_fetch_time_us: AtomicU64,
    pub lmdb_record_fetch_time_us: AtomicU64,
    pub total_records_fetched: AtomicUsize,
    pub preindex_time_us: AtomicU64,

    // Stage 2: View Filtering, Sorting & Slint UI Projection
    pub filter_time_us: AtomicU64,
    pub items_matched: AtomicUsize,
    pub sort_time_us: AtomicU64,
    pub view_model_map_time_us: AtomicU64,
    pub slint_commit_time_us: AtomicU64,

    // Stage 2b: Incremental File Scanner (if executed)
    pub scan_executed: AtomicBool,
    pub fs_scan_time_us: AtomicU64,
    pub files_scanned: AtomicUsize,
    pub new_files: AtomicUsize,
    pub updated_files: AtomicUsize,
    pub unchanged_files: AtomicUsize,
    pub deleted_files: AtomicUsize,
    pub restored_files: AtomicUsize,
    pub scan_commit_time_us: AtomicU64,
}

impl FolderLoadBenchmark {
    pub fn new(folder_id: u64, folder_path: String) -> Self {
        Self {
            folder_id,
            folder_path,
            start_time: Instant::now(),

            lmdb_id_fetch_time_us: AtomicU64::new(0),
            lmdb_record_fetch_time_us: AtomicU64::new(0),
            total_records_fetched: AtomicUsize::new(0),
            preindex_time_us: AtomicU64::new(0),

            filter_time_us: AtomicU64::new(0),
            items_matched: AtomicUsize::new(0),
            sort_time_us: AtomicU64::new(0),
            view_model_map_time_us: AtomicU64::new(0),
            slint_commit_time_us: AtomicU64::new(0),

            scan_executed: AtomicBool::new(false),
            fs_scan_time_us: AtomicU64::new(0),
            files_scanned: AtomicUsize::new(0),
            new_files: AtomicUsize::new(0),
            updated_files: AtomicUsize::new(0),
            unchanged_files: AtomicUsize::new(0),
            deleted_files: AtomicUsize::new(0),
            restored_files: AtomicUsize::new(0),
            scan_commit_time_us: AtomicU64::new(0),
        }
    }
}

#[inline]
fn update_min(atomic: &AtomicU64, val: u64) {
    let mut current = atomic.load(Ordering::Relaxed);
    while current == 0 || val < current {
        match atomic.compare_exchange_weak(current, val, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

#[inline]
fn update_max(atomic: &AtomicU64, val: u64) {
    atomic.fetch_max(val, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::get_system_memory_metrics;

    #[test]
    fn process_and_system_memory_are_read_from_procfs() {
        let metrics = get_system_memory_metrics();
        assert!(metrics.process_rss_mb > 0.0);
        assert!(metrics.process_vms_mb >= metrics.process_rss_mb);
        assert!(metrics.system_total_mb >= metrics.system_available_mb);
        assert!(metrics.system_total_mb > 0.0);
    }
}
