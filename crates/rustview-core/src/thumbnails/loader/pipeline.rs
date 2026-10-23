//! The folder pipeline: scanning a folder and orchestrating the workers.

use super::{PipelineItem, PreparedTarget, ThumbnailScheduler};
use crate::library::models::MediaId;
use crate::telemetry::benchmark::ThumbnailPipelineBenchmark;
use crate::thumbnails::types::ThumbnailConfig;
use crate::util::cast::SaturatingAs;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

impl ThumbnailScheduler {
    /// Launches dedicated full-folder thumbnail pipeline processing ALL items in the folder.
    pub fn start_folder_pipeline(self: &Arc<Self>, folder_id: u64, items: Vec<PipelineItem>, config: ThumbnailConfig) {
        self.cancel_current_pipeline();

        let total_items = items.len();
        if total_items == 0 {
            return;
        }

        self.set_config(config.clone());
        let pipeline_id = self.active_pipeline_id.load(Ordering::Acquire);
        self.is_cancelled.store(false, Ordering::Release);

        tracing::info!(
            folder_id = folder_id,
            pipeline_id = pipeline_id,
            total_items = total_items,
            generate_previews = config.generate_video_previews,
            preview_frame_count = config.preview_frame_count,
            "[ThumbnailPipeline: Start] Initializing dedicated full-folder thumbnail pipeline..."
        );

        let bench = Arc::new(ThumbnailPipelineBenchmark::new(pipeline_id, folder_id, total_items));
        *self.active_benchmark.write() = Some(bench.clone());
        self.ready_drain_queue.wake();

        let scheduler = self.clone();
        let bench_clone = bench;
        self.pool.spawn(move || {
            scheduler.execute_orchestration(pipeline_id, folder_id, items, &config, &bench_clone);
        });
    }

    /// The file path of a media item from the library (folder path + file name), with the folder paths cached per scan.
    pub(super) fn item_path(
        &self,
        rtxn: &heed::RoTxn,
        folder_paths: &mut HashMap<u64, PathBuf>,
        media_id: MediaId,
    ) -> Option<PathBuf> {
        let db = self.library.raw_db();
        let record = db.media.get(rtxn, &media_id).ok().flatten()?;
        let folder_dir = match folder_paths.entry(record.folder_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let folder = db.folders.get(rtxn, &record.folder_id).ok().flatten()?;
                entry.insert(PathBuf::from(folder.path))
            }
        };
        Some(folder_dir.join(&record.filename))
    }

    pub(super) fn execute_orchestration(
        &self,
        pipeline_id: usize,
        folder_id: u64,
        items: Vec<PipelineItem>,
        config: &ThumbnailConfig,
        benchmark: &Arc<ThumbnailPipelineBenchmark>,
    ) {
        if self.is_pipeline_stale(pipeline_id) {
            return;
        }

        // Thumbnails that exist are not decoded here: the Browse view loads the ones it shows on demand, so opening a folder costs
        // one database lookup per item and no pixel memory, however large the folder is.
        let t_scan = Instant::now();
        let mut existing_count = 0usize;
        let mut missing_targets: Vec<PreparedTarget> = Vec::new();

        {
            let rtxn = match self.library.raw_db().env.read_txn() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to acquire LMDB read transaction for thumbnail pipeline");
                    return;
                }
            };

            let mut folder_paths: HashMap<u64, PathBuf> = HashMap::new();
            for item in items {
                if self.is_pipeline_stale(pipeline_id) {
                    return;
                }

                let has_primary = self
                    .library
                    .raw_db()
                    .thumbnails
                    .get(&rtxn, &item.media_id)
                    .ok()
                    .flatten()
                    .is_some();

                let previews_ok = if item.is_video && config.generate_video_previews {
                    if let Ok(Some(prev)) = self.library.raw_db().video_previews.get(&rtxn, &item.media_id) {
                        prev.frame_count >= config.preview_frame_count && !prev.frame_locations.is_empty()
                    } else {
                        false
                    }
                } else if item.animatable && config.generate_video_previews && item.upgrade_animated {
                    // Any record, also the "checked, not animated" marker, means the picture was looked at already.
                    self.library
                        .raw_db()
                        .video_previews
                        .get(&rtxn, &item.media_id)
                        .ok()
                        .flatten()
                        .is_some()
                } else {
                    true
                };

                if has_primary && previews_ok {
                    existing_count += 1;
                } else if let Some(file_path) = self.item_path(&rtxn, &mut folder_paths, item.media_id) {
                    missing_targets.push(PreparedTarget {
                        pipeline_id,
                        folder_id,
                        media_id: item.media_id,
                        file_path,
                        is_video: item.is_video,
                        animatable: item.animatable,
                    });
                }
            }
        }

        benchmark.set_preload_duration(t_scan.elapsed().as_micros().saturating_as::<u64>());
        tracing::info!(
            folder_id = folder_id,
            pipeline_id = pipeline_id,
            existing_cached = existing_count,
            missing_to_generate = missing_targets.len(),
            scan_us = t_scan.elapsed().as_micros().saturating_as::<u64>(),
            "[ThumbnailPipeline: Scan] Existing thumbnails stay in the pack store (loaded on demand); generating the missing ones"
        );

        // =========================================================================
        // PHASE 2: Dedicated Missing Thumbnail & Video Preview Generation
        // =========================================================================
        let final_missing_count = missing_targets.len();

        if final_missing_count > 0 {
            self.tracker.set_generate_total(final_missing_count);

            tracing::info!(
                folder_id = folder_id,
                missing_count = final_missing_count,
                threads = self.pool.current_num_threads(),
                "[ThumbnailPipeline: Phase 2] Generating missing thumbnails and video previews dedicatedly..."
            );

            {
                let mut q = self.generation_queue.lock();
                q.clear();
                for target in missing_targets {
                    q.push_back(target);
                }
            }

            let t_phase2_start = Arc::new(Instant::now());
            let num_threads = self.pool.current_num_threads();

            for _ in 0..num_threads {
                let scheduler = self.clone_handle();
                let cfg = config.clone();
                let bench_gen = benchmark.clone();
                let t_p2 = t_phase2_start.clone();

                self.pool.spawn(move || {
                    scheduler.worker_generation_loop(pipeline_id, folder_id, &cfg, &bench_gen, &t_p2);
                });
            }
        }
    }
}
