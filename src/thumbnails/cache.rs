use super::arena::ThumbnailArena;
use super::types::ThumbnailHandle;
use crate::library::models::MediaId;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub struct L1ThumbnailCache {
    arena: Arc<ThumbnailArena>,
    inner: RwLock<CacheState>,
}

struct CacheState {
    map: HashMap<MediaId, (usize, ThumbnailHandle)>,
    lru_order: VecDeque<MediaId>,
    free_slots: Vec<usize>,
}

impl L1ThumbnailCache {
    pub fn new(capacity: usize) -> Self {
        let arena = Arc::new(ThumbnailArena::new(capacity));
        let free_slots = (0..capacity).rev().collect();

        Self {
            arena,
            inner: RwLock::new(CacheState {
                map: HashMap::with_capacity(capacity),
                lru_order: VecDeque::with_capacity(capacity),
                free_slots,
            }),
        }
    }

    pub fn arena(&self) -> Arc<ThumbnailArena> {
        self.arena.clone()
    }

    pub fn get(&self, media_id: MediaId) -> Option<ThumbnailHandle> {
        let mut state = self.inner.write();
        if let Some(&(_, handle)) = state.map.get(&media_id) {
            state.lru_order.retain(|&id| id != media_id);
            state.lru_order.push_back(media_id);
            Some(handle)
        } else {
            None
        }
    }

    pub fn insert(&self, media_id: MediaId, rgb_pixels: &[u8]) -> Option<ThumbnailHandle> {
        let target_slot = {
            let mut state = self.inner.write();
            if let Some(&(slot_idx, _)) = state.map.get(&media_id) {
                slot_idx
            } else if let Some(free_slot) = state.free_slots.pop() {
                free_slot
            } else {
                let evicted_id = state.lru_order.pop_front()?;
                let (evicted_slot, _) = state.map.remove(&evicted_id).unwrap();
                self.arena.invalidate_slot(evicted_slot);
                evicted_slot
            }
        };

        if let Some(handle) = self.arena.write_slot(target_slot, rgb_pixels) {
            let mut state = self.inner.write();
            state.map.insert(media_id, (target_slot, handle));
            state.lru_order.retain(|&id| id != media_id);
            state.lru_order.push_back(media_id);
            Some(handle)
        } else {
            None
        }
    }

    pub fn remove(&self, media_id: MediaId) {
        let mut state = self.inner.write();
        if let Some((slot_idx, _)) = state.map.remove(&media_id) {
            state.lru_order.retain(|&id| id != media_id);
            state.free_slots.push(slot_idx);
            self.arena.invalidate_slot(slot_idx);
        }
    }

    pub fn clear(&self) {
        let mut state = self.inner.write();
        for (_, (slot_idx, _)) in state.map.drain() {
            self.arena.invalidate_slot(slot_idx);
        }
        state.lru_order.clear();
        state.free_slots = (0..self.arena.slot_count()).rev().collect();
    }
}
