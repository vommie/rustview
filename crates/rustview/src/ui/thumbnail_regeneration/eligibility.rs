//! Single source of truth for which selected items "Thumbnail (Random Pos.)" can act on.
//!
//! Only valid videos qualify: images have no duration, so they cannot get a thumbnail from a random position
//! (the general thumbnail refresh for any media type is "Refresh > File (Full)"). A video is valid when it is
//! physically available according to the library state, i.e. it is not deleted/orphaned, not a pending stub and not
//! flagged corrupted. All decisions are made from in-memory state only; the drive is never touched here.

use rustview_core::library::models::MediaId;
use rustview_core::media::MediaType;
use rustview_core::query::filters::IndexedMediaRecord;
use crate::ui::state::UiState;

/// The facts about one media item that decide whether its thumbnail can be regenerated at a random position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemFlags {
    pub is_video: bool,
    pub is_deleted: bool,
    pub is_pending: bool,
    pub is_corrupted: bool,
}

impl ItemFlags {
    pub fn of(item: &IndexedMediaRecord) -> Self {
        Self {
            is_video: matches!(item.record.media_type, MediaType::Video),
            is_deleted: item.is_deleted,
            is_pending: item.is_pending,
            is_corrupted: item.is_corrupted,
        }
    }

    /// True for a valid, physically available video.
    pub fn is_regenerable(&self) -> bool {
        self.is_video && !self.is_deleted && !self.is_pending && !self.is_corrupted
    }
}

/// Summary of the current selection used by the toolbar button, its tooltip and the context menu entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegenerationEligibility {
    /// Number of selected media items (all types and states).
    pub selected: usize,
    /// Number of selected items that are valid videos; only these are regenerated and counted.
    pub videos: usize,
}

impl RegenerationEligibility {
    /// Counts the eligible items among the flags of all selected items.
    pub fn from_flags(selected: usize, flags: impl Iterator<Item = ItemFlags>) -> Self {
        Self { selected, videos: flags.filter(ItemFlags::is_regenerable).count() }
    }

    /// The button and the menu entry are enabled only if at least one valid video is selected.
    pub fn is_actionable(&self) -> bool {
        self.videos > 0
    }

    /// Selected items that will be ignored (images, deleted, pending or corrupted files).
    pub fn skipped(&self) -> usize {
        self.selected.saturating_sub(self.videos)
    }
}

/// Resolves a selected id to its loaded record via the row map. In-memory only: ids that are not part of the
/// loaded list are ignored instead of being looked up in the database or on disk.
pub(crate) fn loaded_item(st: &UiState, id: MediaId) -> Option<&IndexedMediaRecord> {
    let row = *st.media_row_map.get(&id)?;
    let index = if st.active_indices.is_empty() { row } else { *st.active_indices.get(row)? };
    st.all_folder_media.get(index).filter(|m| m.record.id == id)
}

/// All selected items that are valid videos, in selection set order.
pub fn eligible_items(st: &UiState) -> impl Iterator<Item = &IndexedMediaRecord> {
    st.file_selection
        .selected_ids()
        .iter()
        .filter_map(|&id| loaded_item(st, id))
        .filter(|item| ItemFlags::of(item).is_regenerable())
}

/// Evaluates the current selection in O(selected items).
pub fn evaluate(st: &UiState) -> RegenerationEligibility {
    let selected = st.file_selection.count();
    let flags = st
        .file_selection
        .selected_ids()
        .iter()
        .filter_map(|&id| loaded_item(st, id))
        .map(ItemFlags::of);
    RegenerationEligibility::from_flags(selected, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(is_video: bool, is_deleted: bool, is_pending: bool, is_corrupted: bool) -> ItemFlags {
        ItemFlags { is_video, is_deleted, is_pending, is_corrupted }
    }

    const VALID_VIDEO: ItemFlags = ItemFlags { is_video: true, is_deleted: false, is_pending: false, is_corrupted: false };
    const IMAGE: ItemFlags = ItemFlags { is_video: false, is_deleted: false, is_pending: false, is_corrupted: false };

    #[test]
    fn only_valid_available_videos_are_regenerable() {
        assert!(VALID_VIDEO.is_regenerable());
        assert!(!IMAGE.is_regenerable());
        assert!(!flags(true, true, false, false).is_regenerable(), "deleted/orphaned video");
        assert!(!flags(true, false, true, false).is_regenerable(), "pending stub");
        assert!(!flags(true, false, false, true).is_regenerable(), "corrupted video");
        assert!(!flags(false, true, true, true).is_regenerable());
    }

    #[test]
    fn empty_selection_is_not_actionable() {
        let e = RegenerationEligibility::from_flags(0, std::iter::empty());
        assert!(!e.is_actionable());
        assert_eq!(e.skipped(), 0);
    }

    #[test]
    fn images_only_selection_is_not_actionable() {
        let e = RegenerationEligibility::from_flags(3, [IMAGE; 3].into_iter());
        assert!(!e.is_actionable());
        assert_eq!((e.videos, e.skipped()), (0, 3));
    }

    #[test]
    fn unavailable_only_selection_is_not_actionable() {
        let items = [flags(true, true, false, false), flags(true, false, true, false), flags(false, true, false, false)];
        let e = RegenerationEligibility::from_flags(items.len(), items.into_iter());
        assert!(!e.is_actionable());
        assert_eq!(e.skipped(), 3);
    }

    #[test]
    fn mixed_selection_with_one_valid_video_is_actionable_and_counts_only_videos() {
        let items = [IMAGE, VALID_VIDEO, flags(true, true, false, false), IMAGE, VALID_VIDEO];
        let e = RegenerationEligibility::from_flags(items.len(), items.into_iter());
        assert!(e.is_actionable());
        assert_eq!((e.selected, e.videos, e.skipped()), (5, 2, 3));
    }

    #[test]
    fn selected_ids_that_are_not_loaded_never_count() {
        // 4 selected ids, but only one resolved to a loaded valid video.
        let e = RegenerationEligibility::from_flags(4, [VALID_VIDEO].into_iter());
        assert_eq!((e.videos, e.skipped()), (1, 3));
    }
}
