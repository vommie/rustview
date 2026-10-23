use super::cache::L1AnimatedPreviewCache;
use super::generator::ThumbnailGenerator;
use super::progress::{ThumbnailProgressSnapshot, ThumbnailProgressTracker};
use super::ram_cache::ThumbnailRamCache;
use super::ready_queue::ReadyQueue;
use super::store::PackStore;
use super::types::{ThumbnailConfig, ThumbnailLocation, ThumbnailStorageStats, VideoPreviewRecord};
use super::view_loader::{ViewLoader, ViewLoaderDeps};
use crate::library::Library;
use crate::library::models::MediaId;
use crate::telemetry::benchmark::ThumbnailPipelineBenchmark;
use parking_lot::{Mutex, RwLock};
use rayon::{ThreadPool, ThreadPoolBuilder};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

mod on_demand;
mod pipeline;
mod worker;

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
    /// The picture can be animated (GIF, PNG, WebP, AVIF): its file is checked for hover preview frames.
    pub animatable: bool,
    /// A refresh the user started: pictures that have a thumbnail but were never checked for animation are checked now.
    /// Without it only pictures that get their thumbnail anyway are checked, and everything else keeps its old behavior.
    pub upgrade_animated: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedTarget {
    pub pipeline_id: usize,
    pub folder_id: u64,
    pub media_id: MediaId,
    pub file_path: PathBuf,
    /// From the library record: a file's content decides image or video, not its extension.
    pub is_video: bool,
    pub animatable: bool,
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
    /// Hover preview frames of an animated picture were just stored for the item.
    pub is_animated: bool,
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
                .thread_name(|i| format!("thumb-worker-{i}"))
                .build()
                .expect("Failed to build high-performance thumbnail worker pool"),
        );

        let generator = Arc::new(ThumbnailGenerator::new());
        let ready_drain_queue = Arc::new(ReadyQueue::default());
        let active_pipeline_id = Arc::new(AtomicUsize::new(0));
        let view_loader = Arc::new(ViewLoader::new(
            &ViewLoaderDeps {
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
}
