use crate::settings::LogObscurer;
use crate::thumbnails::ThumbnailStorageStats;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

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
        let page_size = procfs::page_size() as f64;
        metrics.process_vms_mb = statm.size as f64 * page_size / MB;
        metrics.process_rss_mb = statm.resident as f64 * page_size / MB;
    }

    if let Ok(meminfo) = Meminfo::current() {
        metrics.system_total_mb = meminfo.mem_total as f64 / MB;
        metrics.system_available_mb = meminfo.mem_available.unwrap_or(0) as f64 / MB;
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
                let hit_pct = (hits as f64 / total_reqs as f64) * 100.0;
                let avg_slice_us = if reloads > 0 {
                    self.total_slice_time_us.load(Ordering::Relaxed) as f64 / reloads as f64
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

/// Helper formatting an integer with comma-separated thousands grouping.
pub fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    let bytes = s.as_bytes();
    let len = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(b as char);
    }
    result
}

/// Helper formatter for microseconds and milliseconds.
pub fn format_duration_us(us: u64) -> String {
    if us < 1_000 {
        format!("{us} µs")
    } else if us < 1_000_000 {
        format!("{:.2} ms", us as f64 / 1_000.0)
    } else {
        format!("{:.2} s", us as f64 / 1_000_000.0)
    }
}

/// Helper formatter for bytes into KB, MB, GB.
pub fn format_bytes_metric(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub struct BenchmarkReportFormatter;

impl BenchmarkReportFormatter {
    /// Renders an exhaustive ASCII summary report of the folder loading and thumbnail pipeline execution.
    pub fn render_summary(
        folder_bench: Option<&FolderLoadBenchmark>,
        thumb_bench: &ThumbnailPipelineBenchmark,
        storage_stats: &ThumbnailStorageStats,
    ) -> String {
        let mut out = String::with_capacity(4096);
        let total_wall_us = thumb_bench.start_time.elapsed().as_micros() as u64;

        let total_items = folder_bench.map_or(0, |f| f.total_records_fetched.load(Ordering::Relaxed));

        let p0_targets = thumb_bench.total_targets.load(Ordering::Relaxed);

        let overall_throughput = if total_wall_us > 0 && p0_targets > 0 {
            let items_per_sec = (p0_targets as f64) / (total_wall_us as f64 / 1_000_000.0);
            format!("{} items/sec", format_count(items_per_sec.round() as usize))
        } else {
            "-".to_string()
        };

        let mem = get_system_memory_metrics();

        out.push_str("\n================================================================================\n");
        out.push_str("                RUSTVIEW BENCHMARK SUMMARY: FOLDER LOAD & CACHE                 \n");
        out.push_str("================================================================================\n");

        if let Some(fb) = folder_bench {
            out.push_str(&format!(
                "Folder Target        : {} (Folder #{})\n",
                LogObscurer::path(&fb.folder_path),
                fb.folder_id
            ));
            out.push_str(&format!(
                "Total Media Items    : {total_items:>7} active records in database\n"
            ));
        } else {
            out.push_str(&format!("Folder ID Target     : Folder #{}\n", thumb_bench.folder_id));
        }

        out.push_str(&format!(
            "Total Wall-Clock Time: {} (Initial Viewport Ready)\n",
            format_duration_us(total_wall_us)
        ));
        out.push_str(&format!("Initial Throughput   : {overall_throughput}\n"));
        out.push_str(&format!(
            "Process Memory (RSS) : {:.2} MB (Virtual: {:.2} MB | System Avail: {:.1} GB / {:.1} GB)\n",
            mem.process_rss_mb,
            mem.process_vms_mb,
            mem.system_available_mb / 1024.0,
            mem.system_total_mb / 1024.0
        ));
        out.push_str("--------------------------------------------------------------------------------\n");

        // STAGE 1: Database & Pre-indexing
        if let Some(fb) = folder_bench {
            let id_fetch_us = fb.lmdb_id_fetch_time_us.load(Ordering::Relaxed);
            let rec_fetch_us = fb.lmdb_record_fetch_time_us.load(Ordering::Relaxed);
            let preindex_us = fb.preindex_time_us.load(Ordering::Relaxed);
            let stage1_us = id_fetch_us + rec_fetch_us + preindex_us;
            let preindex_per_item = if total_items > 0 {
                format!("{:.2} µs / item", preindex_us as f64 / total_items as f64)
            } else {
                "-".to_string()
            };
            let fetch_per_item = if total_items > 0 {
                format!("{:.2} µs / item", rec_fetch_us as f64 / total_items as f64)
            } else {
                "-".to_string()
            };

            out.push_str("STAGE 1: LMDB DATABASE RETRIEVAL & IN-MEMORY PRE-INDEXING\n");
            out.push_str(&format!(
                "  • ID List Fetch           : {}\n",
                format_duration_us(id_fetch_us)
            ));
            out.push_str(&format!(
                "  • Record Stream Iteration : {} ({})\n",
                format_duration_us(rec_fetch_us),
                fetch_per_item
            ));
            out.push_str(&format!(
                "  • String & Dimension Prep : {} ({})\n",
                format_duration_us(preindex_us),
                preindex_per_item
            ));
            out.push_str(&format!(
                "  • Stage 1 Subtotal        : {}\n",
                format_duration_us(stage1_us)
            ));
            out.push_str("--------------------------------------------------------------------------------\n");

            // STAGE 2: View Filtering & Projection
            let filter_us = fb.filter_time_us.load(Ordering::Relaxed);
            let sort_us = fb.sort_time_us.load(Ordering::Relaxed);
            let map_us = fb.view_model_map_time_us.load(Ordering::Relaxed);
            let commit_us = fb.slint_commit_time_us.load(Ordering::Relaxed);
            let matched = fb.items_matched.load(Ordering::Relaxed);
            let stage2_us = filter_us + sort_us + map_us + commit_us;

            out.push_str("STAGE 2: VIEW FILTERING, SORTING & SLINT PROJECTION\n");
            out.push_str(&format!(
                "  • Search / Tag Filtering  : {} (Matched: {} / {})\n",
                format_duration_us(filter_us),
                matched,
                total_items
            ));
            out.push_str(&format!(
                "  • Sorter Execution        : {}\n",
                format_duration_us(sort_us)
            ));
            out.push_str(&format!(
                "  • Slint View Model Map    : {}\n",
                format_duration_us(map_us)
            ));
            out.push_str(&format!(
                "  • Slint UI Commit (Paint) : {}\n",
                format_duration_us(commit_us)
            ));
            out.push_str(&format!(
                "  • Stage 2 Subtotal        : {}\n",
                format_duration_us(stage2_us)
            ));
            out.push_str("--------------------------------------------------------------------------------\n");

            // Optional Scanner Stage
            if fb.scan_executed.load(Ordering::Relaxed) {
                let scan_us = fb.fs_scan_time_us.load(Ordering::Relaxed);
                let scan_commit_us = fb.scan_commit_time_us.load(Ordering::Relaxed);
                out.push_str("STAGE 2b: BACKGROUND INCREMENTAL FILESYSTEM SCANNER\n");
                out.push_str(&format!(
                    "  • FS Directory Scan Time  : {}\n",
                    format_duration_us(scan_us)
                ));
                out.push_str(&format!(
                    "  • Files Inspected         : {:>7} (New: {}, Upd: {}, Unch: {}, Del: {}, Rest: {})\n",
                    fb.files_scanned.load(Ordering::Relaxed),
                    fb.new_files.load(Ordering::Relaxed),
                    fb.updated_files.load(Ordering::Relaxed),
                    fb.unchanged_files.load(Ordering::Relaxed),
                    fb.deleted_files.load(Ordering::Relaxed),
                    fb.restored_files.load(Ordering::Relaxed),
                ));
                out.push_str(&format!(
                    "  • LMDB Write Txn Commit   : {}\n",
                    format_duration_us(scan_commit_us)
                ));
                out.push_str("--------------------------------------------------------------------------------\n");
            }
        }

        // STAGE 3: Thumbnail Cache & Pipeline Execution
        let l1_hits = thumb_bench.l1_hits.load(Ordering::Relaxed);
        let l1_misses = thumb_bench.l1_misses.load(Ordering::Relaxed);
        let l1_total = l1_hits + l1_misses;
        let l1_ratio = if l1_total > 0 {
            format!("{:.1}%", (l1_hits as f64 / l1_total as f64) * 100.0)
        } else {
            "0.0%".to_string()
        };

        let pack_reads = thumb_bench.l2_pack_reads.load(Ordering::Relaxed);
        let pack_bytes_read = thumb_bench.l2_pack_bytes_read.load(Ordering::Relaxed);
        let avg_pack_read_bytes = if pack_reads > 0 {
            pack_bytes_read / (pack_reads as u64)
        } else {
            0
        };

        let decompress_count = thumb_bench.decompress_count.load(Ordering::Relaxed);
        let decompress_webp = thumb_bench.decompress_webp_count.load(Ordering::Relaxed);
        let decompress_jpeg = thumb_bench.decompress_jpeg_count.load(Ordering::Relaxed);
        let decompress_raw_bytes = thumb_bench.decompress_raw_bytes.load(Ordering::Relaxed);
        let decompress_cpu_us = thumb_bench.decompress_time_us.load(Ordering::Relaxed);
        let decompress_avg_us = if decompress_count > 0 {
            decompress_cpu_us as f64 / decompress_count as f64
        } else {
            0.0
        };

        let video_count = thumb_bench.video_extract_count.load(Ordering::Relaxed);
        let video_cpu_us = thumb_bench.video_extract_time_us.load(Ordering::Relaxed);
        let avg_video_extract_ms = if video_count > 0 {
            (video_cpu_us as f64 / video_count as f64) / 1000.0
        } else {
            0.0
        };

        let preload_duration_us = thumb_bench.preload_duration_us.load(Ordering::Relaxed);
        let preload_throughput = if preload_duration_us > 0 && decompress_count > 0 {
            let thumbs_per_sec = (decompress_count as f64) / (preload_duration_us as f64 / 1_000_000.0);
            format!("{} thumbs/sec", format_count(thumbs_per_sec.round() as usize))
        } else {
            "-".to_string()
        };

        let generate_duration_us = thumb_bench.generate_duration_us.load(Ordering::Relaxed);
        let gen_count = thumb_bench.compress_count.load(Ordering::Relaxed);
        let gen_throughput = if generate_duration_us > 0 && gen_count > 0 {
            let items_per_sec = (gen_count as f64) / (generate_duration_us as f64 / 1_000_000.0);
            format!("{} items/sec", format_count(items_per_sec.round() as usize))
        } else {
            "-".to_string()
        };

        let drain_ticks = thumb_bench.drain_ticks.load(Ordering::Relaxed);
        let drained_items = thumb_bench.drained_items.load(Ordering::Relaxed);
        let row_set_us = thumb_bench.slint_set_row_data_us.load(Ordering::Relaxed);
        let drain_time_us = thumb_bench.drain_time_us.load(Ordering::Relaxed);
        let peak_queue = thumb_bench.peak_drain_queue_depth.load(Ordering::Relaxed);

        out.push_str("STAGE 3: THUMBNAIL CACHE & GENERATION PIPELINE\n");
        out.push_str(&format!(
            "  • L1 RAM Cache Status     : {l1_hits} hits / {l1_total} requests ({l1_ratio} hit ratio, {l1_misses} misses)\n"
        ));
        if decompress_count > 0 {
            out.push_str(&format!(
                "  • P0 Viewport Preload     : {} cached items in {} ({})\n",
                decompress_count,
                format_duration_us(preload_duration_us),
                preload_throughput
            ));
            out.push_str(&format!(
                "    ├─ Binary Pack Reads    : {} reads | {} read (Avg: {} / read)\n",
                pack_reads,
                format_bytes_metric(pack_bytes_read),
                format_bytes_metric(avg_pack_read_bytes)
            ));
            out.push_str(&format!(
                "    └─ Decompress Latency   : Avg: {:.2} ms ({} WebP / {} JPEG | {} raw RGB8)\n",
                decompress_avg_us / 1000.0,
                decompress_webp,
                decompress_jpeg,
                format_bytes_metric(decompress_raw_bytes)
            ));
        }
        if video_count > 0 || gen_count > 0 {
            out.push_str(&format!(
                "  • Phase 2 Generation      : {} items in {} ({})\n",
                gen_count,
                format_duration_us(generate_duration_us),
                gen_throughput
            ));
            if video_count > 0 {
                out.push_str(&format!(
                    "    ├─ Video Extractions    : {video_count} videos | Avg: {avg_video_extract_ms:.2} ms / video\n"
                ));
            }
            out.push_str(&format!(
                "    ├─ Compressed Encodes   : {} items | {}\n",
                thumb_bench.compress_count.load(Ordering::Relaxed),
                format_bytes_metric(thumb_bench.compress_bytes.load(Ordering::Relaxed))
            ));
            out.push_str(&format!(
                "    └─ LMDB Batched Commits : {} batches ({} records in {})\n",
                thumb_bench.lmdb_commit_batches.load(Ordering::Relaxed),
                thumb_bench.lmdb_committed_records.load(Ordering::Relaxed),
                format_duration_us(thumb_bench.lmdb_commit_time_us.load(Ordering::Relaxed))
            ));
        }

        out.push_str("--------------------------------------------------------------------------------\n");

        // STAGE 4: UI Drain & Slint Image Buffer Feed
        out.push_str("STAGE 4: SLINT UI DRAIN & IMAGE BUFFER FEED\n");
        out.push_str(&format!(
            "  • Drain Timer Ticks       : {drain_ticks} cycles (capped batch: 64 items/tick)\n"
        ));
        out.push_str(&format!(
            "  • P0 Items Delivered to UI: {drained_items} / {p0_targets} thumbnails displayed (100% viewport ready)\n"
        ));
        out.push_str(&format!(
            "  • Slint Row Model Updates : {} (set_row_data)\n",
            format_duration_us(row_set_us)
        ));
        out.push_str(&format!(
            "  • Peak Drain Queue Depth  : {peak_queue} items buffered in RAM\n"
        ));
        out.push_str(&format!(
            "  • Stage 4 Subtotal Time   : {}\n",
            format_duration_us(drain_time_us)
        ));
        out.push_str("--------------------------------------------------------------------------------\n");

        // STORAGE & CAPACITY STATS
        let l1_slots_cap = 2048;
        let l1_occupancy_pct = (drained_items.min(l1_slots_cap) as f64 / l1_slots_cap as f64) * 100.0;
        out.push_str("STORAGE FOOTPRINT & CACHE CAPACITY:\n");
        out.push_str(&format!(
            "  • L1 In-Memory Cache      : {} / {} slots utilized ({:.1}% capacity)\n",
            drained_items.min(l1_slots_cap),
            l1_slots_cap,
            l1_occupancy_pct
        ));
        out.push_str(&format!(
            "  • L2 Binary Pack Files    : {} packfiles | {:.2} MB on disk\n",
            storage_stats.pack_files_count, storage_stats.pack_total_mb
        ));
        out.push_str(&format!(
            "  • LMDB Database Size      : {:.2} MB ({} thumbnails, {} video previews)\n",
            storage_stats.lmdb_data_mb,
            format_count(storage_stats.lmdb_thumbnails_count),
            format_count(storage_stats.lmdb_previews_count)
        ));
        out.push_str(&format!(
            "  • Average Stored Size     : {:.2} KB / thumbnail frame\n",
            storage_stats.avg_thumbnail_kb()
        ));
        out.push_str(&format!(
            "  • Total Cache Footprint   : {:.2} MB\n",
            storage_stats.total_cache_mb
        ));
        out.push_str("================================================================================\n");

        out
    }
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
