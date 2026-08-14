use super::cache::L1ThumbnailCache;
use super::error::ThumbnailError;
use super::generator::ThumbnailGenerator;
use super::store::PackStore;
use super::types::{FitMode, ThumbnailHandle, ThumbnailPriority};
use crate::library::Library;
use crate::library::models::MediaId;
use parking_lot::Mutex;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Eq, PartialEq)]
struct ThumbnailJob {
    priority: ThumbnailPriority,
    media_id: MediaId,
    file_path: PathBuf,
}

impl Ord for ThumbnailJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.priority.cmp(&self.priority)
    }
}

impl PartialOrd for ThumbnailJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct ThumbnailScheduler {
    library: Arc<Library>,
    cache: Arc<L1ThumbnailCache>,
    store: Arc<PackStore>,
    generator: Arc<ThumbnailGenerator>,
    pool: ThreadPool,
    queue: Arc<Mutex<BinaryHeap<ThumbnailJob>>>,
}

impl ThumbnailScheduler {
    pub fn new(
        library: Arc<Library>,
        cache: Arc<L1ThumbnailCache>,
        store: Arc<PackStore>,
        threads: usize,
    ) -> Self {
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("thumb-worker-{}", i))
            .build()
            .expect("Failed to build thumbnail worker pool");

        Self {
            library,
            cache,
            store,
            generator: Arc::new(ThumbnailGenerator::new()),
            pool,
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
        }
    }

    pub fn request_thumbnail(
        &self,
        media_id: MediaId,
        file_path: PathBuf,
        priority: ThumbnailPriority,
    ) {
        {
            let mut queue = self.queue.lock();
            queue.push(ThumbnailJob {
                priority,
                media_id,
                file_path,
            });
        }
        self.trigger_workers();
    }

    pub fn load_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
    ) -> Result<ThumbnailHandle, ThumbnailError> {
        // 1. L1 RAM Cache Hit
        if let Some(handle) = self.cache.get(media_id) {
            return Ok(handle);
        }

        // 2. L2 SSD Pack Store Hit
        let location = {
            let rtxn = self.library.raw_db().env.read_txn()?;
            self.library.raw_db().thumbnails.get(&rtxn, &media_id)?
        };

        if let Some(loc) = location {
            let compressed = self.store.read(loc)?;
            let rgb8 = self.generator.decompress_jpeg(&compressed)?;
            if let Some(handle) = self.cache.insert(media_id, &rgb8) {
                return Ok(handle);
            }
        }

        // 3. Cache Miss: Generate and Persist
        let rgb8 = self.generator.generate_rgb8(file_path, FitMode::Contain)?;
        let compressed = self.generator.compress_jpeg(&rgb8)?;
        let location = self.store.append(&compressed)?;

        // Save location to LMDB
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

    fn trigger_workers(&self) {
        let queue = self.queue.clone();
        let generator = self.generator.clone();
        let store = self.store.clone();
        let cache = self.cache.clone();
        let library = self.library.clone();

        self.pool.spawn(move || {
            while let Some(job) = {
                let mut q = queue.lock();
                q.pop()
            } {
                if cache.get(job.media_id).is_some() {
                    continue;
                }

                if let Ok(rgb8) = generator.generate_rgb8(&job.file_path, FitMode::Contain) {
                    if let Ok(compressed) = generator.compress_jpeg(&rgb8)
                        && let Ok(loc) = store.append(&compressed)
                        && let Ok(mut wtxn) = library.raw_db().env.write_txn()
                    {
                        let _ = library
                            .raw_db()
                            .thumbnails
                            .put(&mut wtxn, &job.media_id, &loc);
                        let _ = wtxn.commit();
                    }
                    cache.insert(job.media_id, &rgb8);
                }
            }
        });
    }
}
