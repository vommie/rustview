//! Which thumbnails are decoded ahead of a scrolling view.
//!
//! The tile pool holds a few rows beyond the viewport, enough while the list moves slowly. A fast scroll crosses those rows
//! within a frame or two, so their tiles would show up empty and get their pictures a frame later. While the view moves, the
//! rows it is heading for are therefore decoded into the RAM cache before they have tiles: the faster the view moves, the
//! further ahead. The cache has a byte budget, so this never grows the memory use; it only spends decoding on rows the user
//! is about to see. A jump (the scroll bar, Page Down held) gets no look-ahead: where the next jump lands is unknown.

use std::ops::Range;

/// Rows decoded ahead per row the window moved since the last report.
const ROWS_AHEAD_PER_ROW_MOVED: usize = 4;
/// The most rows decoded ahead (at 8 tiles a row about 200 thumbnails, well below a second of decoding).
const MAX_ROWS_AHEAD: usize = 24;
/// A window that moved further than this in one step jumped.
const JUMP_ROWS: usize = 24;

/// The list positions beyond the new window `target` to decode ahead, given the window before (`previous`); empty when the
/// view did not move, jumped or has nothing in that direction.
pub fn prefetch_range(previous: &Range<usize>, target: &Range<usize>, columns: usize, total: usize) -> Range<usize> {
    if columns == 0 || previous.is_empty() || target.is_empty() || previous.start == target.start {
        return 0..0;
    }
    let moved_rows = previous.start.abs_diff(target.start).div_ceil(columns);
    if moved_rows > JUMP_ROWS {
        return 0..0;
    }
    let ahead = (moved_rows * ROWS_AHEAD_PER_ROW_MOVED).min(MAX_ROWS_AHEAD) * columns;
    if target.start > previous.start {
        target.end.min(total)..(target.end + ahead).min(total)
    } else {
        target.start.saturating_sub(ahead)..target.start
    }
}

#[cfg(test)]
mod tests {
    use super::prefetch_range;

    #[test]
    fn a_moving_view_decodes_the_rows_it_is_heading_for() {
        // 8 columns, the window moved down by two rows: eight rows ahead of the window's end.
        assert_eq!(prefetch_range(&(80..160), &(96..176), 8, 10_000), 176..240);
        // Moving up: the rows in front of the window's start.
        assert_eq!(prefetch_range(&(96..176), &(80..160), 8, 10_000), 16..80);
        // A fast scroll is capped.
        assert_eq!(prefetch_range(&(0..80), &(160..240), 8, 10_000), 240..432);
    }

    #[test]
    fn nothing_is_decoded_ahead_without_a_direction() {
        assert_eq!(
            prefetch_range(&(80..160), &(80..160), 8, 10_000),
            0..0,
            "the view did not move"
        );
        assert_eq!(prefetch_range(&(0..0), &(80..160), 8, 10_000), 0..0, "a new list");
        assert_eq!(prefetch_range(&(0..80), &(4000..4080), 8, 10_000), 0..0, "a jump");
        assert_eq!(prefetch_range(&(80..160), &(96..176), 0, 10_000), 0..0);
    }

    #[test]
    fn the_look_ahead_stays_inside_the_list() {
        assert_eq!(prefetch_range(&(80..160), &(96..176), 8, 200), 176..200);
        assert_eq!(prefetch_range(&(80..160), &(96..176), 8, 176), 176..176);
        assert_eq!(prefetch_range(&(16..96), &(8..88), 8, 10_000), 0..8);
        assert!(prefetch_range(&(8..88), &(0..80), 8, 10_000).is_empty());
    }
}
