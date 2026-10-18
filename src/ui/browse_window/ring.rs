//! A fixed pool of tile slots over a long list: the item at list position `g` always lives in slot `g % pool`.
//!
//! Scrolling by one row therefore changes the content of one row of slots only; every other tile keeps its slot, its Slint
//! instance and its texture. This module is pure position arithmetic; it knows nothing about Slint or thumbnails.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingWindow {
    pool: usize,
    range: Range<usize>,
}

impl RingWindow {
    /// An empty window over a pool of `pool` slots (`pool` is at least 1).
    pub fn new(pool: usize) -> Self {
        Self { pool: pool.max(1), range: 0..0 }
    }

    pub fn pool(&self) -> usize {
        self.pool
    }

    /// The list positions the pool holds right now.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn slot_of(&self, position: usize) -> usize {
        position % self.pool
    }

    /// The slot that shows `position`, if the pool holds it.
    pub fn slot_if_held(&self, position: usize) -> Option<usize> {
        self.range.contains(&position).then(|| position % self.pool)
    }

    /// Forgets the content and takes a new pool size (rows/columns changed, new list).
    pub fn reset(&mut self, pool: usize) {
        self.pool = pool.max(1);
        self.range = 0..0;
    }

    /// Moves the window to `target` (at most `pool` positions) and returns the positions whose slots need new content, in
    /// ascending order. Positions that stay inside the window are not returned.
    pub fn advance(&mut self, target: Range<usize>) -> Vec<usize> {
        let target = if target.len() > self.pool { target.start..target.start + self.pool } else { target };
        let old = std::mem::replace(&mut self.range, target.clone());
        if old.is_empty() || old.end <= target.start || target.end <= old.start {
            return target.collect();
        }
        let mut fresh: Vec<usize> = (target.start..old.start.min(target.end)).collect();
        fresh.extend(old.end.max(target.start)..target.end);
        fresh
    }
}
