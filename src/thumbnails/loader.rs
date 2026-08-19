use super::cache::L1ThumbnailCache;
use super::error::ThumbnailError;
use super::generator::ThumbnailGenerator;
use super::progress::{ThumbnailProgressSnapshot, ThumbnailProgressTracker};
use super::store::PackStore;
use super::types::{
    ThumbnailConfig, ThumbnailFormat, ThumbnailLocation, ThumbnailStorageStats, VideoPreviewRecord,
};
use crate::library::models::MediaId;
use crate::library::Library;
use crate::media::{MediaFormat, MediaType};
use crate::telemetry::benchmark::ThumbnailPipelineBenchmark;
use parking_lot::{Mutex, RwLock};
use rayon::{ThreadPool, ThreadPoolBuilder};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
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
    pub pixel_buffer: SharedPixelBuffer<Rgb8Pixel>,
}

pub struct ThumbnailScheduler {
    pub library: Arc<Library>,
    cache: Arc<L1ThumbnailCache>,
    store: Arc<PackStore>,
    generator: Arc<ThumbnailGenerator>,
    pool: Arc<ThreadPool>,
    tracker: Arc<ThumbnailProgressTracker>,
    ready_drain_queue: Arc<Mutex<Vec<ReadyThumbnailItem>>>,
    generation_queue: Arc<Mutex<VecDeque<PreparedTarget>>>,
    active_pipeline_id: Arc<AtomicUsize>,
    is_cancelled: Arc<AtomicBool>,
    active_benchmark: Arc<RwLock<Option<Arc<ThumbnailPipelineBenchmark>>>>,
    active_in_flight: Arc<Mutex<HashSet<MediaId>>>,
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

        Self {
            library,
            cache,
            store,
            generator: Arc::new(ThumbnailGenerator::new()),
            pool,
            tracker: Arc::new(ThumbnailProgressTracker::new()),
            ready_drain_queue: Arc::new(Mutex::new(Vec::with_capacity(2048))),
            generation_queue: Arc::new(Mutex::new(VecDeque::new())),
            active_pipeline_id: Arc::new(AtomicUsize::new(0)),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            active_benchmark: Arc::new(RwLock::new(None)),
            active_in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn current_pipeline_id(&self) -> usize {
        self.active_pipeline_id.load(Ordering::Acquire)
    }

    pub fn active_benchmark(&self) -> Option<Arc<ThumbnailPipelineBenchmark>> {
        self.active_benchmark.read().clone()
    }

    pub fn is_drain_queue_empty(&self) -> bool {
        self.ready_drain_queue.lock().is_empty()
    }

    pub fn drain_ready_batch(&self, max_items: usize) -> Vec<ReadyThumbnailItem> {
        let current_pipeline = self.active_pipeline_id.load(Ordering::Acquire);
        let mut queue = self.ready_drain_queue.lock();
        if queue.is_empty() {
            return Vec::new();
        }

        queue.retain(|item| item.pipeline_id == current_pipeline);

        let take_count = queue.len().min(max_items);
        let items: Vec<ReadyThumbnailItem> = queue.drain(0..take_count).collect();

        let mut in_flight = self.active_in_flight.lock();
        for item in &items {
            in_flight.remove(&item.media_id);
        }

        items
    }

    pub fn snapshot_progress(&self) -> ThumbnailProgressSnapshot {
        self.tracker.snapshot()
    }

    pub fn tracker(&self) -> &ThumbnailProgressTracker {
        &self.tracker
    }

    pub fn storage_stats(&self) -> ThumbnailStorageStats {
        self.store.storage_stats(&self.library)
    }

    pub fn cancel_current_pipeline(&self) {
        self.is_cancelled.store(true, Ordering::Release);
        self.active_pipeline_id.fetch_add(1, Ordering::SeqCst);
        self.generation_queue.lock().clear();
        self.ready_drain_queue.lock().clear();
        self.active_in_flight.lock().clear();
        self.tracker.reset();
        *self.active_benchmark.write() = None;
    }

    /// Fast, non-blocking synchronous check for an existing thumbnail in L1 cache or L2 PackStore.
    /// Used by viewport virtualization during scrolling to display existing thumbnails without triggering generator threads.
    pub fn try_load_existing(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        // 1. L1 RAM Cache check
        if let Some(cached) = self.cache.get(media_id) {
            let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(cached.width, cached.height);
            pixel_buffer.make_mut_bytes().copy_from_slice(&cached.pixels);
            return Some(pixel_buffer);
        }

        // 2. L2 PackStore check
        let location = {
            let rtxn = self.library.raw_db().env.read_txn().ok()?;
            self.library.raw_db().thumbnails.get(&rtxn, &media_id).ok().flatten()?
        };

        if let Ok(compressed_bytes) = self.store.read(location)
            && let Ok((w, h, rgb8_data)) = self.generator.decompress(&compressed_bytes)
        {
            let _ = self.cache.insert(media_id, w, h, &rgb8_data);
            let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
            pixel_buffer.make_mut_bytes().copy_from_slice(&rgb8_data);
            return Some(pixel_buffer);
        }

        None
    }

    /// Launches dedicated full-folder thumbnail pipeline processing ALL items in the folder.
    /// Phase 1 preloads all existing thumbnails; Phase 2 generates missing thumbnails for all remaining items.
    pub fn start_folder_pipeline(
        self: &Arc<Self>,
        folder_id: u64,
        mut all_targets: Vec<PreparedTarget>,
        config: ThumbnailConfig,
    ) {
        self.cancel_current_pipeline();

        let total_items = all_targets.len();
        if total_items == 0 {
            return;
        }

        let pipeline_id = self.active_pipeline_id.load(Ordering::Acquire);
        self.is_cancelled.store(false, Ordering::Release);

        tracing::info!(
            folder_id = folder_id,
            pipeline_id = pipeline_id,
            total_items = total_items,
            "[ThumbnailPipeline: Start] Initializing dedicated full-folder thumbnail pipeline..."
        );

        let bench = Arc::new(ThumbnailPipelineBenchmark::new(pipeline_id, folder_id, total_items));
        *self.active_benchmark.write() = Some(bench.clone());

        for target in all_targets.iter_mut() {
            target.pipeline_id = pipeline_id;
            target.folder_id = folder_id;
        }

        let scheduler = self.clone();
        let bench_clone = bench;
        self.pool.spawn(move || {
            scheduler.execute_orchestration(
                pipeline_id,
                folder_id,
                all_targets,
                config,
                bench_clone,
            );
        });
    }

    fn execute_orchestration(
        &self,
        pipeline_id: usize,
        folder_id: u64,
        all_targets: Vec<PreparedTarget>,
        config: ThumbnailConfig,
        benchmark: Arc<ThumbnailPipelineBenchmark>,
    ) {
        if self.is_pipeline_stale(pipeline_id) {
            return;
        }

        let mut existing_locations: Vec<(MediaId, PathBuf, ThumbnailLocation)> = Vec::new();
        let mut missing_targets: Vec<PreparedTarget> = Vec::new();

        // 1. Single LMDB Read Transaction Query across all targets
        {
            let rtxn = match self.library.raw_db().env.read_txn() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to acquire LMDB read transaction for thumbnail pipeline");
                    return;
                }
            };

            for target in all_targets {
                if self.is_pipeline_stale(pipeline_id) {
                    return;
                }

                if let Some(cached) = self.cache.get(target.media_id) {
                    benchmark.record_l1_hit();
                    let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(cached.width, cached.height);
                    pixel_buffer.make_mut_bytes().copy_from_slice(&cached.pixels);

                    let mut q = self.ready_drain_queue.lock();
                    q.push(ReadyThumbnailItem {
                        pipeline_id,
                        folder_id,
                        media_id: target.media_id,
                        width: cached.width,
                        height: cached.height,
                        pixel_buffer,
                    });
                    benchmark.record_queue_depth(q.len());
                    continue;
                }

                benchmark.record_l1_miss();

                match self.library.raw_db().thumbnails.get(&rtxn, &target.media_id) {
                    Ok(Some(location)) => {
                        existing_locations.push((target.media_id, target.file_path, location));
                    }
                    Ok(None) => {
                        missing_targets.push(target);
                    }
                    Err(_) => {
                        missing_targets.push(target);
                    }
                }
            }
        }

        let existing_count = existing_locations.len();
        let missing_count = missing_targets.len();

        tracing::info!(
            folder_id = folder_id,
            pipeline_id = pipeline_id,
            existing_cached = existing_count,
            missing_to_generate = missing_count,
            "[ThumbnailPipeline: Orchestration] Segregated items: preloading cached and generating missing..."
        );

        // =========================================================================
        // PHASE 1: Bulk High-Throughput Parallel Preload (With Auto-Recovery)
        // =========================================================================
        let corrupted_items = Arc::new(Mutex::new(Vec::new()));

        if !existing_locations.is_empty() {
            let t_phase1 = Instant::now();
            self.tracker.set_preload_total(existing_count);

            use rayon::prelude::*;
            let corrupted_ref = corrupted_items.clone();
            let bench_ref = benchmark.clone();

            existing_locations
                .into_par_iter()
                .for_each(|(media_id, path, location)| {
                    if self.is_pipeline_stale(pipeline_id) {
                        return;
                    }

                    let mut load_successful = false;
                    let t_read = Instant::now();

                    if let Ok(compressed_bytes) = self.store.read(location) {
                        let is_webp = ThumbnailFormat::detect(&compressed_bytes)
                            != Some(ThumbnailFormat::Jpeg);
                        bench_ref.record_pack_read(
                            compressed_bytes.len() as u64,
                            t_read.elapsed().as_micros() as u64,
                        );

                        let t_dec = Instant::now();
                        if let Ok((w, h, rgb8_data)) = self.generator.decompress(&compressed_bytes) {
                            let raw_len = rgb8_data.len() as u64;
                            bench_ref.record_decompress(
                                is_webp,
                                raw_len,
                                t_dec.elapsed().as_micros() as u64,
                            );

                            if !self.is_pipeline_stale(pipeline_id) {
                                let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
                                pixel_buffer.make_mut_bytes().copy_from_slice(&rgb8_data);

                                let _ = self.cache.insert(media_id, w, h, &rgb8_data);
                                let mut q = self.ready_drain_queue.lock();
                                q.push(ReadyThumbnailItem {
                                    pipeline_id,
                                    folder_id,
                                    media_id,
                                    width: w,
                                    height: h,
                                    pixel_buffer,
                                });
                                bench_ref.record_queue_depth(q.len());
                                load_successful = true;
                            }
                        }
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

            benchmark.set_preload_duration(t_phase1.elapsed().as_micros() as u64);
        }

        if self.is_pipeline_stale(pipeline_id) {
            return;
        }

        // Auto-Recovery: Purge corrupted entries and re-queue for Phase 2 generation
        let corrupted_list = corrupted_items.lock().drain(..).collect::<Vec<_>>();
        if !corrupted_list.is_empty() {
            let count = corrupted_list.len();
            benchmark.record_corrupted_repaired(count);

            if let Ok(mut wtxn) = self.library.raw_db().env.write_txn() {
                for item in &corrupted_list {
                    let _ = self.library.raw_db().thumbnails.delete(&mut wtxn, &item.media_id);
                    let _ = self.library.raw_db().video_previews.delete(&mut wtxn, &item.media_id);
                }
                let _ = wtxn.commit();
            }

            for item in corrupted_list {
                missing_targets.push(item);
            }
        }

        // =========================================================================
        // PHASE 2: Dedicated Missing Thumbnail Generation for ALL Missing Items
        // =========================================================================
        let final_missing_count = missing_targets.len();

        if final_missing_count > 0 {
            self.tracker.set_generate_total(final_missing_count);

            tracing::info!(
                folder_id = folder_id,
                missing_count = final_missing_count,
                threads = self.pool.current_num_threads(),
                "[ThumbnailPipeline: Phase 2] Generating missing thumbnails dedicatedly for all items..."
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
                    scheduler.worker_generation_loop(
                        pipeline_id,
                        folder_id,
                        cfg,
                        bench_gen,
                        t_p2,
                    );
                });
            }
        }
    }

    fn worker_generation_loop(
        &self,
        pipeline_id: usize,
        folder_id: u64,
        config: ThumbnailConfig,
        benchmark: Arc<ThumbnailPipelineBenchmark>,
        t_phase2_start: Arc<Instant>,
    ) {
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

            let is_video =
                MediaFormat::from_path(&target.file_path).media_type() == Some(MediaType::Video);

            if is_video {
                let t_vid = Instant::now();
                if let Ok(video_res) = self
                    .generator
                    .generate_video_thumbnails(&target.file_path, &config)
                {
                    benchmark.record_video_extract(t_vid.elapsed().as_micros() as u64);

                    if self.is_pipeline_stale(pipeline_id) {
                        self.tracker.decrement_active_workers();
                        break;
                    }

                    let primary = video_res.primary_frame;
                    let t_comp = Instant::now();

                    if let Ok(compressed_primary) = self.generator.compress(
                        &primary.rgb_pixels,
                        primary.width,
                        primary.height,
                        config.format,
                        config.quality,
                    ) {
                        benchmark.record_compress(
                            compressed_primary.len() as u64,
                            t_comp.elapsed().as_micros() as u64,
                        );

                        let t_app = Instant::now();
                        if let Ok(primary_loc) = self.store.append(&compressed_primary) {
                            benchmark.record_pack_append(
                                compressed_primary.len() as u64,
                                t_app.elapsed().as_micros() as u64,
                            );

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

                            if !self.is_pipeline_stale(pipeline_id) {
                                batched_db_writes.push((
                                    target.media_id,
                                    primary_loc,
                                    preview_rec,
                                ));

                                let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(primary.width, primary.height);
                                pixel_buffer.make_mut_bytes().copy_from_slice(&primary.rgb_pixels);

                                let _ = self.cache.insert(
                                    target.media_id,
                                    primary.width,
                                    primary.height,
                                    &primary.rgb_pixels,
                                );

                                let mut q = self.ready_drain_queue.lock();
                                q.push(ReadyThumbnailItem {
                                    pipeline_id,
                                    folder_id,
                                    media_id: target.media_id,
                                    width: primary.width,
                                    height: primary.height,
                                    pixel_buffer,
                                });
                                benchmark.record_queue_depth(q.len());
                            }
                        }
                    }
                }
            } else {
                let t_img = Instant::now();
                if let Ok(frame) = self.generator.generate_image_thumbnail(
                    &target.file_path,
                    config.max_bound,
                    config.scaling_algorithm,
                ) {
                    benchmark.record_image_decode_resize(t_img.elapsed().as_micros() as u64);

                    let t_comp = Instant::now();
                    if let Ok(compressed_bytes) = self.generator.compress(
                        &frame.rgb_pixels,
                        frame.width,
                        frame.height,
                        config.format,
                        config.quality,
                    ) {
                        benchmark.record_compress(
                            compressed_bytes.len() as u64,
                            t_comp.elapsed().as_micros() as u64,
                        );

                        let t_app = Instant::now();
                        if let Ok(location) = self.store.append(&compressed_bytes) {
                            benchmark.record_pack_append(
                                compressed_bytes.len() as u64,
                                t_app.elapsed().as_micros() as u64,
                            );

                            if !self.is_pipeline_stale(pipeline_id) {
                                batched_db_writes.push((
                                    target.media_id,
                                    location,
                                    None,
                                ));

                                let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(frame.width, frame.height);
                                pixel_buffer.make_mut_bytes().copy_from_slice(&frame.rgb_pixels);

                                let _ = self.cache.insert(
                                    target.media_id,
                                    frame.width,
                                    frame.height,
                                    &frame.rgb_pixels,
                                );

                                let mut q = self.ready_drain_queue.lock();
                                q.push(ReadyThumbnailItem {
                                    pipeline_id,
                                    folder_id,
                                    media_id: target.media_id,
                                    width: frame.width,
                                    height: frame.height,
                                    pixel_buffer,
                                });
                                benchmark.record_queue_depth(q.len());
                            }
                        }
                    }
                }
            }

            if batched_db_writes.len() >= config.batch_commit_size {
                self.flush_db_batch(&mut batched_db_writes, &benchmark);
            }

            self.tracker.increment_generate_done();
            self.tracker.decrement_active_workers();
        }

        if !batched_db_writes.is_empty() {
            self.flush_db_batch(&mut batched_db_writes, &benchmark);
        }

        benchmark.set_generate_duration(t_phase2_start.elapsed().as_micros() as u64);
    }

    fn flush_db_batch(
        &self,
        batch: &mut Vec<(MediaId, ThumbnailLocation, Option<VideoPreviewRecord>)>,
        benchmark: &ThumbnailPipelineBenchmark,
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
            benchmark.record_lmdb_commit(count, t_db.elapsed().as_micros() as u64);
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
            active_benchmark: self.active_benchmark.clone(),
            active_in_flight: self.active_in_flight.clone(),
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

    /// Explicitly regenerates thumbnail & previews from physical file, overwriting PackStore and LMDB index.
    pub fn regenerate_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<super::cache::CachedThumbnail, ThumbnailError> {
        let is_video = MediaFormat::from_path(file_path).media_type() == Some(MediaType::Video);
        let (frame, video_preview_rec) = if is_video {
            let video_res = self
                .generator
                .generate_video_thumbnails(file_path, config)?;
            let primary = video_res.primary_frame;
            let mut preview_rec = None;
            if !video_res.preview_frames.is_empty() {
                let (p_format, p_quality) = if config.custom_preview_settings {
                    (config.preview_format, config.preview_quality)
                } else {
                    (config.format, config.quality)
                };

                let mut locations = Vec::with_capacity(video_res.preview_frames.len());
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
            (primary, preview_rec)
        } else {
            let f = self.generator.generate_image_thumbnail(
                file_path,
                config.max_bound,
                config.scaling_algorithm,
            )?;
            (f, None)
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
            if let Some(p_rec) = video_preview_rec {
                self.library
                    .raw_db()
                    .video_previews
                    .put(&mut wtxn, &media_id, &p_rec)?;
            } else {
                let _ = self
                    .library
                    .raw_db()
                    .video_previews
                    .delete(&mut wtxn, &media_id);
            }
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
