use super::gif::AnimatedPreview;
use crate::library::models::MediaId;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// High-speed bounded L1 in-memory animated preview cache with LRU eviction for animated media (e.g. GIFs).
pub struct L1AnimatedPreviewCache {
    capacity: usize,
    inner: RwLock<AnimatedCacheState>,
}

struct AnimatedCacheState {
    map: HashMap<MediaId, Arc<AnimatedPreview>>,
    lru_order: VecDeque<MediaId>,
}

impl L1AnimatedPreviewCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: RwLock::new(AnimatedCacheState {
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

    pub fn get(&self, media_id: MediaId) -> Option<Arc<AnimatedPreview>> {
        let mut state = self.inner.write();
        if let Some(item) = state.map.get(&media_id).cloned() {
            state.lru_order.retain(|&id| id != media_id);
            state.lru_order.push_back(media_id);
            Some(item)
        } else {
            None
        }
    }

    pub fn insert(&self, media_id: MediaId, preview: Arc<AnimatedPreview>) {
        let mut state = self.inner.write();
        if state.map.len() >= self.capacity
            && !state.map.contains_key(&media_id)
            && let Some(evicted_id) = state.lru_order.pop_front()
        {
            state.map.remove(&evicted_id);
        }

        state.map.insert(media_id, preview);
        state.lru_order.retain(|&id| id != media_id);
        state.lru_order.push_back(media_id);
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
