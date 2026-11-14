//! The order of the columns of the Browse table (`view.table_column_order`).
//!
//! A column is named by a stable key in `settings.json`; its number (0 to 12) is the one the UI and the sort settings use.
//! The widths stay in their own fields (`view.col_*_width`), so a settings file of an older version reads as before and gets
//! the default order.

/// The keys of the columns in their default order; the position is the column's number.
pub const TABLE_COLUMN_KEYS: [&str; 13] = [
    "filename", "path", "modified", "created", "size", "rating", "tags", "width", "height", "ar", "duration", "format",
    "flags",
];

/// The default order as it is stored.
pub fn default_order() -> Vec<String> {
    TABLE_COLUMN_KEYS.iter().map(|&key| key.to_owned()).collect()
}

/// The column numbers in display order for a stored order: unknown keys and repeats are dropped, columns that are missing
/// (a hand-edited file, a column added by a later version) follow at the end in their default order.
pub fn display_order(stored: &[String]) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::with_capacity(TABLE_COLUMN_KEYS.len());
    for key in stored {
        if let Some(column) = TABLE_COLUMN_KEYS.iter().position(|known| known == key)
            && !order.contains(&column)
        {
            order.push(column);
        }
    }
    for column in 0..TABLE_COLUMN_KEYS.len() {
        if !order.contains(&column) {
            order.push(column);
        }
    }
    order
}

/// The stored form of a display order.
pub fn stored_order(display: &[usize]) -> Vec<String> {
    display
        .iter()
        .filter_map(|&column| TABLE_COLUMN_KEYS.get(column))
        .map(|&key| key.to_owned())
        .collect()
}

/// A stored order with every column exactly once.
pub fn sanitize(stored: &[String]) -> Vec<String> {
    stored_order(&display_order(stored))
}

/// The place of every column in the display order (`rank[column]`), the inverse of `display_order`.
pub fn ranks(display: &[usize]) -> Vec<usize> {
    let mut ranks = vec![0; display.len()];
    for (place, &column) in display.iter().enumerate() {
        if let Some(rank) = ranks.get_mut(column) {
            *rank = place;
        }
    }
    ranks
}

/// Moves the column at display place `from` to place `to` (the place it has afterwards); the others keep their order.
/// Places outside the list change nothing.
pub fn move_column(display: &mut Vec<usize>, from: usize, to: usize) {
    if from >= display.len() || to >= display.len() || from == to {
        return;
    }
    let column = display.remove(from);
    display.insert(to, column);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|&name| name.to_owned()).collect()
    }

    #[test]
    fn a_missing_or_damaged_order_becomes_a_complete_one() {
        let all: Vec<usize> = (0..13).collect();
        assert_eq!(display_order(&[]), all, "an older settings file has no order");
        assert_eq!(display_order(&default_order()), all);
        // Unknown keys and repeats go, the rest follows in the default order.
        let order = display_order(&keys(&["size", "nonsense", "filename", "size"]));
        assert_eq!(order[..2], [4, 0]);
        assert_eq!(order.len(), 13);
        assert_eq!(order[2..], [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(sanitize(&keys(&["flags", "flags"]))[0], "flags");
        assert_eq!(sanitize(&keys(&["flags", "flags"])).len(), 13);
    }

    #[test]
    fn the_stored_order_survives_a_round_trip() {
        let display = display_order(&keys(&["rating", "filename", "tags"]));
        assert_eq!(display_order(&stored_order(&display)), display);
        let ranks = ranks(&display);
        assert_eq!((ranks[5], ranks[0], ranks[6]), (0, 1, 2));
        for (place, &column) in display.iter().enumerate() {
            assert_eq!(ranks[column], place);
        }
    }

    #[test]
    fn a_column_moves_to_the_place_it_is_dropped_on() {
        let mut display: Vec<usize> = (0..13).collect();
        move_column(&mut display, 0, 3);
        assert_eq!(
            display[..5],
            [1, 2, 3, 0, 4],
            "moved right: the columns in between move one to the left"
        );
        move_column(&mut display, 3, 0);
        assert_eq!(display, (0..13).collect::<Vec<_>>(), "and back");
        move_column(&mut display, 12, 0);
        assert_eq!(display[..3], [12, 0, 1]);
        let before = display.clone();
        move_column(&mut display, 4, 4);
        move_column(&mut display, 20, 1);
        move_column(&mut display, 1, 20);
        assert_eq!(
            display, before,
            "nothing moves for the same place or a place outside the list"
        );
    }
}
