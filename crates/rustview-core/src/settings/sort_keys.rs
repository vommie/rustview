//! The Browse list's sort order in `settings.json` (`view.sort_keys`, with `view.sort_column` and `view.sort_order`
//! repeating the first level for older versions).

use super::schema::{SortKeySetting, ViewSettings};
use crate::query::{SortColumn, SortKey, SortOrder};

/// The column with that stored name; `None` for a name no version knows (`SortColumn::from_name` would make it the file
/// name).
fn known_column(name: &str) -> Option<SortColumn> {
    let column = SortColumn::from_name(name);
    (column.as_str() == name).then_some(column)
}

/// The sort order the settings hold: the stored list, or the single column of a settings file of 0.6.15 and older.
/// Unknown columns and repeats are dropped; the result is never empty.
pub fn sort_keys_of(view: &ViewSettings) -> Vec<SortKey> {
    let stored: Vec<SortKey> = if view.sort_keys.is_empty() {
        vec![SortKey::new(
            SortColumn::from_name(&view.sort_column),
            SortOrder::from_name(&view.sort_order),
        )]
    } else {
        view.sort_keys
            .iter()
            .filter_map(|key| {
                Some(SortKey::new(
                    known_column(&key.column)?,
                    SortOrder::from_name(&key.order),
                ))
            })
            .collect()
    };
    SortKey::sanitized(&stored)
}

/// Writes the sort order into the settings: the list, and its first level into the two fields older versions read.
pub fn store_sort_keys(view: &mut ViewSettings, keys: &[SortKey]) {
    let keys = SortKey::sanitized(keys);
    view.sort_keys = keys
        .iter()
        .map(|key| SortKeySetting {
            column: key.column.as_str().to_owned(),
            order: key.order.as_str().to_owned(),
        })
        .collect();
    if let Some(first) = keys.first() {
        first.column.as_str().clone_into(&mut view.sort_column);
        first.order.as_str().clone_into(&mut view.sort_order);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_settings_file_gives_its_single_column() {
        let mut view = ViewSettings::default();
        "Rating".clone_into(&mut view.sort_column);
        "Descending".clone_into(&mut view.sort_order);
        assert_eq!(view.sort_keys.len(), 0);
        assert_eq!(
            sort_keys_of(&view),
            [SortKey::new(SortColumn::Rating, SortOrder::Descending)]
        );
    }

    #[test]
    fn the_list_survives_a_round_trip_and_keeps_the_old_fields_in_step() {
        let keys = [
            SortKey::new(SortColumn::Rating, SortOrder::Descending),
            SortKey::new(SortColumn::Filename, SortOrder::Ascending),
        ];
        let mut view = ViewSettings::default();
        store_sort_keys(&mut view, &keys);
        assert_eq!(sort_keys_of(&view), keys);
        assert_eq!(
            (view.sort_column.as_str(), view.sort_order.as_str()),
            ("Rating", "Descending")
        );
    }

    #[test]
    fn a_hand_edited_list_is_cleaned() {
        let mut view = ViewSettings::default();
        let stored = |column: &str, order: &str| SortKeySetting {
            column: column.to_owned(),
            order: order.to_owned(),
        };
        view.sort_keys = vec![
            stored("Nonsense", "Ascending"),
            stored("Tags", "sideways"),
            stored("Tags", "Descending"),
        ];
        assert_eq!(
            sort_keys_of(&view),
            [SortKey::new(SortColumn::Tags, SortOrder::Ascending)]
        );
        view.sort_keys = vec![stored("Nonsense", "Ascending")];
        assert_eq!(
            sort_keys_of(&view),
            [SortKey::new(SortColumn::Filename, SortOrder::Ascending)],
            "nothing usable"
        );
    }
}
