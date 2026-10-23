//! Which media a rating hotkey may rate (pure).
//!
//! A rating is data of the library record, so everything with a record can be rated, including corrupt and pending media.
//! Media the library does not know and media whose file is gone (a deleted record) are skipped: there is nothing to rate.

use rustview_core::library::models::MediaId;
use std::collections::HashSet;

/// What the application knows about a medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// A live record.
    Rateable,
    /// The record is marked deleted (the file is gone).
    Deleted,
    /// Neither the loaded folder nor the library has a record.
    Unknown,
}

/// The media of `ids` that may be rated.
pub fn rateable_ids(
    ids: impl IntoIterator<Item = MediaId>,
    standing: impl Fn(MediaId) -> Standing,
) -> HashSet<MediaId> {
    ids.into_iter()
        .filter(|&id| standing(id) == Standing::Rateable)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing(id: MediaId) -> Standing {
        match id {
            1..=9 => Standing::Rateable,
            10..=19 => Standing::Deleted,
            _ => Standing::Unknown,
        }
    }

    #[test]
    fn only_live_records_are_rated() {
        let ids = rateable_ids([1, 2, 10, 20, 3], standing);
        assert_eq!(ids, HashSet::from([1, 2, 3]));
    }

    #[test]
    fn an_empty_selection_rates_nothing() {
        assert!(rateable_ids([], standing).is_empty());
        assert!(rateable_ids([10, 11, 99], standing).is_empty());
    }

    #[test]
    fn a_duplicated_medium_is_rated_once() {
        assert_eq!(rateable_ids([4, 4, 4], standing), HashSet::from([4]));
    }
}
