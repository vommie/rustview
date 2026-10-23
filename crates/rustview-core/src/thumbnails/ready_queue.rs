//! Thumbnails that are ready for the UI.
//!
//! Workers push finished thumbnails; the UI registers one notifier and drains when it is told to, instead of polling a timer
//! that wakes the event loop 60 times a second while nothing happens.

use super::loader::{ReadyThumbnailItem, VIEW_LOAD_PIPELINE_ID};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

type Notifier = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
pub struct ReadyQueue {
    items: Mutex<Vec<ReadyThumbnailItem>>,
    notifier: OnceLock<Notifier>,
}

impl ReadyQueue {
    /// Registers the function that is called after every push (once; later registrations are ignored). It must be cheap and must
    /// not block: it runs on the worker threads.
    pub fn set_notifier(&self, notifier: Notifier) {
        let _ = self.notifier.set(notifier);
    }

    /// Wakes the UI without adding an item (a pipeline started: the busy state and the progress display must update now).
    pub fn wake(&self) {
        if let Some(notifier) = self.notifier.get() {
            notifier();
        }
    }

    /// Adds an item, wakes the UI and returns the queue depth.
    pub fn push(&self, item: ReadyThumbnailItem) -> usize {
        let depth = {
            let mut items = self.items.lock();
            items.push(item);
            items.len()
        };
        self.wake();
        depth
    }

    pub fn is_empty(&self) -> bool {
        self.items.lock().is_empty()
    }

    /// Takes up to `max_items` items. Items of a pipeline other than `current_pipeline` are stale and dropped; view-loaded items
    /// belong to no pipeline and stay.
    pub fn drain_batch(&self, current_pipeline: usize, max_items: usize) -> Vec<ReadyThumbnailItem> {
        let mut items = self.items.lock();
        if items.is_empty() {
            return Vec::new();
        }
        items.retain(|item| item.pipeline_id == current_pipeline || item.pipeline_id == VIEW_LOAD_PIPELINE_ID);
        let take = items.len().min(max_items);
        items.drain(0..take).collect()
    }

    /// Drops everything that belongs to a folder pipeline (a pipeline was cancelled); view-loaded items stay.
    pub fn retain_view_items(&self) {
        self.items
            .lock()
            .retain(|item| item.pipeline_id == VIEW_LOAD_PIPELINE_ID);
    }
}
