pub mod arena;
pub mod cache;
pub mod error;
pub mod gc;
pub mod generator;
pub mod gif;
pub mod image;
pub mod loader;
pub mod progress;
pub mod regeneration;
pub mod store;
pub mod types;
pub mod video;

pub use arena::ThumbnailArena;
pub use cache::{CachedThumbnail, L1AnimatedPreviewCache, L1ThumbnailCache};
pub use error::ThumbnailError;
pub use gc::ThumbnailGarbageCollector;
pub use generator::ThumbnailGenerator;
pub use gif::{AnimatedFrame, AnimatedPreview, GifThumbnailer};
pub use loader::{PreparedTarget, ReadyThumbnailItem, ThumbnailScheduler};
pub use progress::{PipelinePhase, ThumbnailProgressSnapshot, ThumbnailProgressTracker};
pub use store::PackStore;
pub use types::{
    anonymize_target, calculate_aspect_dimensions, generate_random_video_offset_percent,
    FitMode, ScalingAlgorithm, ThumbnailConfig, ThumbnailFormat, ThumbnailHandle,
    ThumbnailLocation, ThumbnailPriority, ThumbnailStorageStats, VideoPreviewRecord,
    VideoThumbnailEngine, DEFAULT_THUMBNAIL_BOUND, THUMBNAIL_BYTE_SIZE, THUMBNAIL_CHANNELS,
    THUMBNAIL_PIXELS, THUMBNAIL_SIZE,
};

use crate::library::models::MediaId;
use crate::library::Library;
use crate::telemetry::benchmark::ThumbnailPipelineBenchmark;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Unified high-performance Thumbnail Subsystem facade.
pub struct ThumbnailManager {
    cache: Arc<L1ThumbnailCache>,
    animated_cache: Arc<L1AnimatedPreviewCache>,
    store: Arc<PackStore>,
    scheduler: Arc<ThumbnailScheduler>,
}

impl ThumbnailManager {
    pub fn new(
        library: Arc<Library>,
        cache_dir: &Path,
        l1_slots: usize,
        worker_threads: usize,
    ) -> Result<Self, ThumbnailError> {
        let store = Arc::new(PackStore::open(cache_dir)?);
        let cache = Arc::new(L1ThumbnailCache::new(l1_slots));
        let animated_cache = Arc::new(L1AnimatedPreviewCache::new(64));
        let scheduler = Arc::new(ThumbnailScheduler::new(
            library,
            cache.clone(),
            animated_cache.clone(),
            store.clone(),
            worker_threads,
        ));

        Ok(Self {
            cache,
            animated_cache,
            store,
            scheduler,
        })
    }

    pub fn set_config(&self, config: ThumbnailConfig) {
        self.scheduler.set_config(config);
    }

    pub fn get_config(&self) -> ThumbnailConfig {
        self.scheduler.get_config()
    }

    pub fn start_pipeline(
        &self,
        folder_id: u64,
        all_targets: Vec<PreparedTarget>,
        config: ThumbnailConfig,
    ) {
        self.scheduler
            .start_folder_pipeline(folder_id, all_targets, config);
    }

    /// Fast synchronous lookup in L1 RAM cache or L2 PackStore.
    #[inline]
    pub fn try_load_existing(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        self.scheduler.try_load_existing(media_id)
    }

    /// Fast non-blocking lookup for cached preview animation frames (0.05ms).
    /// NEVER invokes heavy video decoding on UI threads.
    #[inline]
    pub fn try_load_preview_frames(
        &self,
        media_id: MediaId,
    ) -> Option<Vec<(u32, u32, Vec<u8>)>> {
        self.scheduler.try_load_preview_frames(media_id)
    }

    /// Fast non-blocking lookup for cached animated GIF frames in L1 RAM cache.
    #[inline]
    pub fn try_get_animated_preview(&self, media_id: MediaId) -> Option<Arc<AnimatedPreview>> {
        self.scheduler.try_get_animated_preview(media_id)
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
        self.scheduler
            .get_or_load_animated_preview_async(media_id, path, config, on_loaded);
    }

    pub fn cancel_pipeline(&self) {
        self.scheduler.cancel_current_pipeline();
    }

    pub fn current_pipeline_id(&self) -> usize {
        self.scheduler.current_pipeline_id()
    }

    pub fn is_drain_queue_empty(&self) -> bool {
        self.scheduler.is_drain_queue_empty()
    }

    pub fn active_benchmark(&self) -> Option<Arc<ThumbnailPipelineBenchmark>> {
        self.scheduler.active_benchmark()
    }

    pub fn drain_ready_batch(&self, max_items: usize) -> Vec<ReadyThumbnailItem> {
        self.scheduler.drain_ready_batch(max_items)
    }

    pub fn progress(&self) -> ThumbnailProgressSnapshot {
        self.scheduler.snapshot_progress()
    }

    pub fn tracker(&self) -> Arc<ThumbnailProgressTracker> {
        self.scheduler.tracker_arc()
    }

    pub fn storage_stats(&self) -> ThumbnailStorageStats {
        self.store.storage_stats(&self.scheduler.library)
    }

    pub fn get_or_load_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: ThumbnailConfig,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        self.scheduler.load_sync(media_id, file_path, config)
    }

    pub fn regenerate_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        self.scheduler.regenerate_sync(media_id, file_path, config)
    }

    /// Explicitly regenerates thumbnail & previews with an overridden video offset percentage, overwriting PackStore and LMDB index.
    pub fn regenerate_with_offset_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
        offset_percent: u8,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        self.scheduler
            .regenerate_with_offset_sync(media_id, file_path, config, offset_percent)
    }

    pub fn cache(&self) -> &L1ThumbnailCache {
        &self.cache
    }

    pub fn animated_cache(&self) -> &L1AnimatedPreviewCache {
        &self.animated_cache
    }

    pub fn store(&self) -> &PackStore {
        &self.store
    }

    pub fn scheduler(&self) -> &ThumbnailScheduler {
        &self.scheduler
    }
}
