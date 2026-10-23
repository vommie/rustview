pub mod animated_frames;
pub mod cache;
pub mod error;
pub mod gc;
pub mod generator;
pub mod gif;
pub mod image;
pub mod loader;
pub mod progress;
pub mod ram_cache;
pub mod ready_queue;
pub mod regeneration;
pub mod store;
pub mod types;
pub mod video;
pub mod view_loader;

pub use cache::L1AnimatedPreviewCache;
pub use error::ThumbnailError;
pub use gc::ThumbnailGarbageCollector;
pub use generator::ThumbnailGenerator;
pub use gif::{AnimatedFrame, AnimatedPreview, GifThumbnailer};
pub use loader::{PipelineItem, PreparedTarget, ReadyThumbnailItem, ThumbnailScheduler};
pub use progress::{PipelinePhase, ThumbnailProgressSnapshot, ThumbnailProgressTracker};
pub use ram_cache::{CachedThumbnail, RamCacheStats, ThumbnailRamCache};
pub use store::PackStore;
pub use types::{
    anonymize_target, calculate_aspect_dimensions, generate_random_video_offset_percent,
    FitMode, ScalingAlgorithm, ThumbnailConfig, ThumbnailFormat,
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
    cache: Arc<ThumbnailRamCache>,
    animated_cache: Arc<L1AnimatedPreviewCache>,
    store: Arc<PackStore>,
    scheduler: Arc<ThumbnailScheduler>,
}

impl ThumbnailManager {
    pub fn new(
        library: Arc<Library>,
        cache_dir: &Path,
        cache_budget_bytes: usize,
        worker_threads: usize,
    ) -> Result<Self, ThumbnailError> {
        let store = Arc::new(PackStore::open(cache_dir)?);
        let cache = Arc::new(ThumbnailRamCache::new(cache_budget_bytes));
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
        items: Vec<PipelineItem>,
        config: ThumbnailConfig,
    ) {
        self.scheduler.start_folder_pipeline(folder_id, items, config);
    }

    /// Blocking lookup in the RAM cache, then in the pack store. Never call this on the UI thread: use
    /// [`Self::cached_thumbnail`] and [`Self::request_view_thumbnails`] there.
    #[inline]
    pub fn try_load_existing(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        self.scheduler.try_load_existing(media_id)
    }

    /// The thumbnail if it is in RAM (no I/O, no decode); safe on the UI thread.
    #[inline]
    pub fn cached_thumbnail(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        self.scheduler.cached_thumbnail(media_id)
    }

    /// Loads these thumbnails in the background, most urgent first; they arrive through [`Self::drain_ready_batch`].
    pub fn request_view_thumbnails(&self, ids: impl IntoIterator<Item = MediaId>) -> usize {
        self.scheduler.request_view_thumbnails(ids)
    }

    pub fn cancel_view_requests(&self) {
        self.scheduler.cancel_view_requests();
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

    /// Stops the running folder pipeline and drops the thumbnail loads the previous list had requested.
    pub fn cancel_pipeline(&self) {
        self.scheduler.cancel_current_pipeline();
        self.scheduler.cancel_view_requests();
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

    /// Registers the function that is called (on a worker thread) whenever thumbnails become ready for the UI.
    pub fn set_ready_notifier(&self, notifier: Arc<dyn Fn() + Send + Sync>) {
        self.scheduler.set_ready_notifier(notifier);
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

    /// Image or video as the library knows the medium (content decides, not the extension).
    pub fn is_video_media(&self, media_id: MediaId, file_path: &Path) -> bool {
        self.scheduler.is_video_media(media_id, file_path)
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

    pub fn cache(&self) -> &ThumbnailRamCache {
        &self.cache
    }

    /// A shared handle for code that has no `ThumbnailManager` at hand (the UI state keeps one).
    pub fn cache_handle(&self) -> Arc<ThumbnailRamCache> {
        self.cache.clone()
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
