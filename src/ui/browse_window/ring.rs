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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_window_fills_every_position() {
        let mut ring = RingWindow::new(8);
        assert_eq!(ring.advance(0..8), (0..8).collect::<Vec<_>>());
        assert_eq!(ring.range(), 0..8);
    }

    #[test]
    fn scrolling_by_one_row_changes_one_row_of_slots() {
        let mut ring = RingWindow::new(24);
        ring.advance(0..24);
        let fresh = ring.advance(4..28);
        assert_eq!(fresh, vec![24, 25, 26, 27]);
        let slots: Vec<usize> = fresh.iter().map(|&p| ring.slot_of(p)).collect();
        assert_eq!(slots, vec![0, 1, 2, 3], "the slots of the rows that left are reused");
    }

    #[test]
    fn scrolling_up_refreshes_the_rows_that_enter_at_the_top() {
        let mut ring = RingWindow::new(24);
        ring.advance(8..32);
        assert_eq!(ring.advance(4..28), vec![4, 5, 6, 7]);
    }

    #[test]
    fn a_jump_refreshes_everything() {
        let mut ring = RingWindow::new(10);
        ring.advance(0..10);
        assert_eq!(ring.advance(500..510), (500..510).collect::<Vec<_>>());
    }

    #[test]
    fn an_unchanged_window_refreshes_nothing() {
        let mut ring = RingWindow::new(10);
        ring.advance(20..30);
        assert!(ring.advance(20..30).is_empty());
    }

    #[test]
    fn two_positions_never_share_a_slot_inside_the_window() {
        let mut ring = RingWindow::new(7);
        ring.advance(100..107);
        let mut slots: Vec<usize> = ring.range().map(|p| ring.slot_of(p)).collect();
        slots.sort_unstable();
        assert_eq!(slots, (0..7).collect::<Vec<_>>());
    }

    #[test]
    fn positions_outside_the_window_have_no_slot() {
        let mut ring = RingWindow::new(10);
        ring.advance(10..20);
        assert_eq!(ring.slot_if_held(15), Some(5));
        assert_eq!(ring.slot_if_held(9), None);
        assert_eq!(ring.slot_if_held(20), None);
    }

    #[test]
    fn a_shorter_window_at_the_end_of_the_list_is_fine() {
        let mut ring = RingWindow::new(10);
        ring.advance(0..10);
        assert_eq!(ring.advance(5..12), vec![10, 11]);
    }

    #[test]
    fn an_oversized_target_is_cut_to_the_pool() {
        let mut ring = RingWindow::new(4);
        assert_eq!(ring.advance(0..9), vec![0, 1, 2, 3]);
    }
}
