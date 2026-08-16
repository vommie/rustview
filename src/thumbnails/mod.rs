pub mod arena;
pub mod cache;
pub mod error;
pub mod gc;
pub mod generator;
pub mod image;
pub mod loader;
pub mod progress;
pub mod store;
pub mod types;
pub mod video;

pub use arena::ThumbnailArena;
pub use cache::{CachedThumbnail, L1ThumbnailCache};
pub use error::ThumbnailError;
pub use gc::ThumbnailGarbageCollector;
pub use generator::ThumbnailGenerator;
pub use loader::{PreparedTarget, ReadyThumbnailItem, ThumbnailScheduler};
pub use progress::{PipelinePhase, ThumbnailProgressSnapshot, ThumbnailProgressTracker};
pub use store::PackStore;
pub use types::{
    DEFAULT_THUMBNAIL_BOUND, FitMode, ScalingAlgorithm, THUMBNAIL_BYTE_SIZE, THUMBNAIL_CHANNELS,
    THUMBNAIL_PIXELS, THUMBNAIL_SIZE, ThumbnailConfig, ThumbnailFormat, ThumbnailHandle,
    ThumbnailLocation, ThumbnailPriority, VideoPreviewRecord, VideoThumbnailEngine,
    anonymize_target, calculate_aspect_dimensions,
};

use crate::library::Library;
use crate::library::models::MediaId;
use std::path::Path;
use std::sync::Arc;

/// Unified high-performance Thumbnail Subsystem facade.
pub struct ThumbnailManager {
    cache: Arc<L1ThumbnailCache>,
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
        let scheduler = Arc::new(ThumbnailScheduler::new(
            library,
            cache.clone(),
            store.clone(),
            worker_threads,
        ));

        Ok(Self {
            cache,
            store,
            scheduler,
        })
    }

    pub fn start_pipeline(
        &self,
        folder_id: u64,
        sorted_targets: Vec<PreparedTarget>,
        config: ThumbnailConfig,
    ) {
        self.scheduler
            .start_folder_pipeline(folder_id, sorted_targets, config);
    }

    pub fn cancel_pipeline(&self) {
        self.scheduler.cancel_current_pipeline();
    }

    pub fn current_pipeline_id(&self) -> usize {
        self.scheduler.current_pipeline_id()
    }

    pub fn drain_ready_batch(&self, max_items: usize) -> Vec<ReadyThumbnailItem> {
        self.scheduler.drain_ready_batch(max_items)
    }

    pub fn progress(&self) -> ThumbnailProgressSnapshot {
        self.scheduler.snapshot_progress()
    }

    pub fn get_or_load_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: ThumbnailConfig,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        self.scheduler.load_sync(media_id, file_path, config)
    }

    pub fn load_preview_frames_sync(
        &self,
        media_id: MediaId,
    ) -> Result<Vec<(u32, u32, Vec<u8>)>, ThumbnailError> {
        self.scheduler.load_preview_frames_sync(media_id)
    }

    pub fn cache(&self) -> &L1ThumbnailCache {
        &self.cache
    }

    pub fn store(&self) -> &PackStore {
        &self.store
    }

    pub fn scheduler(&self) -> &ThumbnailScheduler {
        &self.scheduler
    }
}
