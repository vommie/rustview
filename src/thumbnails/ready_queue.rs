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
        self.items.lock().retain(|item| item.pipeline_id == VIEW_LOAD_PIPELINE_ID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn item(pipeline_id: usize, media_id: u64) -> ReadyThumbnailItem {
        ReadyThumbnailItem {
            pipeline_id,
            folder_id: 1,
            media_id,
            width: 0,
            height: 0,
            pixel_buffer: None,
            orig_width: 0,
            orig_height: 0,
            duration_ms: 0,
            is_corrupted: false,
        }
    }

    #[test]
    fn every_push_notifies_and_reports_the_depth() {
        let queue = ReadyQueue::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        queue.set_notifier(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(queue.push(item(1, 1)), 1);
        assert_eq!(queue.push(item(1, 2)), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn stale_pipeline_items_are_dropped_and_view_items_stay() {
        let queue = ReadyQueue::default();
        queue.push(item(1, 10));
        queue.push(item(2, 20));
        queue.push(item(VIEW_LOAD_PIPELINE_ID, 30));
        let ids: Vec<u64> = queue.drain_batch(2, 10).iter().map(|i| i.media_id).collect();
        assert_eq!(ids, vec![20, 30]);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_batch_is_capped_and_the_rest_waits() {
        let queue = ReadyQueue::default();
        for id in 0..5 {
            queue.push(item(1, id));
        }
        assert_eq!(queue.drain_batch(1, 2).len(), 2);
        assert!(!queue.is_empty());
        assert_eq!(queue.drain_batch(1, 10).len(), 3);
    }

    #[test]
    fn cancelling_keeps_only_view_items() {
        let queue = ReadyQueue::default();
        queue.push(item(1, 1));
        queue.push(item(VIEW_LOAD_PIPELINE_ID, 2));
        queue.retain_view_items();
        let ids: Vec<u64> = queue.drain_batch(9, 10).iter().map(|i| i.media_id).collect();
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn pushing_without_a_notifier_is_fine() {
        assert_eq!(ReadyQueue::default().push(item(1, 1)), 1);
    }
}
