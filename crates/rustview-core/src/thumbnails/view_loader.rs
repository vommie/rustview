//! On-demand loading of the thumbnails that the Browse view shows.
//!
//! The view asks for the thumbnails of the tiles it is about to draw; worker threads read, decode and cache them and report each
//! one through the ready queue the UI drains. A new request replaces the pending ones (the user scrolled on), so a fast scroll
//! never builds a backlog; requests are served nearest-to-the-viewport first. Nothing here runs on the UI thread, and nothing
//! touches the drive: the compressed thumbnails live in the pack store.

use super::generator::ThumbnailGenerator;
use super::loader::{ReadyThumbnailItem, VIEW_LOAD_PIPELINE_ID};
use super::ram_cache::ThumbnailRamCache;
use super::ready_queue::ReadyQueue;
use super::store::PackStore;
use crate::library::Library;
use crate::library::models::MediaId;
use parking_lot::{Condvar, Mutex};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

/// Pending requests in priority order plus the ones a worker is busy with.
#[derive(Default)]
struct QueueState {
    pending: VecDeque<MediaId>,
    in_flight: HashSet<MediaId>,
    shutdown: bool,
}

/// The request queue (pure: no threads, no I/O).
#[derive(Default)]
pub struct ViewLoadQueue {
    state: Mutex<QueueState>,
    wake: Condvar,
}

impl ViewLoadQueue {
    /// Replaces the pending requests with `ids` (most urgent first). Ids that a worker already handles are skipped. Returns how
    /// many requests are pending now.
    pub fn replace_pending(&self, ids: impl IntoIterator<Item = MediaId>) -> usize {
        let mut state = self.state.lock();
        let mut seen = HashSet::new();
        let fresh: VecDeque<MediaId> = ids
            .into_iter()
            .filter(|id| !state.in_flight.contains(id) && seen.insert(*id))
            .collect();
        state.pending = fresh;
        let pending = state.pending.len();
        drop(state);
        if pending > 0 {
            self.wake.notify_all();
        }
        pending
    }

    /// Drops every pending request (the folder changed).
    pub fn clear(&self) {
        self.state.lock().pending.clear();
    }

    pub fn pending_len(&self) -> usize {
        self.state.lock().pending.len()
    }

    /// Blocks until a request is available and claims it; `None` once the queue is shut down.
    pub fn next(&self) -> Option<MediaId> {
        let mut state = self.state.lock();
        loop {
            if state.shutdown {
                return None;
            }
            if let Some(id) = state.pending.pop_front() {
                state.in_flight.insert(id);
                return Some(id);
            }
            self.wake.wait(&mut state);
        }
    }

    pub fn finish(&self, id: MediaId) {
        self.state.lock().in_flight.remove(&id);
    }

    pub fn shutdown(&self) {
        self.state.lock().shutdown = true;
        self.wake.notify_all();
    }
}

/// What the workers need; all of it is shared with the scheduler.
#[derive(Clone)]
pub struct ViewLoaderDeps {
    pub library: Arc<Library>,
    pub cache: Arc<ThumbnailRamCache>,
    pub store: Arc<PackStore>,
    pub generator: Arc<ThumbnailGenerator>,
    pub ready: Arc<ReadyQueue>,
}

pub struct ViewLoader {
    queue: Arc<ViewLoadQueue>,
}

impl ViewLoader {
    pub fn new(deps: &ViewLoaderDeps, threads: usize) -> Self {
        let queue = Arc::new(ViewLoadQueue::default());
        for index in 0..threads.max(1) {
            let queue = queue.clone();
            let deps = deps.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("thumb-view-{index}"))
                .spawn(move || {
                    while let Some(id) = queue.next() {
                        load_one(&deps, id);
                        queue.finish(id);
                    }
                });
            if let Err(e) = spawned {
                tracing::error!(error = %e, "[ThumbnailViewLoader] Failed to start a loader thread");
            }
        }
        Self { queue }
    }

    /// Asks for these thumbnails, most urgent first; the earlier pending requests are dropped.
    pub fn request(&self, ids: impl IntoIterator<Item = MediaId>) -> usize {
        self.queue.replace_pending(ids)
    }

    pub fn cancel_pending(&self) {
        self.queue.clear();
    }

    pub fn pending(&self) -> usize {
        self.queue.pending_len()
    }
}

impl Drop for ViewLoader {
    fn drop(&mut self) {
        self.queue.shutdown();
    }
}

/// Reads and decodes one thumbnail and hands it to the UI. A missing record is left to the generation pipeline; a record that
/// does not decode is removed so that the next folder scan regenerates it.
fn load_one(deps: &ViewLoaderDeps, id: MediaId) {
    let pixels = match deps.cache.get(id) {
        Some(cached) => cached,
        None => match decode_stored(deps, id) {
            Some(decoded) => {
                deps.cache.insert(id, decoded.clone());
                decoded
            }
            None => return,
        },
    };
    let (width, height) = (pixels.width(), pixels.height());
    deps.ready.push(ReadyThumbnailItem {
        pipeline_id: VIEW_LOAD_PIPELINE_ID,
        folder_id: 0,
        media_id: id,
        width,
        height,
        pixel_buffer: Some(pixels),
        orig_width: 0,
        orig_height: 0,
        duration_ms: 0,
        is_corrupted: false,
        is_animated: false,
    });
}

fn decode_stored(deps: &ViewLoaderDeps, id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
    let location = {
        let rtxn = deps.library.raw_db().env.read_txn().ok()?;
        deps.library.raw_db().thumbnails.get(&rtxn, &id).ok().flatten()?
    };
    let compressed = deps.store.read(location).ok()?;
    match deps.generator.decompress(&compressed) {
        Ok((width, height, rgb)) => {
            let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
            let target = pixels.make_mut_bytes();
            let len = target.len().min(rgb.len());
            target[..len].copy_from_slice(&rgb[..len]);
            Some(pixels)
        }
        Err(e) => {
            tracing::warn!(media_id = id, error = %e, "[ThumbnailViewLoader] A stored thumbnail does not decode; dropping its record so that it is regenerated");
            if let Ok(mut wtxn) = deps.library.raw_db().env.write_txn() {
                let _ = deps.library.raw_db().thumbnails.delete(&mut wtxn, &id);
                let _ = wtxn.commit();
            }
            None
        }
    }
}
