use super::cache::L1ThumbnailCache;
use super::error::ThumbnailError;
use super::generator::ThumbnailGenerator;
use super::progress::{ThumbnailProgressSnapshot, ThumbnailProgressTracker};
use super::store::PackStore;
use super::types::{FitMode, ThumbnailHandle, ThumbnailLocation, anonymize_target};
use crate::library::Library;
use crate::library::models::MediaId;
use parking_lot::Mutex;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

pub struct PreparedTarget {
    pub media_id: MediaId,
    pub file_path: PathBuf,
}

pub struct ReadyThumbnailItem {
    pub media_id: MediaId,
    pub rgb8_pixels: Vec<u8>,
}

pub struct ThumbnailScheduler {
    library: Arc<Library>,
    cache: Arc<L1ThumbnailCache>,
    store: Arc<PackStore>,
    generator: Arc<ThumbnailGenerator>,
    pool: Arc<ThreadPool>,
    tracker: Arc<ThumbnailProgressTracker>,
    ready_drain_queue: Arc<Mutex<Vec<ReadyThumbnailItem>>>,
    generation_queue: Arc<Mutex<VecDeque<PreparedTarget>>>,
    active_pipeline_id: Arc<AtomicUsize>,
    is_cancelled: Arc<AtomicBool>,
}

impl ThumbnailScheduler {
    pub fn new(
        library: Arc<Library>,
        cache: Arc<L1ThumbnailCache>,
        store: Arc<PackStore>,
        threads: usize,
    ) -> Self {
        let pool = Arc::new(
            ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("thumb-worker-{}", i))
                .build()
                .expect("Failed to build high-performance thumbnail worker pool"),
        );

        tracing::info!(
            worker_threads = threads,
            "Initialized SIMD Thumbnail Worker Pool."
        );

        Self {
            library,
            cache,
            store,
            generator: Arc::new(ThumbnailGenerator::new()),
            pool,
            tracker: Arc::new(ThumbnailProgressTracker::new()),
            ready_drain_queue: Arc::new(Mutex::new(Vec::with_capacity(512))),
            generation_queue: Arc::new(Mutex::new(VecDeque::new())),
            active_pipeline_id: Arc::new(AtomicUsize::new(0)),
            is_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Drains available ready thumbnail pixel buffers to the UI thread in batches.
    pub fn drain_ready_batch(&self, max_items: usize) -> Vec<ReadyThumbnailItem> {
        let mut queue = self.ready_drain_queue.lock();
        if queue.is_empty() {
            return Vec::new();
        }
        let take_count = queue.len().min(max_items);
        queue.drain(0..take_count).collect()
    }

    pub fn snapshot_progress(&self) -> ThumbnailProgressSnapshot {
        self.tracker.snapshot()
    }

    pub fn cancel_current_pipeline(&self) {
        self.is_cancelled.store(true, Ordering::Release);
        self.active_pipeline_id.fetch_add(1, Ordering::SeqCst);
        self.generation_queue.lock().clear();
        self.ready_drain_queue.lock().clear();
        self.tracker.reset();
        tracing::debug!("Thumbnail pipeline cancelled and queues cleared.");
    }

    /// Launches the 2-Phase pipeline with automatic cache-corruption recovery.
    pub fn start_folder_pipeline(
        self: &Arc<Self>,
        folder_id: u64,
        sorted_targets: Vec<PreparedTarget>,
    ) {
        self.cancel_current_pipeline();

        let pipeline_id = self.active_pipeline_id.load(Ordering::Acquire);
        self.is_cancelled.store(false, Ordering::Release);

        let total_items = sorted_targets.len();
        if total_items == 0 {
            return;
        }

        let scheduler = self.clone();
        self.pool.spawn(move || {
            scheduler.execute_orchestration(pipeline_id, folder_id, sorted_targets);
        });
    }

    fn execute_orchestration(
        &self,
        pipeline_id: usize,
        folder_id: u64,
        sorted_targets: Vec<PreparedTarget>,
    ) {
        if self.is_pipeline_stale(pipeline_id) {
            return;
        }

        let t_start = Instant::now();
        let total_folder_media = sorted_targets.len();

        let mut existing_locations: Vec<(MediaId, PathBuf, ThumbnailLocation)> = Vec::new();
        let mut missing_targets: Vec<PreparedTarget> = Vec::new();

        // 1. Single LMDB Read Transaction Query
        {
            let rtxn = match self.library.raw_db().env.read_txn() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to acquire LMDB read transaction");
                    return;
                }
            };

            for target in sorted_targets {
                // Check L1 RAM cache first
                if let Some(handle) = self.cache.get(target.media_id)
                    && let Some(rgb_data) = self.cache.arena().read_slot(handle)
                {
                    self.ready_drain_queue.lock().push(ReadyThumbnailItem {
                        media_id: target.media_id,
                        rgb8_pixels: rgb_data.to_vec(),
                    });
                    continue;
                }

                // Check LMDB index for L2 PackStore location
                match self
                    .library
                    .raw_db()
                    .thumbnails
                    .get(&rtxn, &target.media_id)
                {
                    Ok(Some(location)) => {
                        existing_locations.push((target.media_id, target.file_path, location));
                    }
                    Ok(None) => {
                        missing_targets.push(target);
                    }
                    Err(e) => {
                        tracing::warn!(
                            media_id = target.media_id,
                            error = %e,
                            "Failed to read thumbnail index record from LMDB"
                        );
                        missing_targets.push(target);
                    }
                }
            }
        }

        let existing_count = existing_locations.len();
        let missing_count = missing_targets.len();

        tracing::info!(
            folder_id = folder_id,
            total_folder_files = total_folder_media,
            cached_in_db = existing_count,
            missing_to_generate = missing_count,
            l1_ram_slots = self.cache.arena().slot_count(),
            elapsed_ms = t_start.elapsed().as_millis(),
            "=== THUMBNAIL PIPELINE START ==="
        );

        // =========================================================================
        // PHASE 1: Bulk High-Throughput Parallel Preload (With Auto-Recovery)
        // =========================================================================
        let corrupted_items = Arc::new(Mutex::new(Vec::new()));

        if !existing_locations.is_empty() {
            let t_phase1 = Instant::now();
            self.tracker.set_preload_total(existing_count);

            tracing::info!(
                count = existing_count,
                "PHASE 1: Starting Parallel Preload of Cached Thumbnails..."
            );

            use rayon::prelude::*;
            let corrupted_ref = corrupted_items.clone();

            existing_locations
                .into_par_iter()
                .for_each(|(media_id, path, location)| {
                    if self.is_pipeline_stale(pipeline_id) {
                        return;
                    }

                    let mut load_successful = false;

                    if let Ok(compressed_bytes) = self.store.read(location)
                        && let Ok(rgb8_data) = self.generator.decompress_jpeg(&compressed_bytes)
                    {
                        let _ = self.cache.insert(media_id, &rgb8_data);
                        self.ready_drain_queue.lock().push(ReadyThumbnailItem {
                            media_id,
                            rgb8_pixels: rgb8_data,
                        });
                        load_successful = true;
                    }

                    if !load_successful {
                        // Mark broken entry for auto-recovery purge & regeneration
                        corrupted_ref.lock().push(PreparedTarget {
                            media_id,
                            file_path: path,
                        });
                    }

                    self.tracker.increment_preload_done();
                });

            tracing::info!(
                elapsed_ms = t_phase1.elapsed().as_millis(),
                "PHASE 1 Complete. Preload finished."
            );
        }

        if self.is_pipeline_stale(pipeline_id) {
            return;
        }

        // =========================================================================
        // AUTO-RECOVERY: Purge broken entries from LMDB & Re-queue for Phase 2
        // =========================================================================
        let corrupted_list = corrupted_items.lock().drain(..).collect::<Vec<_>>();
        if !corrupted_list.is_empty() {
            let count = corrupted_list.len();
            if let Ok(mut wtxn) = self.library.raw_db().env.write_txn() {
                for item in &corrupted_list {
                    let _ = self
                        .library
                        .raw_db()
                        .thumbnails
                        .delete(&mut wtxn, &item.media_id);
                }
                let _ = wtxn.commit();
            }

            tracing::warn!(
                repaired_count = count,
                "[Auto-Recovery] Detected corrupted or truncated cache entries from prior crash. Purged from LMDB and queued for fresh SIMD generation."
            );

            for item in corrupted_list {
                missing_targets.push(item);
            }
        }

        // =========================================================================
        // PHASE 2: Sorted Missing Thumbnail Generation
        // =========================================================================
        let final_missing_count = missing_targets.len();

        if final_missing_count > 0 {
            self.tracker.set_generate_total(final_missing_count);

            tracing::info!(
                count = final_missing_count,
                "PHASE 2: Starting Ordered Generation of Missing Thumbnails..."
            );

            {
                let mut q = self.generation_queue.lock();
                q.clear();
                for target in missing_targets {
                    q.push_back(target);
                }
            }

            let num_threads = self.pool.current_num_threads();
            for _ in 0..num_threads {
                let scheduler = self.clone_handle();
                self.pool.spawn(move || {
                    scheduler.worker_generation_loop(pipeline_id);
                });
            }
        }
    }

    fn worker_generation_loop(&self, pipeline_id: usize) {
        let mut batched_db_writes: Vec<(MediaId, ThumbnailLocation)> = Vec::with_capacity(16);

        while !self.is_pipeline_stale(pipeline_id) {
            let target = {
                let mut q = self.generation_queue.lock();
                q.pop_front()
            };

            let target = match target {
                Some(t) => t,
                None => break,
            };

            self.tracker.increment_active_workers();
            let t_gen = Instant::now();

            tracing::debug!(
                media_target = %anonymize_target(&target.file_path, Some(target.media_id)),
                "Starting SIMD CatmullRom Thumbnail Generation..."
            );

            let rgb8_res = self
                .generator
                .generate_rgb8(&target.file_path, FitMode::Contain);

            match rgb8_res {
                Ok(rgb8_data) => match self.generator.compress_jpeg(&rgb8_data) {
                    Ok(compressed_bytes) => match self.store.append(&compressed_bytes) {
                        Ok(location) => {
                            tracing::trace!(
                                media_target = %anonymize_target(&target.file_path, Some(target.media_id)),
                                pack_id = location.pack_id,
                                offset = location.offset,
                                length = location.length,
                                total_gen_ms = t_gen.elapsed().as_millis(),
                                "Appended compressed thumbnail to PackStore"
                            );

                            batched_db_writes.push((target.media_id, location));
                            let _ = self.cache.insert(target.media_id, &rgb8_data);

                            self.ready_drain_queue.lock().push(ReadyThumbnailItem {
                                media_id: target.media_id,
                                rgb8_pixels: rgb8_data,
                            });
                        }
                        Err(e) => {
                            tracing::error!(
                                media_target = %anonymize_target(&target.file_path, Some(target.media_id)),
                                error = %e,
                                "Failed to append to PackStore"
                            );
                        }
                    },
                    Err(e) => {
                        tracing::error!(
                            media_target = %anonymize_target(&target.file_path, Some(target.media_id)),
                            error = %e,
                            "JPEG compression failed"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        media_target = %anonymize_target(&target.file_path, Some(target.media_id)),
                        error = %e,
                        "Failed to generate thumbnail frame"
                    );
                }
            }

            if batched_db_writes.len() >= 16 {
                self.flush_db_batch(&mut batched_db_writes);
            }

            self.tracker.increment_generate_done();
            self.tracker.decrement_active_workers();
        }

        if !batched_db_writes.is_empty() {
            self.flush_db_batch(&mut batched_db_writes);
        }
    }

    fn flush_db_batch(&self, batch: &mut Vec<(MediaId, ThumbnailLocation)>) {
        if batch.is_empty() {
            return;
        }

        let t_db = Instant::now();
        if let Ok(mut wtxn) = self.library.raw_db().env.write_txn() {
            let count = batch.len();
            for (id, loc) in batch.drain(..) {
                let _ = self.library.raw_db().thumbnails.put(&mut wtxn, &id, &loc);
            }
            let _ = wtxn.commit();
            tracing::debug!(
                batch_size = count,
                elapsed_ms = t_db.elapsed().as_millis(),
                "Committed batched thumbnail metadata records to LMDB"
            );
        }
    }

    fn is_pipeline_stale(&self, pipeline_id: usize) -> bool {
        self.is_cancelled.load(Ordering::Acquire)
            || self.active_pipeline_id.load(Ordering::Acquire) != pipeline_id
    }

    fn clone_handle(&self) -> Self {
        Self {
            library: self.library.clone(),
            cache: self.cache.clone(),
            store: self.store.clone(),
            generator: self.generator.clone(),
            pool: self.pool.clone(),
            tracker: self.tracker.clone(),
            ready_drain_queue: self.ready_drain_queue.clone(),
            generation_queue: self.generation_queue.clone(),
            active_pipeline_id: self.active_pipeline_id.clone(),
            is_cancelled: self.is_cancelled.clone(),
        }
    }

    pub fn load_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
    ) -> Result<ThumbnailHandle, ThumbnailError> {
        if let Some(handle) = self.cache.get(media_id) {
            return Ok(handle);
        }

        let location = {
            let rtxn = self.library.raw_db().env.read_txn()?;
            self.library.raw_db().thumbnails.get(&rtxn, &media_id)?
        };

        if let Some(loc) = location
            && let Ok(compressed) = self.store.read(loc)
            && let Ok(rgb8) = self.generator.decompress_jpeg(&compressed)
            && let Some(handle) = self.cache.insert(media_id, &rgb8)
        {
            return Ok(handle);
        }

        let rgb8 = self.generator.generate_rgb8(file_path, FitMode::Contain)?;
        let compressed = self.generator.compress_jpeg(&rgb8)?;
        let location = self.store.append(&compressed)?;

        {
            let mut wtxn = self.library.raw_db().env.write_txn()?;
            self.library
                .raw_db()
                .thumbnails
                .put(&mut wtxn, &media_id, &location)?;
            wtxn.commit()?;
        }

        let handle = self
            .cache
            .insert(media_id, &rgb8)
            .ok_or(ThumbnailError::ArenaExhausted)?;
        Ok(handle)
    }
}
