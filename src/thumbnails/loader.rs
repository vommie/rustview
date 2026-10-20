use super::cache::L1AnimatedPreviewCache;
use super::error::ThumbnailError;
use super::generator::ThumbnailGenerator;
use super::gif::{AnimatedPreview, GifThumbnailer};
use super::progress::{ThumbnailProgressSnapshot, ThumbnailProgressTracker};
use super::ram_cache::{CachedThumbnail, ThumbnailRamCache};
use super::ready_queue::ReadyQueue;
use super::store::PackStore;
use super::types::{
    ThumbnailConfig, ThumbnailLocation, ThumbnailStorageStats, VideoPreviewRecord,
};
use super::view_loader::{ViewLoader, ViewLoaderDeps};
use crate::library::models::MediaId;
use crate::library::Library;
use crate::media::{MediaFormat, MediaType};
use crate::telemetry::benchmark::ThumbnailPipelineBenchmark;
use parking_lot::{Mutex, RwLock};
use rayon::{ThreadPool, ThreadPoolBuilder};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Threads that decode the thumbnails the Browse view shows (small decodes, latency matters more than throughput): half of the
/// worker threads, between 2 and 4, so a scrollbar jump on a many-core machine fills its tiles faster without starving the UI.
fn view_loader_threads(worker_threads: usize) -> usize {
    (worker_threads / 2).clamp(2, 4)
}

/// Pipeline id of the thumbnails that the view loader delivers: they belong to no folder pipeline, so starting or cancelling
/// one never discards them (the view applies a thumbnail only if its tile still exists).
pub const VIEW_LOAD_PIPELINE_ID: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbedMetadata {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
struct BatchedDbWriteItem {
    media_id: MediaId,
    location: Option<ThumbnailLocation>,
    preview_record: Option<VideoPreviewRecord>,
    probed_metadata: Option<ProbedMetadata>,
    is_corrupted: bool,
}

/// What the folder pipeline needs to know about one item. The file path is not part of it: the scan resolves it from the library
/// for the few items that still need a thumbnail, so starting a pipeline for a folder of millions of items copies no paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineItem {
    pub media_id: MediaId,
    pub folder_id: u64,
    pub is_video: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedTarget {
    pub pipeline_id: usize,
    pub folder_id: u64,
    pub media_id: MediaId,
    pub file_path: PathBuf,
    /// From the library record: a file's content decides image or video, not its extension.
    pub is_video: bool,
}

#[derive(Debug, Clone)]
pub struct ReadyThumbnailItem {
    pub pipeline_id: usize,
    pub folder_id: u64,
    pub media_id: MediaId,
    pub width: u32,
    pub height: u32,
    pub pixel_buffer: Option<SharedPixelBuffer<Rgb8Pixel>>,
    pub orig_width: u32,
    pub orig_height: u32,
    pub duration_ms: u64,
    pub is_corrupted: bool,
}

pub struct ThumbnailScheduler {
    pub library: Arc<Library>,
    cache: Arc<ThumbnailRamCache>,
    animated_cache: Arc<L1AnimatedPreviewCache>,
    store: Arc<PackStore>,
    generator: Arc<ThumbnailGenerator>,
    pool: Arc<ThreadPool>,
    view_loader: Arc<ViewLoader>,
    tracker: Arc<ThumbnailProgressTracker>,
    ready_drain_queue: Arc<ReadyQueue>,
    generation_queue: Arc<Mutex<VecDeque<PreparedTarget>>>,
    active_pipeline_id: Arc<AtomicUsize>,
    is_cancelled: Arc<AtomicBool>,
    active_benchmark: Arc<RwLock<Option<Arc<ThumbnailPipelineBenchmark>>>>,
    active_in_flight: Arc<Mutex<HashSet<MediaId>>>,
    current_config: Arc<RwLock<ThumbnailConfig>>,
}

impl ThumbnailScheduler {
    pub fn new(
        library: Arc<Library>,
        cache: Arc<ThumbnailRamCache>,
        animated_cache: Arc<L1AnimatedPreviewCache>,
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

        let generator = Arc::new(ThumbnailGenerator::new());
        let ready_drain_queue = Arc::new(ReadyQueue::default());
        let active_pipeline_id = Arc::new(AtomicUsize::new(0));
        let view_loader = Arc::new(ViewLoader::new(
            ViewLoaderDeps {
                library: library.clone(),
                cache: cache.clone(),
                store: store.clone(),
                generator: generator.clone(),
                ready: ready_drain_queue.clone(),
            },
            view_loader_threads(threads),
        ));

        Self {
            library,
            cache,
            animated_cache,
            store,
            generator,
            pool,
            view_loader,
            tracker: Arc::new(ThumbnailProgressTracker::new()),
            ready_drain_queue,
            generation_queue: Arc::new(Mutex::new(VecDeque::new())),
            active_pipeline_id,
            is_cancelled: Arc::new(AtomicBool::new(false)),
            active_benchmark: Arc::new(RwLock::new(None)),
            active_in_flight: Arc::new(Mutex::new(HashSet::new())),
            current_config: Arc::new(RwLock::new(ThumbnailConfig::default())),
        }
    }

    pub fn set_config(&self, config: ThumbnailConfig) {
        *self.current_config.write() = config;
    }

    pub fn get_config(&self) -> ThumbnailConfig {
        self.current_config.read().clone()
    }

    pub fn current_pipeline_id(&self) -> usize {
        self.active_pipeline_id.load(Ordering::Acquire)
    }

    pub fn active_benchmark(&self) -> Option<Arc<ThumbnailPipelineBenchmark>> {
        self.active_benchmark.read().clone()
    }

    pub fn is_drain_queue_empty(&self) -> bool {
        self.ready_drain_queue.is_empty()
    }

    /// Registers the function that tells the UI that thumbnails are ready (called from the worker threads after every push).
    pub fn set_ready_notifier(&self, notifier: Arc<dyn Fn() + Send + Sync>) {
        self.ready_drain_queue.set_notifier(notifier);
    }

    pub fn drain_ready_batch(&self, max_items: usize) -> Vec<ReadyThumbnailItem> {
        let current_pipeline = self.active_pipeline_id.load(Ordering::Acquire);
        let items = self.ready_drain_queue.drain_batch(current_pipeline, max_items);
        if items.is_empty() {
            return items;
        }

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

    pub fn tracker_arc(&self) -> Arc<ThumbnailProgressTracker> {
        self.tracker.clone()
    }

    pub fn storage_stats(&self) -> ThumbnailStorageStats {
        self.store.storage_stats(&self.library)
    }

    pub fn cancel_current_pipeline(&self) {
        self.is_cancelled.store(true, Ordering::Release);
        self.active_pipeline_id.fetch_add(1, Ordering::SeqCst);
        self.generation_queue.lock().clear();
        self.ready_drain_queue.retain_view_items();
        self.active_in_flight.lock().clear();
        self.tracker.reset();
        *self.active_benchmark.write() = None;
    }

    /// The thumbnail if it is already in RAM (a reference-count bump, no I/O and no decode). The Browse view uses this on the UI
    /// thread; what is missing goes through [`Self::request_view_thumbnails`].
    pub fn cached_thumbnail(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        self.cache.get(media_id)
    }

    /// Asks the view loader for these thumbnails, most urgent first; earlier pending requests are dropped. Returns how many
    /// requests are pending.
    pub fn request_view_thumbnails(&self, ids: impl IntoIterator<Item = MediaId>) -> usize {
        self.view_loader.request(ids)
    }

    pub fn cancel_view_requests(&self) {
        self.view_loader.cancel_pending();
    }

    /// Blocking lookup in RAM, then in the pack store. For callers that are not on the UI thread.
    pub fn try_load_existing(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        if let Some(cached) = self.cache.get(media_id) {
            return Some(cached);
        }

        let location = {
            let rtxn = self.library.raw_db().env.read_txn().ok()?;
            self.library.raw_db().thumbnails.get(&rtxn, &media_id).ok().flatten()?
        };

        if let Ok(compressed_bytes) = self.store.read(location)
            && let Ok((w, h, rgb8_data)) = self.generator.decompress(&compressed_bytes)
        {
            return Some(self.cache.insert_rgb(media_id, w, h, &rgb8_data).pixels);
        }

        None
    }

    /// Fast non-blocking lookup for cached preview frames in LMDB / PackStore (0.05ms).
    pub fn try_load_preview_frames(
        &self,
        media_id: MediaId,
    ) -> Option<Vec<(u32, u32, Vec<u8>)>> {
        let rtxn = self.library.raw_db().env.read_txn().ok()?;
        let p_rec = self.library.raw_db().video_previews.get(&rtxn, &media_id).ok().flatten()?;

        let mut frames = Vec::with_capacity(p_rec.frame_locations.len());
        for loc in &p_rec.frame_locations {
            if let Ok(bytes) = self.store.read(*loc)
                && let Ok((w, h, rgb8)) = self.generator.decompress(&bytes)
            {
                frames.push((w, h, rgb8));
            }
        }

        if frames.is_empty() {
            None
        } else {
            Some(frames)
        }
    }

    /// Fast non-blocking lookup for cached animated preview in RAM (e.g. GIF).
    pub fn try_get_animated_preview(&self, media_id: MediaId) -> Option<Arc<AnimatedPreview>> {
        self.animated_cache.get(media_id)
    }

    /// Asynchronously extracts, scales, and caches animated GIF frames in a background worker thread.
    pub fn get_or_load_animated_preview_async<F>(
        &self,
        media_id: MediaId,
        path: PathBuf,
        config: ThumbnailConfig,
        on_loaded: F,
    ) where
        F: FnOnce(Result<Arc<AnimatedPreview>, ThumbnailError>) + Send + 'static,
    {
        if let Some(cached) = self.animated_cache.get(media_id) {
            on_loaded(Ok(cached));
            return;
        }

        let cache = self.animated_cache.clone();
        self.pool.spawn(move || {
            let res = GifThumbnailer::extract_frames(
                &path,
                config.max_bound,
                config.scaling_algorithm,
            )
            .map(Arc::new);

            if let Ok(ref preview) = res {
                cache.insert(media_id, preview.clone());
            }

            on_loaded(res);
        });
    }

    /// Launches dedicated full-folder thumbnail pipeline processing ALL items in the folder.
    pub fn start_folder_pipeline(
        self: &Arc<Self>,
        folder_id: u64,
        items: Vec<PipelineItem>,
        config: ThumbnailConfig,
    ) {
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
            scheduler.execute_orchestration(
                pipeline_id,
                folder_id,
                items,
                config,
                bench_clone,
            );
        });
    }

    /// The file path of a media item from the library (folder path + file name), with the folder paths cached per scan.
    fn item_path(
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

    fn execute_orchestration(
        &self,
        pipeline_id: usize,
        folder_id: u64,
        items: Vec<PipelineItem>,
        config: ThumbnailConfig,
        benchmark: Arc<ThumbnailPipelineBenchmark>,
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

                let has_primary = self.library.raw_db().thumbnails.get(&rtxn, &item.media_id).ok().flatten().is_some();

                let previews_ok = if item.is_video && config.generate_video_previews {
                    if let Ok(Some(prev)) = self.library.raw_db().video_previews.get(&rtxn, &item.media_id) {
                        prev.frame_count >= config.preview_frame_count && !prev.frame_locations.is_empty()
                    } else {
                        false
                    }
                } else {
                    true
                };

                if has_primary && previews_ok {
                    existing_count += 1;
                } else if let Some(file_path) = self.item_path(&rtxn, &mut folder_paths, item.media_id) {
                    missing_targets.push(PreparedTarget { pipeline_id, folder_id, media_id: item.media_id, file_path, is_video: item.is_video });
                }
            }
        }

        benchmark.set_preload_duration(t_scan.elapsed().as_micros() as u64);
        tracing::info!(
            folder_id = folder_id,
            pipeline_id = pipeline_id,
            existing_cached = existing_count,
            missing_to_generate = missing_targets.len(),
            scan_us = t_scan.elapsed().as_micros() as u64,
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
        let mut batched_db_writes: Vec<BatchedDbWriteItem> =
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

            let is_video = target.is_video;

            let mut is_corrupted = false;

            if is_video {
                let t_vid = Instant::now();
                match self
                    .generator
                    .generate_video_thumbnails(&target.file_path, &config)
                {
                    Ok(video_res) => {
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
                                    let meta_update = if video_res.orig_width > 0 && video_res.orig_height > 0 {
                                        Some(ProbedMetadata {
                                            width: video_res.orig_width,
                                            height: video_res.orig_height,
                                            duration_ms: video_res.duration_ms,
                                        })
                                    } else {
                                        None
                                    };

                                    batched_db_writes.push(BatchedDbWriteItem {
                                        media_id: target.media_id,
                                        location: Some(primary_loc),
                                        preview_record: preview_rec,
                                        probed_metadata: meta_update,
                                        is_corrupted: false,
                                    });

                                    let pixel_buffer = self
                                    .cache
                                    .insert_rgb(target.media_id, primary.width, primary.height, &primary.rgb_pixels)
                                    .pixels;

                                    let depth = self.ready_drain_queue.push(ReadyThumbnailItem {
                                        pipeline_id,
                                        folder_id,
                                        media_id: target.media_id,
                                        width: primary.width,
                                        height: primary.height,
                                        pixel_buffer: Some(pixel_buffer),
                                        orig_width: video_res.orig_width,
                                        orig_height: video_res.orig_height,
                                        duration_ms: video_res.duration_ms,
                                        is_corrupted: false,
                                    });
                                    benchmark.record_queue_depth(depth);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(media_id = target.media_id, error = %e, "[ThumbnailPipeline] Video thumbnail generation failed, marking file as corrupted");
                        is_corrupted = true;
                    }
                }
            } else {
                let t_img = Instant::now();
                match self.generator.generate_image_thumbnail(
                    &target.file_path,
                    config.max_bound,
                    config.scaling_algorithm,
                ) {
                    Ok(frame) => {
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
                                    let meta_update = if frame.orig_width > 0 && frame.orig_height > 0 {
                                        Some(ProbedMetadata {
                                            width: frame.orig_width,
                                            height: frame.orig_height,
                                            duration_ms: 0,
                                        })
                                    } else {
                                        None
                                    };

                                    batched_db_writes.push(BatchedDbWriteItem {
                                        media_id: target.media_id,
                                        location: Some(location),
                                        preview_record: None,
                                        probed_metadata: meta_update,
                                        is_corrupted: false,
                                    });

                                    let pixel_buffer = self
                                    .cache
                                    .insert_rgb(target.media_id, frame.width, frame.height, &frame.rgb_pixels)
                                    .pixels;

                                    let depth = self.ready_drain_queue.push(ReadyThumbnailItem {
                                        pipeline_id,
                                        folder_id,
                                        media_id: target.media_id,
                                        width: frame.width,
                                        height: frame.height,
                                        pixel_buffer: Some(pixel_buffer),
                                        orig_width: frame.orig_width,
                                        orig_height: frame.orig_height,
                                        duration_ms: 0,
                                        is_corrupted: false,
                                    });
                                    benchmark.record_queue_depth(depth);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(media_id = target.media_id, error = %e, "[ThumbnailPipeline] Image thumbnail generation failed, marking file as corrupted");
                        is_corrupted = true;
                    }
                }
            }

            if is_corrupted && !self.is_pipeline_stale(pipeline_id) {
                batched_db_writes.push(BatchedDbWriteItem {
                    media_id: target.media_id,
                    location: None,
                    preview_record: None,
                    probed_metadata: None,
                    is_corrupted: true,
                });

                let depth = self.ready_drain_queue.push(ReadyThumbnailItem {
                    pipeline_id,
                    folder_id,
                    media_id: target.media_id,
                    width: 0,
                    height: 0,
                    pixel_buffer: None,
                    orig_width: 0,
                    orig_height: 0,
                    duration_ms: 0,
                    is_corrupted: true,
                });
                benchmark.record_queue_depth(depth);
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
        batch: &mut Vec<BatchedDbWriteItem>,
        benchmark: &ThumbnailPipelineBenchmark,
    ) {
        if batch.is_empty() {
            return;
        }

        let t_db = Instant::now();
        if let Ok(mut wtxn) = self.library.raw_db().env.write_txn() {
            let count = batch.len();
            for item in batch.drain(..) {
                if let Some(loc) = item.location {
                    let _ = self.library.raw_db().thumbnails.put(&mut wtxn, &item.media_id, &loc);
                }
                if let Some(p_rec) = item.preview_record {
                    let _ = self
                        .library
                        .raw_db()
                        .video_previews
                        .put(&mut wtxn, &item.media_id, &p_rec);
                }

                if let Ok(Some(mut rec)) = self.library.raw_db().media.get(&wtxn, &item.media_id) {
                    let mut changed = false;
                    if rec.is_corrupted != item.is_corrupted {
                        rec.is_corrupted = item.is_corrupted;
                        changed = true;
                    }
                    if let Some(meta) = item.probed_metadata {
                        if rec.width == 0 && meta.width > 0 {
                            rec.width = meta.width;
                            changed = true;
                        }
                        if rec.height == 0 && meta.height > 0 {
                            rec.height = meta.height;
                            changed = true;
                        }
                        if rec.duration_ms == 0 && meta.duration_ms > 0 {
                            rec.duration_ms = meta.duration_ms;
                            changed = true;
                        }
                    }
                    if changed {
                        let _ = self.library.raw_db().media.put(&mut wtxn, &item.media_id, &rec);
                    }
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
            animated_cache: self.animated_cache.clone(),
            store: self.store.clone(),
            generator: self.generator.clone(),
            pool: self.pool.clone(),
            view_loader: self.view_loader.clone(),
            tracker: self.tracker.clone(),
            ready_drain_queue: self.ready_drain_queue.clone(),
            generation_queue: self.generation_queue.clone(),
            active_pipeline_id: self.active_pipeline_id.clone(),
            is_cancelled: self.is_cancelled.clone(),
            active_benchmark: self.active_benchmark.clone(),
            active_in_flight: self.active_in_flight.clone(),
            current_config: self.current_config.clone(),
        }
    }

    /// Image or video as the library knows it (a file's content decides, not its extension); the extension only
    /// answers for files that are not in the library.
    pub fn is_video_media(&self, media_id: MediaId, file_path: &Path) -> bool {
        if let Ok(rtxn) = self.library.raw_db().env.read_txn()
            && let Ok(Some(record)) = self.library.raw_db().media.get(&rtxn, &media_id)
        {
            return record.media_type == MediaType::Video;
        }
        MediaFormat::from_path(file_path).media_type() == Some(MediaType::Video)
    }

    pub fn load_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: ThumbnailConfig,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        if let Some(item) = self.cache.get_thumbnail(media_id) {
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
            let item = self.cache.insert_rgb(media_id, w, h, &rgb8);
            return Ok(item);
        }

        let is_video = self.is_video_media(media_id, file_path);
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
            .insert_rgb(media_id, frame.width, frame.height, &frame.rgb_pixels);
        Ok(item)
    }

    /// Explicitly regenerates thumbnail & previews from physical file, overwriting PackStore and LMDB index.
    pub fn regenerate_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        let is_video = self.is_video_media(media_id, file_path);
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
            .insert_rgb(media_id, frame.width, frame.height, &frame.rgb_pixels);

        Ok(item)
    }

    /// Explicitly regenerates thumbnail & previews with an overridden video offset percentage, overwriting PackStore and LMDB index.
    pub fn regenerate_with_offset_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
        offset_percent: u8,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        let custom_cfg = config.with_video_offset_percent(offset_percent);
        self.regenerate_sync(media_id, file_path, &custom_cfg)
    }
}
