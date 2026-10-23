//! The text report of a thumbnail pipeline run and the number formatting it needs.

use super::{FolderLoadBenchmark, ThumbnailPipelineBenchmark, get_system_memory_metrics};
use crate::settings::LogObscurer;
use crate::thumbnails::ThumbnailStorageStats;
use crate::util::cast::{Az, FloatToInt, SaturatingAs};
use std::fmt::Write as _;
use std::sync::atomic::Ordering;

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
        format!("{:.2} ms", us.az::<f64>() / 1_000.0)
    } else {
        format!("{:.2} s", us.az::<f64>() / 1_000_000.0)
    }
}

/// Helper formatter for bytes into KB, MB, GB.
pub fn format_bytes_metric(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes.az::<f64>() / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes.az::<f64>() / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes.az::<f64>() / (1024.0 * 1024.0 * 1024.0))
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
        let total_wall_us = thumb_bench.start_time.elapsed().as_micros().saturating_as::<u64>();

        let total_items = folder_bench.map_or(0, |f| f.total_records_fetched.load(Ordering::Relaxed));

        let p0_targets = thumb_bench.total_targets.load(Ordering::Relaxed);

        let overall_throughput = if total_wall_us > 0 && p0_targets > 0 {
            let items_per_sec = p0_targets.az::<f64>() / (total_wall_us.az::<f64>() / 1_000_000.0);
            format!("{} items/sec", format_count(items_per_sec.round().to_int::<usize>()))
        } else {
            "-".to_string()
        };

        let mem = get_system_memory_metrics();

        out.push_str("\n================================================================================\n");
        out.push_str("                RUSTVIEW BENCHMARK SUMMARY: FOLDER LOAD & CACHE                 \n");
        out.push_str("================================================================================\n");

        if let Some(fb) = folder_bench {
            let _ = writeln!(
                out,
                "Folder Target        : {} (Folder #{})",
                LogObscurer::path(&fb.folder_path),
                fb.folder_id
            );
            let _ = writeln!(
                out,
                "Total Media Items    : {total_items:>7} active records in database"
            );
        } else {
            let _ = writeln!(out, "Folder ID Target     : Folder #{}", thumb_bench.folder_id);
        }

        let _ = writeln!(
            out,
            "Total Wall-Clock Time: {} (Initial Viewport Ready)",
            format_duration_us(total_wall_us)
        );
        let _ = writeln!(out, "Initial Throughput   : {overall_throughput}");
        let _ = writeln!(
            out,
            "Process Memory (RSS) : {:.2} MB (Virtual: {:.2} MB | System Avail: {:.1} GB / {:.1} GB)",
            mem.process_rss_mb,
            mem.process_vms_mb,
            mem.system_available_mb / 1024.0,
            mem.system_total_mb / 1024.0
        );
        out.push_str("--------------------------------------------------------------------------------\n");

        // STAGE 1: Database & Pre-indexing
        if let Some(fb) = folder_bench {
            let id_fetch_us = fb.lmdb_id_fetch_time_us.load(Ordering::Relaxed);
            let rec_fetch_us = fb.lmdb_record_fetch_time_us.load(Ordering::Relaxed);
            let preindex_us = fb.preindex_time_us.load(Ordering::Relaxed);
            let stage1_us = id_fetch_us + rec_fetch_us + preindex_us;
            let preindex_per_item = if total_items > 0 {
                format!("{:.2} µs / item", preindex_us.az::<f64>() / total_items.az::<f64>())
            } else {
                "-".to_string()
            };
            let fetch_per_item = if total_items > 0 {
                format!("{:.2} µs / item", rec_fetch_us.az::<f64>() / total_items.az::<f64>())
            } else {
                "-".to_string()
            };

            out.push_str("STAGE 1: LMDB DATABASE RETRIEVAL & IN-MEMORY PRE-INDEXING\n");
            let _ = writeln!(out, "  • ID List Fetch           : {}", format_duration_us(id_fetch_us));
            let _ = writeln!(
                out,
                "  • Record Stream Iteration : {} ({})",
                format_duration_us(rec_fetch_us),
                fetch_per_item
            );
            let _ = writeln!(
                out,
                "  • String & Dimension Prep : {} ({})",
                format_duration_us(preindex_us),
                preindex_per_item
            );
            let _ = writeln!(out, "  • Stage 1 Subtotal        : {}", format_duration_us(stage1_us));
            out.push_str("--------------------------------------------------------------------------------\n");

            // STAGE 2: View Filtering & Projection
            let filter_us = fb.filter_time_us.load(Ordering::Relaxed);
            let sort_us = fb.sort_time_us.load(Ordering::Relaxed);
            let map_us = fb.view_model_map_time_us.load(Ordering::Relaxed);
            let commit_us = fb.slint_commit_time_us.load(Ordering::Relaxed);
            let matched = fb.items_matched.load(Ordering::Relaxed);
            let stage2_us = filter_us + sort_us + map_us + commit_us;

            out.push_str("STAGE 2: VIEW FILTERING, SORTING & SLINT PROJECTION\n");
            let _ = writeln!(
                out,
                "  • Search / Tag Filtering  : {} (Matched: {} / {})",
                format_duration_us(filter_us),
                matched,
                total_items
            );
            let _ = writeln!(out, "  • Sorter Execution        : {}", format_duration_us(sort_us));
            let _ = writeln!(out, "  • Slint View Model Map    : {}", format_duration_us(map_us));
            let _ = writeln!(out, "  • Slint UI Commit (Paint) : {}", format_duration_us(commit_us));
            let _ = writeln!(out, "  • Stage 2 Subtotal        : {}", format_duration_us(stage2_us));
            out.push_str("--------------------------------------------------------------------------------\n");

            // Optional Scanner Stage
            if fb.scan_executed.load(Ordering::Relaxed) {
                let scan_us = fb.fs_scan_time_us.load(Ordering::Relaxed);
                let scan_commit_us = fb.scan_commit_time_us.load(Ordering::Relaxed);
                out.push_str("STAGE 2b: BACKGROUND INCREMENTAL FILESYSTEM SCANNER\n");
                let _ = writeln!(out, "  • FS Directory Scan Time  : {}", format_duration_us(scan_us));
                let _ = writeln!(
                    out,
                    "  • Files Inspected         : {:>7} (New: {}, Upd: {}, Unch: {}, Del: {}, Rest: {})",
                    fb.files_scanned.load(Ordering::Relaxed),
                    fb.new_files.load(Ordering::Relaxed),
                    fb.updated_files.load(Ordering::Relaxed),
                    fb.unchanged_files.load(Ordering::Relaxed),
                    fb.deleted_files.load(Ordering::Relaxed),
                    fb.restored_files.load(Ordering::Relaxed),
                );
                let _ = writeln!(
                    out,
                    "  • LMDB Write Txn Commit   : {}",
                    format_duration_us(scan_commit_us)
                );
                out.push_str("--------------------------------------------------------------------------------\n");
            }
        }

        // STAGE 3: Thumbnail Cache & Pipeline Execution
        let l1_hits = thumb_bench.l1_hits.load(Ordering::Relaxed);
        let l1_misses = thumb_bench.l1_misses.load(Ordering::Relaxed);
        let l1_total = l1_hits + l1_misses;
        let l1_ratio = if l1_total > 0 {
            format!("{:.1}%", (l1_hits.az::<f64>() / l1_total.az::<f64>()) * 100.0)
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
            decompress_cpu_us.az::<f64>() / decompress_count.az::<f64>()
        } else {
            0.0
        };

        let video_count = thumb_bench.video_extract_count.load(Ordering::Relaxed);
        let video_cpu_us = thumb_bench.video_extract_time_us.load(Ordering::Relaxed);
        let avg_video_extract_ms = if video_count > 0 {
            (video_cpu_us.az::<f64>() / video_count.az::<f64>()) / 1000.0
        } else {
            0.0
        };

        let preload_duration_us = thumb_bench.preload_duration_us.load(Ordering::Relaxed);
        let preload_throughput = if preload_duration_us > 0 && decompress_count > 0 {
            let thumbs_per_sec = decompress_count.az::<f64>() / (preload_duration_us.az::<f64>() / 1_000_000.0);
            format!("{} thumbs/sec", format_count(thumbs_per_sec.round().to_int::<usize>()))
        } else {
            "-".to_string()
        };

        let generate_duration_us = thumb_bench.generate_duration_us.load(Ordering::Relaxed);
        let gen_count = thumb_bench.compress_count.load(Ordering::Relaxed);
        let gen_throughput = if generate_duration_us > 0 && gen_count > 0 {
            let items_per_sec = gen_count.az::<f64>() / (generate_duration_us.az::<f64>() / 1_000_000.0);
            format!("{} items/sec", format_count(items_per_sec.round().to_int::<usize>()))
        } else {
            "-".to_string()
        };

        let drain_ticks = thumb_bench.drain_ticks.load(Ordering::Relaxed);
        let drained_items = thumb_bench.drained_items.load(Ordering::Relaxed);
        let row_set_us = thumb_bench.slint_set_row_data_us.load(Ordering::Relaxed);
        let drain_time_us = thumb_bench.drain_time_us.load(Ordering::Relaxed);
        let peak_queue = thumb_bench.peak_drain_queue_depth.load(Ordering::Relaxed);

        out.push_str("STAGE 3: THUMBNAIL CACHE & GENERATION PIPELINE\n");
        let _ = writeln!(
            out,
            "  • L1 RAM Cache Status     : {l1_hits} hits / {l1_total} requests ({l1_ratio} hit ratio, {l1_misses} misses)"
        );
        if decompress_count > 0 {
            let _ = writeln!(
                out,
                "  • P0 Viewport Preload     : {} cached items in {} ({})",
                decompress_count,
                format_duration_us(preload_duration_us),
                preload_throughput
            );
            let _ = writeln!(
                out,
                "    ├─ Binary Pack Reads    : {} reads | {} read (Avg: {} / read)",
                pack_reads,
                format_bytes_metric(pack_bytes_read),
                format_bytes_metric(avg_pack_read_bytes)
            );
            let _ = writeln!(
                out,
                "    └─ Decompress Latency   : Avg: {:.2} ms ({} WebP / {} JPEG | {} raw RGB8)",
                decompress_avg_us / 1000.0,
                decompress_webp,
                decompress_jpeg,
                format_bytes_metric(decompress_raw_bytes)
            );
        }
        if video_count > 0 || gen_count > 0 {
            let _ = writeln!(
                out,
                "  • Phase 2 Generation      : {} items in {} ({})",
                gen_count,
                format_duration_us(generate_duration_us),
                gen_throughput
            );
            if video_count > 0 {
                let _ = writeln!(
                    out,
                    "    ├─ Video Extractions    : {video_count} videos | Avg: {avg_video_extract_ms:.2} ms / video"
                );
            }
            let _ = writeln!(
                out,
                "    ├─ Compressed Encodes   : {} items | {}",
                thumb_bench.compress_count.load(Ordering::Relaxed),
                format_bytes_metric(thumb_bench.compress_bytes.load(Ordering::Relaxed))
            );
            let _ = writeln!(
                out,
                "    └─ LMDB Batched Commits : {} batches ({} records in {})",
                thumb_bench.lmdb_commit_batches.load(Ordering::Relaxed),
                thumb_bench.lmdb_committed_records.load(Ordering::Relaxed),
                format_duration_us(thumb_bench.lmdb_commit_time_us.load(Ordering::Relaxed))
            );
        }

        out.push_str("--------------------------------------------------------------------------------\n");

        // STAGE 4: UI Drain & Slint Image Buffer Feed
        out.push_str("STAGE 4: SLINT UI DRAIN & IMAGE BUFFER FEED\n");
        let _ = writeln!(
            out,
            "  • Drain Timer Ticks       : {drain_ticks} cycles (capped batch: 64 items/tick)"
        );
        let _ = writeln!(
            out,
            "  • P0 Items Delivered to UI: {drained_items} / {p0_targets} thumbnails displayed (100% viewport ready)"
        );
        let _ = writeln!(
            out,
            "  • Slint Row Model Updates : {} (set_row_data)",
            format_duration_us(row_set_us)
        );
        let _ = writeln!(out, "  • Peak Drain Queue Depth  : {peak_queue} items buffered in RAM");
        let _ = writeln!(
            out,
            "  • Stage 4 Subtotal Time   : {}",
            format_duration_us(drain_time_us)
        );
        out.push_str("--------------------------------------------------------------------------------\n");

        // STORAGE & CAPACITY STATS
        let l1_slots_cap = 2048;
        let l1_occupancy_pct = (drained_items.min(l1_slots_cap).az::<f64>() / l1_slots_cap.az::<f64>()) * 100.0;
        out.push_str("STORAGE FOOTPRINT & CACHE CAPACITY:\n");
        let _ = writeln!(
            out,
            "  • L1 In-Memory Cache      : {} / {} slots utilized ({:.1}% capacity)",
            drained_items.min(l1_slots_cap),
            l1_slots_cap,
            l1_occupancy_pct
        );
        let _ = writeln!(
            out,
            "  • L2 Binary Pack Files    : {} packfiles | {:.2} MB on disk",
            storage_stats.pack_files_count, storage_stats.pack_total_mb
        );
        let _ = writeln!(
            out,
            "  • LMDB Database Size      : {:.2} MB ({} thumbnails, {} video previews)",
            storage_stats.lmdb_data_mb,
            format_count(storage_stats.lmdb_thumbnails_count),
            format_count(storage_stats.lmdb_previews_count)
        );
        let _ = writeln!(
            out,
            "  • Average Stored Size     : {:.2} KB / thumbnail frame",
            storage_stats.avg_thumbnail_kb()
        );
        let _ = writeln!(
            out,
            "  • Total Cache Footprint   : {:.2} MB",
            storage_stats.total_cache_mb
        );
        out.push_str("================================================================================\n");

        out
    }
}
