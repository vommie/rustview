//! Which tile an action such as F2 (rename) acts on (pure).

/// The tile under the pointer, else the last hovered tile, else the first tile. `ids` are the media ids in slot order.
pub fn pick_target(
    under_pointer: Option<usize>,
    hovered: Option<usize>,
    last_hovered: Option<u64>,
    ids: &[u64],
) -> Option<u64> {
    under_pointer
        .or(hovered)
        .and_then(|index| ids.get(index).copied())
        .or(last_hovered.filter(|id| ids.contains(id)))
        .or_else(|| ids.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: [u64; 3] = [10, 20, 30];

    #[test]
    fn the_tile_under_the_pointer_wins() {
        assert_eq!(pick_target(Some(2), Some(0), Some(20), &IDS), Some(30));
    }

    #[test]
    fn the_hovered_tile_is_used_when_the_pointer_is_elsewhere() {
        assert_eq!(pick_target(None, Some(1), Some(30), &IDS), Some(20));
    }

    #[test]
    fn the_last_hovered_tile_counts_while_it_is_still_shown() {
        assert_eq!(pick_target(None, None, Some(30), &IDS), Some(30));
        assert_eq!(
            pick_target(None, None, Some(99), &IDS),
            Some(10),
            "a removed tile falls back to the first"
        );
    }

    #[test]
    fn a_stale_index_falls_through() {
        assert_eq!(pick_target(Some(7), None, Some(20), &IDS), Some(20));
        assert_eq!(pick_target(Some(7), None, None, &IDS), Some(10));
    }

    #[test]
    fn without_tiles_there_is_no_target() {
        assert_eq!(pick_target(Some(0), Some(0), Some(1), &[]), None);
    }
}
