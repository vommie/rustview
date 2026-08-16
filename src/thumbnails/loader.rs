use super::cache::L1ThumbnailCache;
use super::error::ThumbnailError;
use super::generator::ThumbnailGenerator;
use super::progress::{ThumbnailProgressSnapshot, ThumbnailProgressTracker};
use super::store::PackStore;
use super::types::{ThumbnailConfig, ThumbnailLocation, VideoPreviewRecord, anonymize_target};
use crate::library::Library;
use crate::library::models::MediaId;
use crate::media::{MediaFormat, MediaType};
use parking_lot::Mutex;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct PreparedTarget {
    pub pipeline_id: usize,
    pub folder_id: u64,
    pub media_id: MediaId,
    pub file_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ReadyThumbnailItem {
    pub pipeline_id: usize,
    pub folder_id: u64,
    pub media_id: MediaId,
    pub width: u32,
    pub height: u32,
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

    pub fn current_pipeline_id(&self) -> usize {
        self.active_pipeline_id.load(Ordering::Acquire)
    }

    pub fn drain_ready_batch(&self, max_items: usize) -> Vec<ReadyThumbnailItem> {
        let current_pipeline = self.active_pipeline_id.load(Ordering::Acquire);
        let mut queue = self.ready_drain_queue.lock();
        if queue.is_empty() {
            return Vec::new();
        }

        // Strictly filter out and discard any thumbnail from obsolete/cancelled pipelines
        queue.retain(|item| item.pipeline_id == current_pipeline);

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

    pub fn start_folder_pipeline(
        self: &Arc<Self>,
        folder_id: u64,
        mut sorted_targets: Vec<PreparedTarget>,
        config: ThumbnailConfig,
    ) {
        self.cancel_current_pipeline();

        let pipeline_id = self.active_pipeline_id.load(Ordering::Acquire);
        self.is_cancelled.store(false, Ordering::Release);

        for target in sorted_targets.iter_mut() {
            target.pipeline_id = pipeline_id;
            target.folder_id = folder_id;
        }

        let total_items = sorted_targets.len();
        if total_items == 0 {
            return;
        }

        let scheduler = self.clone();
        self.pool.spawn(move || {
            scheduler.execute_orchestration(pipeline_id, folder_id, sorted_targets, config);
        });
    }

    fn execute_orchestration(
        &self,
        pipeline_id: usize,
        folder_id: u64,
        sorted_targets: Vec<PreparedTarget>,
        config: ThumbnailConfig,
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
                if self.is_pipeline_stale(pipeline_id) {
                    return;
                }

                // Check L1 RAM cache first
                if let Some(cached) = self.cache.get(target.media_id) {
                    self.ready_drain_queue.lock().push(ReadyThumbnailItem {
                        pipeline_id,
                        folder_id,
                        media_id: target.media_id,
                        width: cached.width,
                        height: cached.height,
                        rgb8_pixels: cached.pixels.as_ref().clone(),
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
            pipeline_id = pipeline_id,
            folder_id = folder_id,
            total_folder_files = total_folder_media,
            cached_in_db = existing_count,
            missing_to_generate = missing_count,
            l1_ram_slots = self.cache.capacity(),
            target_db_size = config.max_bound,
            target_format = %config.format.as_str(),
            scaling_algo = %config.scaling_algorithm.as_str(),
            video_engine = %config.video_engine.as_str(),
            video_offset = config.video_offset_percent,
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
                        && let Ok((w, h, rgb8_data)) = self.generator.decompress(&compressed_bytes)
                        && !self.is_pipeline_stale(pipeline_id)
                    {
                        let _ = self.cache.insert(media_id, w, h, &rgb8_data);
                        self.ready_drain_queue.lock().push(ReadyThumbnailItem {
                            pipeline_id,
                            folder_id,
                            media_id,
                            width: w,
                            height: h,
                            rgb8_pixels: rgb8_data,
                        });
                        load_successful = true;
                    }

                    if !load_successful && !self.is_pipeline_stale(pipeline_id) {
                        corrupted_ref.lock().push(PreparedTarget {
                            pipeline_id,
                            folder_id,
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

        // Auto-Recovery: Purge broken entries and re-queue for Phase 2
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
                    let _ = self
                        .library
                        .raw_db()
                        .video_previews
                        .delete(&mut wtxn, &item.media_id);
                }
                let _ = wtxn.commit();
            }

            tracing::warn!(
                repaired_count = count,
                "[Auto-Recovery] Detected corrupted or truncated cache entries. Purged and queued for regeneration."
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
                let cfg = config.clone();
                self.pool.spawn(move || {
                    scheduler.worker_generation_loop(pipeline_id, folder_id, cfg);
                });
            }
        }
    }

    fn worker_generation_loop(&self, pipeline_id: usize, folder_id: u64, config: ThumbnailConfig) {
        let mut batched_db_writes: Vec<(MediaId, ThumbnailLocation, Option<VideoPreviewRecord>)> =
            Vec::with_capacity(config.batch_commit_size);

        while !self.is_pipeline_stale(pipeline_id) {
            let target = {
                let mut q = self.generation_queue.lock();
                q.pop_front()
            };

            let target = match target {
                Some(t) => t,
                None => break,
            };

            if self.is_pipeline_stale(pipeline_id) {
                break;
            }

            self.tracker.increment_active_workers();
            let t_gen = Instant::now();

            let is_video =
                MediaFormat::from_path(&target.file_path).media_type() == Some(MediaType::Video);

            if is_video {
                // =============================================================
                // UNIFIED VIDEO EXTRACTION (Primary + Previews in 1 Open Pass)
                // =============================================================
                match self
                    .generator
                    .generate_video_thumbnails(&target.file_path, &config)
                {
                    Ok(video_res) => {
                        if self.is_pipeline_stale(pipeline_id) {
                            self.tracker.decrement_active_workers();
                            break;
                        }

                        let primary = video_res.primary_frame;
                        match self.generator.compress(
                            &primary.rgb_pixels,
                            primary.width,
                            primary.height,
                            config.format,
                            config.quality,
                        ) {
                            Ok(compressed_primary) => {
                                match self.store.append(&compressed_primary) {
                                    Ok(primary_loc) => {
                                        let mut preview_rec = None;
                                        if !video_res.preview_frames.is_empty() {
                                            let (p_format, p_quality) =
                                                if config.custom_preview_settings {
                                                    (config.preview_format, config.preview_quality)
                                                } else {
                                                    (config.format, config.quality)
                                                };

                                            let mut locations =
                                                Vec::with_capacity(video_res.preview_frames.len());
                                            for p_frame in &video_res.preview_frames {
                                                if let Ok(p_bytes) = self.generator.compress(
                                                    &p_frame.rgb_pixels,
                                                    p_frame.width,
                                                    p_frame.height,
                                                    p_format,
                                                    p_quality,
                                                ) && let Ok(p_loc) = self.store.append(&p_bytes)
                                                {
                                                    locations.push(p_loc);
                                                }
                                            }
                                            if !locations.is_empty() {
                                                preview_rec = Some(VideoPreviewRecord {
                                                    frame_count: locations.len() as u32,
                                                    frame_locations: locations,
                                                });
                                            }
                                        }

                                        tracing::trace!(
                                            media_target = %anonymize_target(&target.file_path, Some(target.media_id)),
                                            width = primary.width,
                                            height = primary.height,
                                            preview_frames = video_res.preview_frames.len(),
                                            total_gen_ms = t_gen.elapsed().as_millis(),
                                            "Saved video thumbnail and previews to PackStore"
                                        );

                                        if !self.is_pipeline_stale(pipeline_id) {
                                            batched_db_writes.push((
                                                target.media_id,
                                                primary_loc,
                                                preview_rec,
                                            ));
                                            let _ = self.cache.insert(
                                                target.media_id,
                                                primary.width,
                                                primary.height,
                                                &primary.rgb_pixels,
                                            );

                                            self.ready_drain_queue.lock().push(ReadyThumbnailItem {
                                                pipeline_id,
                                                folder_id,
                                                media_id: target.media_id,
                                                width: primary.width,
                                                height: primary.height,
                                                rgb8_pixels: primary.rgb_pixels,
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(target = ?target.file_path, error = %e, "Failed to append primary video thumb to PackStore");
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(target = ?target.file_path, error = %e, "Primary video thumb compression failed");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(target = ?target.file_path, error = %e, "Failed to extract video thumbnail");
                    }
                }
            } else {
                // =============================================================
                // IMAGE EXTRACTION
                // =============================================================
                match self.generator.generate_image_thumbnail(
                    &target.file_path,
                    config.max_bound,
                    config.scaling_algorithm,
                ) {
                    Ok(frame) => match self.generator.compress(
                        &frame.rgb_pixels,
                        frame.width,
                        frame.height,
                        config.format,
                        config.quality,
                    ) {
                        Ok(compressed_bytes) => match self.store.append(&compressed_bytes) {
                            Ok(location) => {
                                if !self.is_pipeline_stale(pipeline_id) {
                                    batched_db_writes.push((target.media_id, location, None));
                                    let _ = self.cache.insert(
                                        target.media_id,
                                        frame.width,
                                        frame.height,
                                        &frame.rgb_pixels,
                                    );

                                    self.ready_drain_queue.lock().push(ReadyThumbnailItem {
                                        pipeline_id,
                                        folder_id,
                                        media_id: target.media_id,
                                        width: frame.width,
                                        height: frame.height,
                                        rgb8_pixels: frame.rgb_pixels,
                                    });
                                }
                            }
                            Err(e) => {
                                tracing::error!(target = ?target.file_path, error = %e, "Failed to append image thumb to PackStore");
                            }
                        },
                        Err(e) => {
                            tracing::error!(target = ?target.file_path, error = %e, "Image thumb compression failed");
                        }
                    },
                    Err(e) => {
                        tracing::warn!(target = ?target.file_path, error = %e, "Failed to decode image");
                    }
                }
            }

            if batched_db_writes.len() >= config.batch_commit_size {
                self.flush_db_batch(&mut batched_db_writes);
            }

            self.tracker.increment_generate_done();
            self.tracker.decrement_active_workers();
        }

        if !batched_db_writes.is_empty() {
            self.flush_db_batch(&mut batched_db_writes);
        }
    }

    fn flush_db_batch(
        &self,
        batch: &mut Vec<(MediaId, ThumbnailLocation, Option<VideoPreviewRecord>)>,
    ) {
        if batch.is_empty() {
            return;
        }

        let t_db = Instant::now();
        if let Ok(mut wtxn) = self.library.raw_db().env.write_txn() {
            let count = batch.len();
            for (id, loc, preview_rec) in batch.drain(..) {
                let _ = self.library.raw_db().thumbnails.put(&mut wtxn, &id, &loc);
                if let Some(p_rec) = preview_rec {
                    let _ = self
                        .library
                        .raw_db()
                        .video_previews
                        .put(&mut wtxn, &id, &p_rec);
                }
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
        config: ThumbnailConfig,
    ) -> Result<super::cache::CachedThumbnail, ThumbnailError> {
        if let Some(item) = self.cache.get(media_id) {
            return Ok(item);
        }

        let location = {
            let rtxn = self.library.raw_db().env.read_txn()?;
            self.library.raw_db().thumbnails.get(&rtxn, &media_id)?
        };

        if let Some(loc) = location
            && let Ok(compressed) = self.store.read(loc)
            && let Ok((w, h, rgb8)) = self.generator.decompress(&compressed)
        {
            let item = self.cache.insert(media_id, w, h, &rgb8);
            return Ok(item);
        }

        let is_video = MediaFormat::from_path(file_path).media_type() == Some(MediaType::Video);
        let frame = if is_video {
            self.generator
                .generate_video_thumbnails(file_path, &config)?
                .primary_frame
        } else {
            self.generator.generate_image_thumbnail(
                file_path,
                config.max_bound,
                config.scaling_algorithm,
            )?
        };

        let compressed = self.generator.compress(
            &frame.rgb_pixels,
            frame.width,
            frame.height,
            config.format,
            config.quality,
        )?;
        let location = self.store.append(&compressed)?;

        {
            let mut wtxn = self.library.raw_db().env.write_txn()?;
            self.library
                .raw_db()
                .thumbnails
                .put(&mut wtxn, &media_id, &location)?;
            wtxn.commit()?;
        }

        let item = self
            .cache
            .insert(media_id, frame.width, frame.height, &frame.rgb_pixels);
        Ok(item)
    }

    pub fn load_preview_frames_sync(
        &self,
        media_id: MediaId,
    ) -> Result<Vec<(u32, u32, Vec<u8>)>, ThumbnailError> {
        let record = {
            let rtxn = self.library.raw_db().env.read_txn()?;
            self.library.raw_db().video_previews.get(&rtxn, &media_id)?
        };

        let Some(p_rec) = record else {
            return Err(ThumbnailError::Ffmpeg(
                "No video preview found in database".into(),
            ));
        };

        let mut frames = Vec::with_capacity(p_rec.frame_locations.len());
        for loc in p_rec.frame_locations {
            if let Ok(bytes) = self.store.read(loc)
                && let Ok((w, h, rgb8)) = self.generator.decompress(&bytes)
            {
                frames.push((w, h, rgb8));
            }
        }

        if frames.is_empty() {
            return Err(ThumbnailError::Ffmpeg(
                "Failed to decompress video preview frames".into(),
            ));
        }

        Ok(frames)
    }
}
