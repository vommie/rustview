//! The selection of the Browse view over folders and media together.
//!
//! The media keep their selection in `UiState::file_selection`, the folder rows theirs in `BrowseFolderRows`; both are ordinary
//! sets. What joins them is the list position (`[Up][folders][media][new folders]`): a click, a Shift range or a key press moves
//! over positions, whatever sits there. The Shift anchor and the "cursor" (what the keys move from) live next to the selection
//! they belong to; at most one of the two holds them (`browse_anchor`, `browse_cursor`).

use super::rows::{FolderRowRef, Slot};
use crate::ui::selection::model::FileSelectionModel;
use crate::ui::state::UiState;
use rustview_core::library::models::{FolderId, MediaId};
use std::collections::HashSet;

/// One selectable thing of the Browse view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseItem {
    Media(MediaId),
    Folder(FolderRowRef),
}

/// What a selection change touched, so that only those tiles are redrawn.
#[derive(Debug, Default)]
pub struct SelectionDelta {
    pub media: Vec<MediaId>,
    pub folders_changed: bool,
}

impl SelectionDelta {
    pub fn is_empty(&self) -> bool {
        self.media.is_empty() && !self.folders_changed
    }
}

type FolderSelectionSnapshot = (HashSet<FolderId>, bool);

impl UiState {
    /// Where a Shift range starts; the media anchor wins, as it is the one every other selection function sets.
    pub fn browse_anchor(&self) -> Option<BrowseItem> {
        self.file_selection
            .anchor_id()
            .map(BrowseItem::Media)
            .or_else(|| self.folder_rows.anchor().map(BrowseItem::Folder))
            .filter(|item| self.browse_position_of(*item).is_some())
    }

    /// The item the keys move from: the last one clicked or reached by a key, else (media only) the first selected one.
    pub fn browse_cursor(&self) -> Option<BrowseItem> {
        let cursor = self
            .file_selection
            .last_clicked_id()
            .map(BrowseItem::Media)
            .or_else(|| self.folder_rows.cursor().map(BrowseItem::Folder))
            .filter(|item| self.browse_position_of(*item).is_some());
        cursor
            .or_else(|| self.browse_anchor())
            .or_else(|| {
                self.file_selection
                    .selected_ids()
                    .iter()
                    .filter_map(|id| self.media_row_map.get(id).map(|&row| (row, *id)))
                    .min()
                    .map(|(_, id)| BrowseItem::Media(id))
            })
            .or_else(|| {
                self.folder_rows
                    .selected_folders_ordered()
                    .first()
                    .map(|&id| BrowseItem::Folder(FolderRowRef::Folder(id)))
                    .or_else(|| {
                        self.folder_rows
                            .up_selected()
                            .then_some(BrowseItem::Folder(FolderRowRef::Up))
                    })
            })
    }

    /// The list position of an item, `None` when the view does not show it.
    pub fn browse_position_of(&self, item: BrowseItem) -> Option<usize> {
        match item {
            BrowseItem::Media(id) => self.view_position_of(id),
            BrowseItem::Folder(row) => self.folder_rows.position_of(row, self.active_indices.len()),
        }
    }

    /// The item at a list position.
    pub fn browse_item_at(&self, position: usize) -> Option<BrowseItem> {
        match self.slot_at(position) {
            Slot::Folder(row) => Some(BrowseItem::Folder(row)),
            Slot::Media(row) => self.item_at(row).map(|item| BrowseItem::Media(item.record.id)),
            Slot::Empty => None,
        }
    }

    /// Selects one medium and nothing else.
    pub fn select_single_media(&mut self, id: MediaId) {
        self.folder_rows.clear_selection();
        self.file_selection.select_single(id);
    }

    /// Selected folders and media.
    pub fn browse_selection_counts(&self) -> (usize, usize) {
        (self.folder_rows.selection_count(), self.file_selection.count())
    }

    fn set_browse_anchor(&mut self, item: BrowseItem) {
        match item {
            BrowseItem::Media(id) => {
                self.file_selection.set_anchor_id(Some(id));
                self.folder_rows.set_anchor(None);
            }
            BrowseItem::Folder(row) => {
                self.file_selection.set_anchor_id(None);
                self.folder_rows.set_anchor(Some(row));
            }
        }
    }

    fn set_browse_cursor(&mut self, item: BrowseItem) {
        match item {
            BrowseItem::Media(id) => {
                self.file_selection.set_last_clicked_id(Some(id));
                self.folder_rows.set_cursor(None);
            }
            BrowseItem::Folder(row) => {
                self.file_selection.set_last_clicked_id(None);
                self.folder_rows.set_cursor(Some(row));
            }
        }
    }

    fn selection_before(&self) -> (HashSet<MediaId>, FolderSelectionSnapshot) {
        (
            self.file_selection.selected_ids().clone(),
            self.folder_rows.selection_snapshot(),
        )
    }

    fn delta_since(&self, before: &(HashSet<MediaId>, FolderSelectionSnapshot)) -> SelectionDelta {
        SelectionDelta {
            media: FileSelectionModel::compute_selection_diff(&before.0, self.file_selection.selected_ids()),
            folders_changed: before.1 != self.folder_rows.selection_snapshot(),
        }
    }

    /// The items from `a` to `b` in list order ("..." and empty slots left out); O(distance).
    fn browse_range(&self, a: BrowseItem, b: BrowseItem) -> Vec<BrowseItem> {
        let (Some(pa), Some(pb)) = (self.browse_position_of(a), self.browse_position_of(b)) else {
            return Vec::new();
        };
        (pa.min(pb)..=pa.max(pb))
            .filter_map(|position| self.browse_item_at(position))
            .filter(|item| !matches!(item, BrowseItem::Folder(FolderRowRef::Up)))
            .collect()
    }

    /// A click or a key press on `item`: plain = select only it, Ctrl = toggle it, Shift = the range from the anchor (with Ctrl added
    /// to the selection that is there).
    pub fn browse_click(&mut self, item: BrowseItem, ctrl: bool, shift: bool) -> SelectionDelta {
        let before = self.selection_before();

        if shift {
            let anchor = self.browse_anchor().unwrap_or(item);
            let range = self.browse_range(anchor, item);
            let mut media: HashSet<MediaId> = if ctrl {
                self.file_selection.selected_ids().clone()
            } else {
                HashSet::new()
            };
            if !ctrl {
                self.folder_rows.clear_selection();
            }
            if range.is_empty() {
                // Only "..." lies in the range: it is selected alone.
                if let BrowseItem::Folder(row) = item {
                    self.folder_rows.select_only(row);
                    self.file_selection.set_selection(HashSet::new());
                }
            } else {
                for entry in range {
                    match entry {
                        BrowseItem::Media(id) => {
                            media.insert(id);
                        }
                        BrowseItem::Folder(row) => self.folder_rows.add(row),
                    }
                }
                self.file_selection.set_selection(media);
            }
            self.set_browse_anchor(anchor);
            self.set_browse_cursor(item);
        } else if ctrl {
            match item {
                BrowseItem::Media(id) => {
                    self.file_selection.toggle(id);
                    self.folder_rows.set_anchor(None);
                    self.folder_rows.set_cursor(None);
                    if self.folder_rows.up_selected() {
                        self.folder_rows.clear_selection();
                    }
                }
                BrowseItem::Folder(row) => {
                    self.folder_rows.toggle(row);
                    if row == FolderRowRef::Up {
                        self.file_selection.clear();
                    } else {
                        self.file_selection.set_anchor_id(None);
                        self.file_selection.set_last_clicked_id(None);
                    }
                }
            }
        } else {
            match item {
                BrowseItem::Media(id) => self.select_single_media(id),
                BrowseItem::Folder(row) => {
                    self.file_selection.clear();
                    self.folder_rows.select_only(row);
                }
            }
        }

        self.delta_since(&before)
    }

    /// Select All: every medium of the list and every folder row ("..." excepted).
    pub fn browse_select_all(&mut self, media_ids: &[MediaId]) -> SelectionDelta {
        let before = self.selection_before();
        self.file_selection.select_all(media_ids);
        let folders: HashSet<FolderId> = self.folder_rows.all_folders().into_iter().collect();
        let first_folder = self.folder_rows.all_folders().first().copied();
        self.folder_rows.set_folders(folders);
        if media_ids.is_empty() {
            if let Some(first) = first_folder {
                self.folder_rows.set_anchor(Some(FolderRowRef::Folder(first)));
            }
            if let Some(&last) = self.folder_rows.all_folders().last() {
                self.folder_rows.set_cursor(Some(FolderRowRef::Folder(last)));
            }
        }
        self.delta_since(&before)
    }

    /// Clears the whole selection, folders and media.
    pub fn clear_browse_selection(&mut self) -> SelectionDelta {
        let before = self.selection_before();
        self.file_selection.clear();
        self.folder_rows.clear_selection();
        self.delta_since(&before)
    }
}
