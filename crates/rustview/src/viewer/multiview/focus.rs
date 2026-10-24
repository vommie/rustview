//! The selected medium of a Multiview (pure): the one tile the keys act on.
//!
//! A Multiview of several tiles has at most one selected tile. Only a plain click on a medium (or on its media bar) selects
//! it; nothing clears the selection but the tile leaving the view. A Multiview of one tile always acts on that tile, so the
//! lone tile counts as selected, and when the view grows from one tile to more, that tile stays the selected one. A view
//! that opens with several tiles starts with none, and a key that needs a target refuses then (`docs/multiview_focus.md`).

use super::follow::TileKey;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Focus(Option<TileKey>);

impl Focus {
    /// A focus that follows the rules for `tiles` (a lone tile is selected, several start without).
    pub fn for_tiles(tiles: &[TileKey]) -> Self {
        let mut focus = Self::default();
        focus.normalize(tiles);
        focus
    }

    /// The tile a key acts on: the only tile, or the selected one of several (`None` = nothing selected).
    pub fn target(&self, tiles: &[TileKey]) -> Option<TileKey> {
        match tiles {
            [only] => Some(*only),
            _ => self.0.filter(|key| tiles.contains(key)),
        }
    }

    /// The tile that carries the marks of the selection: only a view of two or more tiles shows them.
    pub fn marked(&self, tiles: &[TileKey]) -> Option<TileKey> {
        (tiles.len() >= 2).then(|| self.target(tiles)).flatten()
    }

    /// Selects `key`; returns whether the selection changed (a key that is not shown changes nothing).
    pub fn select(&mut self, key: TileKey, tiles: &[TileKey]) -> bool {
        if !tiles.contains(&key) || self.0 == Some(key) {
            return false;
        }
        self.0 = Some(key);
        true
    }

    /// Brings the selection in line with the tiles after they changed: a tile that left is no longer selected, a lone tile
    /// is. Returns whether the selection changed.
    pub fn normalize(&mut self, tiles: &[TileKey]) -> bool {
        let next = match tiles {
            [only] => Some(*only),
            _ => self.0.filter(|key| tiles.contains(key)),
        };
        std::mem::replace(&mut self.0, next) != next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_of_several_tiles_starts_without_a_selection() {
        let focus = Focus::for_tiles(&[1, 2, 3]);
        assert_eq!(focus.target(&[1, 2, 3]), None);
        assert_eq!(focus.marked(&[1, 2, 3]), None);
    }

    #[test]
    fn a_lone_tile_is_the_target_without_marks() {
        let focus = Focus::for_tiles(&[7]);
        assert_eq!(focus.target(&[7]), Some(7));
        assert_eq!(focus.marked(&[7]), None);
        // Even a selection that was never normalized acts on the lone tile.
        assert_eq!(Focus::default().target(&[7]), Some(7));
    }

    #[test]
    fn a_click_selects_and_only_another_click_moves_it() {
        let tiles = [1, 2, 3];
        let mut focus = Focus::for_tiles(&tiles);
        assert!(focus.select(2, &tiles));
        assert!(
            !focus.select(2, &tiles),
            "selecting the selected tile again changes nothing"
        );
        assert_eq!(focus.marked(&tiles), Some(2));
        assert!(focus.select(3, &tiles));
        assert_eq!(focus.target(&tiles), Some(3));
        assert!(!focus.select(9, &tiles), "a tile that is not shown cannot be selected");
        assert_eq!(focus.target(&tiles), Some(3));
    }

    #[test]
    fn the_lone_tile_stays_selected_when_the_view_grows() {
        let mut focus = Focus::for_tiles(&[5]);
        assert!(!focus.normalize(&[5, 6]));
        assert_eq!(focus.marked(&[5, 6]), Some(5));
    }

    #[test]
    fn a_selected_tile_that_leaves_takes_the_selection_with_it() {
        let mut focus = Focus::for_tiles(&[1, 2, 3]);
        focus.select(2, &[1, 2, 3]);
        assert!(focus.normalize(&[1, 3]));
        assert_eq!(focus.target(&[1, 3]), None);
        // Down to one tile: that tile is the target again, and stays it when the view grows once more.
        assert!(focus.normalize(&[3]));
        assert!(!focus.normalize(&[3, 4]));
        assert_eq!(focus.target(&[3, 4]), Some(3));
    }

    #[test]
    fn a_tile_that_stays_keeps_the_selection_when_others_leave() {
        let mut focus = Focus::for_tiles(&[1, 2, 3, 4]);
        focus.select(4, &[1, 2, 3, 4]);
        assert!(!focus.normalize(&[4, 1]));
        assert_eq!(focus.target(&[4, 1]), Some(4));
    }
}
