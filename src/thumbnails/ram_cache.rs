//! The one RAM cache of decoded thumbnails: byte-budgeted, O(1) per operation, shared by the loader and the Browse view.
//!
//! Entries are reference-counted pixel buffers, so a tile that shows a thumbnail, the cache and a drag ghost all share one
//! allocation and a hit never copies pixels. Eviction removes the least recently used entries until the budget holds; an
//! entry that a tile still shows stays alive through its own reference, the cache only forgets it.

use crate::library::models::MediaId;
use parking_lot::Mutex;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Media ids are dense counters; a multiplicative hash is enough and much cheaper than SipHash.
#[derive(Default, Clone, Copy)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

pub type IdBuildHasher = BuildHasherDefault<IdHasher>;

/// A decoded thumbnail with its size; the pixels are shared with the cache and with every tile that shows them.
#[derive(Debug, Clone)]
pub struct CachedThumbnail {
    pub width: u32,
    pub height: u32,
    pub pixels: SharedPixelBuffer<Rgb8Pixel>,
}

impl CachedThumbnail {
    pub fn new(pixels: SharedPixelBuffer<Rgb8Pixel>) -> Self {
        Self { width: pixels.width(), height: pixels.height(), pixels }
    }

    /// Copies tightly packed RGB8 bytes into a new shared buffer.
    pub fn from_rgb8(width: u32, height: u32, rgb: &[u8]) -> Self {
        let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
        let target = pixels.make_mut_bytes();
        let len = target.len().min(rgb.len());
        target[..len].copy_from_slice(&rgb[..len]);
        Self { width, height, pixels }
    }
}

const NIL: usize = usize::MAX;

struct Node {
    id: MediaId,
    pixels: SharedPixelBuffer<Rgb8Pixel>,
    bytes: usize,
    prev: usize,
    next: usize,
}

struct Inner {
    index: HashMap<MediaId, usize, IdBuildHasher>,
    nodes: Vec<Node>,
    free: Vec<usize>,
    /// Most recently used.
    head: usize,
    /// Least recently used.
    tail: usize,
    used_bytes: usize,
    budget_bytes: usize,
    hits: u64,
    misses: u64,
    evictions: u64,
}

/// Counters for the logs and the settings page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RamCacheStats {
    pub entries: usize,
    pub used_bytes: usize,
    pub budget_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

pub struct ThumbnailRamCache {
    inner: Mutex<Inner>,
}

fn buffer_bytes(pixels: &SharedPixelBuffer<Rgb8Pixel>) -> usize {
    pixels.width() as usize * pixels.height() as usize * std::mem::size_of::<Rgb8Pixel>()
}

impl Inner {
    fn unlink(&mut self, slot: usize) {
        let (prev, next) = (self.nodes[slot].prev, self.nodes[slot].next);
        if prev == NIL {
            self.head = next;
        } else {
            self.nodes[prev].next = next;
        }
        if next == NIL {
            self.tail = prev;
        } else {
            self.nodes[next].prev = prev;
        }
    }

    fn push_front(&mut self, slot: usize) {
        self.nodes[slot].prev = NIL;
        self.nodes[slot].next = self.head;
        if self.head != NIL {
            let old_head = self.head;
            self.nodes[old_head].prev = slot;
        }
        self.head = slot;
        if self.tail == NIL {
            self.tail = slot;
        }
    }

    fn remove_slot(&mut self, slot: usize) {
        self.unlink(slot);
        let id = self.nodes[slot].id;
        self.used_bytes -= self.nodes[slot].bytes;
        self.nodes[slot].bytes = 0;
        // Drop the pixels now; the slot is recycled by the next insert.
        self.nodes[slot].pixels = SharedPixelBuffer::new(0, 0);
        self.index.remove(&id);
        self.free.push(slot);
    }

    fn evict_to_budget(&mut self) {
        while self.used_bytes > self.budget_bytes && self.tail != NIL {
            let victim = self.tail;
            self.remove_slot(victim);
            self.evictions += 1;
        }
    }
}

impl ThumbnailRamCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                index: HashMap::with_hasher(IdBuildHasher::default()),
                nodes: Vec::new(),
                free: Vec::new(),
                head: NIL,
                tail: NIL,
                used_bytes: 0,
                budget_bytes,
                hits: 0,
                misses: 0,
                evictions: 0,
            }),
        }
    }

    /// The cached pixels (a reference-count bump, no copy); the entry becomes the most recently used.
    pub fn get(&self, id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        let mut inner = self.inner.lock();
        let Some(&slot) = inner.index.get(&id) else {
            inner.misses += 1;
            return None;
        };
        inner.hits += 1;
        if inner.head != slot {
            inner.unlink(slot);
            inner.push_front(slot);
        }
        Some(inner.nodes[slot].pixels.clone())
    }

    pub fn get_thumbnail(&self, id: MediaId) -> Option<CachedThumbnail> {
        self.get(id).map(CachedThumbnail::new)
    }

    /// Membership test that neither counts as a use nor changes the order.
    pub fn contains(&self, id: MediaId) -> bool {
        self.inner.lock().index.contains_key(&id)
    }

    /// Stores the pixels (replacing an older entry of the same id) and evicts down to the budget. An entry larger than the whole
    /// budget is not stored.
    pub fn insert(&self, id: MediaId, pixels: SharedPixelBuffer<Rgb8Pixel>) {
        let bytes = buffer_bytes(&pixels);
        let mut inner = self.inner.lock();
        if bytes > inner.budget_bytes {
            return;
        }
        if let Some(&slot) = inner.index.get(&id) {
            inner.remove_slot(slot);
        }
        let node = Node { id, pixels, bytes, prev: NIL, next: NIL };
        let slot = match inner.free.pop() {
            Some(slot) => {
                inner.nodes[slot] = node;
                slot
            }
            None => {
                inner.nodes.push(node);
                inner.nodes.len() - 1
            }
        };
        inner.index.insert(id, slot);
        inner.used_bytes += bytes;
        inner.push_front(slot);
        inner.evict_to_budget();
    }

    /// Copies tightly packed RGB8 bytes into a shared buffer, stores it and returns it.
    pub fn insert_rgb(&self, id: MediaId, width: u32, height: u32, rgb: &[u8]) -> CachedThumbnail {
        let thumbnail = CachedThumbnail::from_rgb8(width, height, rgb);
        self.insert(id, thumbnail.pixels.clone());
        thumbnail
    }

    pub fn remove(&self, id: MediaId) {
        let mut inner = self.inner.lock();
        if let Some(&slot) = inner.index.get(&id) {
            inner.remove_slot(slot);
        }
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.index.clear();
        inner.nodes.clear();
        inner.free.clear();
        inner.head = NIL;
        inner.tail = NIL;
        inner.used_bytes = 0;
    }

    /// Changes the budget and evicts at once if the cache is over it.
    pub fn set_budget(&self, budget_bytes: usize) {
        let mut inner = self.inner.lock();
        inner.budget_bytes = budget_bytes;
        inner.evict_to_budget();
    }

    pub fn stats(&self) -> RamCacheStats {
        let inner = self.inner.lock();
        RamCacheStats {
            entries: inner.index.len(),
            used_bytes: inner.used_bytes,
            budget_bytes: inner.budget_bytes,
            hits: inner.hits,
            misses: inner.misses,
            evictions: inner.evictions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(side: u32) -> SharedPixelBuffer<Rgb8Pixel> {
        SharedPixelBuffer::new(side, side)
    }

    const TILE_BYTES: usize = 10 * 10 * 3;

    #[test]
    fn a_hit_shares_the_allocation() {
        let cache = ThumbnailRamCache::new(10 * TILE_BYTES);
        let pixels = tile(10);
        cache.insert(1, pixels.clone());
        let hit = cache.get(1).expect("cached");
        assert_eq!(hit.as_bytes().as_ptr(), pixels.as_bytes().as_ptr());
    }

    #[test]
    fn the_least_recently_used_entry_is_evicted_first() {
        let cache = ThumbnailRamCache::new(3 * TILE_BYTES);
        for id in 1..=3 {
            cache.insert(id, tile(10));
        }
        assert!(cache.get(1).is_some());
        cache.insert(4, tile(10));
        assert!(!cache.contains(2), "2 was the least recently used");
        assert!(cache.contains(1) && cache.contains(3) && cache.contains(4));
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn the_budget_is_counted_in_bytes_not_entries() {
        let cache = ThumbnailRamCache::new(4 * TILE_BYTES);
        cache.insert(1, tile(20));
        assert_eq!(cache.stats().used_bytes, 4 * TILE_BYTES);
        cache.insert(2, tile(10));
        assert!(!cache.contains(1), "one big tile makes room for the small one");
        assert_eq!(cache.stats().used_bytes, TILE_BYTES);
    }

    #[test]
    fn an_entry_larger_than_the_budget_is_not_stored() {
        let cache = ThumbnailRamCache::new(TILE_BYTES - 1);
        cache.insert(1, tile(10));
        assert!(cache.stats().entries == 0 && cache.stats().used_bytes == 0);
    }

    #[test]
    fn replacing_an_entry_keeps_the_accounting_exact() {
        let cache = ThumbnailRamCache::new(10 * TILE_BYTES);
        cache.insert(1, tile(10));
        cache.insert(1, tile(20));
        let stats = cache.stats();
        assert_eq!((stats.entries, stats.used_bytes), (1, 4 * TILE_BYTES));
    }

    #[test]
    fn shrinking_the_budget_evicts_immediately() {
        let cache = ThumbnailRamCache::new(10 * TILE_BYTES);
        for id in 1..=10 {
            cache.insert(id, tile(10));
        }
        cache.set_budget(2 * TILE_BYTES);
        let stats = cache.stats();
        assert_eq!(stats.entries, 2);
        assert!(cache.contains(10) && cache.contains(9));
    }

    #[test]
    fn slots_are_recycled_after_removal() {
        let cache = ThumbnailRamCache::new(2 * TILE_BYTES);
        for id in 1..=100 {
            cache.insert(id, tile(10));
        }
        let inner = cache.inner.lock();
        assert!(inner.nodes.len() <= 3, "the slab must not grow with churn, got {}", inner.nodes.len());
    }

    #[test]
    fn hits_and_misses_are_counted() {
        let cache = ThumbnailRamCache::new(10 * TILE_BYTES);
        cache.insert(1, tile(10));
        let _ = cache.get(1);
        let _ = cache.get(2);
        let stats = cache.stats();
        assert_eq!((stats.hits, stats.misses), (1, 1));
    }
}
