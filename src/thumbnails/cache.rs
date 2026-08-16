use crate::library::models::MediaId;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct CachedThumbnail {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
}

/// High-speed non-fragmenting L1 in-memory thumbnail cache with LRU eviction.
pub struct L1ThumbnailCache {
    capacity: usize,
    inner: RwLock<CacheState>,
}

struct CacheState {
    map: HashMap<MediaId, CachedThumbnail>,
    lru_order: VecDeque<MediaId>,
}

impl L1ThumbnailCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: RwLock::new(CacheState {
                map: HashMap::with_capacity(capacity),
                lru_order: VecDeque::with_capacity(capacity),
            }),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.inner.read().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, media_id: MediaId) -> Option<CachedThumbnail> {
        let mut state = self.inner.write();
        if let Some(item) = state.map.get(&media_id).cloned() {
            state.lru_order.retain(|&id| id != media_id);
            state.lru_order.push_back(media_id);
            Some(item)
        } else {
            None
        }
    }

    pub fn insert(
        &self,
        media_id: MediaId,
        width: u32,
        height: u32,
        rgb_pixels: &[u8],
    ) -> CachedThumbnail {
        let item = CachedThumbnail {
            width,
            height,
            pixels: Arc::new(rgb_pixels.to_vec()),
        };

        let mut state = self.inner.write();
        if state.map.len() >= self.capacity
            && !state.map.contains_key(&media_id)
            && let Some(evicted_id) = state.lru_order.pop_front()
        {
            state.map.remove(&evicted_id);
        }

        state.map.insert(media_id, item.clone());
        state.lru_order.retain(|&id| id != media_id);
        state.lru_order.push_back(media_id);

        item
    }

    pub fn remove(&self, media_id: MediaId) {
        let mut state = self.inner.write();
        state.map.remove(&media_id);
        state.lru_order.retain(|&id| id != media_id);
    }

    pub fn clear(&self) {
        let mut state = self.inner.write();
        state.map.clear();
        state.lru_order.clear();
    }
}
