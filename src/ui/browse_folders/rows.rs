//! The folder rows of the Browse view (pure): which subfolders are listed in front of the media, which ones were just created
//! and wait at the end, and how a list position maps to a folder, a medium or nothing.
//!
//! Positions are positions of the whole list (what the tile pool and the Slint views use): `[Up] [folders...] [media...] [new folders...]`.
//! The media keep their own positions (`media_row_map`, `active_indices`); only the folder rows in front shift them by `front_len()`.

use crate::library::models::{FolderHierarchySnapshot, FolderId};

/// `MediaItemView::id` of the "Up" entry; folders use the negated folder id, media ids are never negative.
pub const UP_ROW_ID: i32 = i32::MIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRowRef {
    Up,
    Folder(FolderId),
}

impl FolderRowRef {
    pub fn view_id(self) -> i32 {
        match self {
            Self::Up => UP_ROW_ID,
            Self::Folder(id) => -(id.min(i32::MAX as u64) as i32),
        }
    }

    /// `None` for the id of a medium.
    pub fn from_view_id(id: i32) -> Option<Self> {
        match id {
            UP_ROW_ID => Some(Self::Up),
            id if id < 0 => Some(Self::Folder(u64::from(id.unsigned_abs()))),
            _ => None,
        }
    }
}

/// What sits at a list position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Folder(FolderRowRef),
    /// The medium at this position of the media list (`active_indices`).
    Media(usize),
    Empty,
}

/// What decides whether and which folders are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderRowsRequest {
    /// "Show Folders" is on and the view is a plain folder (no query, no recursive view).
    pub enabled: bool,
    pub current: Option<FolderId>,
    pub show_hidden: bool,
    pub show_deleted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowseFolderRows {
    /// Parent of the open folder, `Some` when the "Up" entry exists (it never exists at the tree's root).
    up: Option<FolderId>,
    /// The subfolders in name order.
    front: Vec<FolderId>,
    /// Folders created while the view was open: after the media, so that nothing shifts, until the folder is opened again.
    tail: Vec<FolderId>,
    selected: Option<FolderRowRef>,
    /// A folder was appended and the list was not rebuilt since: the next `rebuild` reports a change although the rows may equal
    /// what was there (the appended folder is already part of `tail`), so the view is refilled.
    dirty: bool,
}

impl BrowseFolderRows {
    /// Rows in front of the media (the "Up" entry and the folders).
    #[inline]
    pub fn front_len(&self) -> usize {
        usize::from(self.up.is_some()) + self.front.len()
    }

    #[inline]
    pub fn tail_len(&self) -> usize {
        self.tail.len()
    }

    /// Number of rows that are folders (the "Up" entry included).
    #[inline]
    pub fn folder_rows(&self) -> usize {
        self.front_len() + self.tail.len()
    }

    #[inline]
    pub fn up_target(&self) -> Option<FolderId> {
        self.up
    }

    #[inline]
    pub fn selected(&self) -> Option<FolderRowRef> {
        self.selected
    }

    /// Length of the whole list for `media_len` media.
    #[inline]
    pub fn view_len(&self, media_len: usize) -> usize {
        self.front_len() + media_len + self.tail.len()
    }

    pub fn slot_at(&self, position: usize, media_len: usize) -> Slot {
        let front = self.front_len();
        if position < front {
            let has_up = self.up.is_some();
            return match (has_up, position) {
                (true, 0) => Slot::Folder(FolderRowRef::Up),
                (true, p) => Slot::Folder(FolderRowRef::Folder(self.front[p - 1])),
                (false, p) => Slot::Folder(FolderRowRef::Folder(self.front[p])),
            };
        }
        let after_front = position - front;
        if after_front < media_len {
            return Slot::Media(after_front);
        }
        self.tail
            .get(after_front - media_len)
            .map_or(Slot::Empty, |&id| Slot::Folder(FolderRowRef::Folder(id)))
    }

    /// The list position of a folder row (linear in the number of folders, used on clicks only).
    pub fn position_of(&self, row: FolderRowRef, media_len: usize) -> Option<usize> {
        let has_up = usize::from(self.up.is_some());
        match row {
            FolderRowRef::Up => self.up.map(|_| 0),
            FolderRowRef::Folder(id) => self
                .front
                .iter()
                .position(|&f| f == id)
                .map(|i| i + has_up)
                .or_else(|| self.tail.iter().position(|&f| f == id).map(|i| self.front_len() + media_len + i)),
        }
    }

    pub fn select(&mut self, row: Option<FolderRowRef>) {
        self.selected = row;
    }

    /// Forgets the folders that were appended: they take their place in name order (the folder was opened again, or the folders
    /// were shown anew).
    pub fn reset_tail(&mut self) {
        self.tail.clear();
    }

    /// Appends a created folder after the media. Ignored when the folder is listed already.
    pub fn append_created(&mut self, id: FolderId) -> bool {
        if self.front.contains(&id) || self.tail.contains(&id) {
            return false;
        }
        self.tail.push(id);
        self.dirty = true;
        true
    }

    /// Recomputes the rows from the snapshot (memory only). Appended folders stay appended while they still exist. Returns
    /// whether anything changed.
    pub fn rebuild(&mut self, snapshot: &FolderHierarchySnapshot, request: FolderRowsRequest) -> bool {
        let previous = self.clone();
        *self = self.computed(snapshot, request);
        *self != previous
    }

    fn computed(&self, snapshot: &FolderHierarchySnapshot, request: FolderRowsRequest) -> Self {
        let Some(node) = request.current.filter(|_| request.enabled).and_then(|id| snapshot.get_node(id)) else {
            return Self::default();
        };

        let up = node
            .parent_id
            .filter(|parent| !snapshot.root_ids.contains(&node.id) && snapshot.nodes.contains_key(parent));

        let listed = |id: &FolderId| {
            snapshot
                .get_node(*id)
                .is_some_and(|child| (request.show_hidden || !child.is_hidden) && (request.show_deleted || !child.is_deleted))
        };
        let tail: Vec<FolderId> = self.tail.iter().copied().filter(|id| listed(id) && node.children.contains(id)).collect();
        let front: Vec<FolderId> = node
            .children
            .iter()
            .copied()
            .filter(|id| listed(id) && !tail.contains(id))
            .collect();

        let mut rows = Self { up, front, tail, selected: None, dirty: false };
        rows.selected = self.selected.filter(|row| rows.position_of(*row, 0).is_some());
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::models::CachedFolderNode;
    use std::path::PathBuf;

    fn node(id: FolderId, parent: Option<FolderId>, name: &str, hidden: bool, children: Vec<FolderId>) -> CachedFolderNode {
        CachedFolderNode {
            id,
            parent_id: parent,
            name: name.into(),
            path: PathBuf::from(format!("/r/{name}")),
            depth: 0,
            is_hidden: hidden,
            is_deleted: false,
            subfolder_count: children.len() as u32,
            media_count: 0,
            recursive_media_count: 0,
            children,
        }
    }

    fn snapshot() -> FolderHierarchySnapshot {
        let mut s = FolderHierarchySnapshot::default();
        for n in [
            node(1, None, "root", false, vec![2, 3, 4]),
            node(2, Some(1), "a", false, vec![5]),
            node(3, Some(1), "b", true, vec![]),
            node(4, Some(1), "c", false, vec![]),
            node(5, Some(2), "a1", false, vec![]),
        ] {
            s.nodes.insert(n.id, n);
        }
        s.root_ids = vec![1];
        s
    }

    fn request(current: u64, hidden: bool) -> FolderRowsRequest {
        FolderRowsRequest { enabled: true, current: Some(current), show_hidden: hidden, show_deleted: true }
    }

    #[test]
    fn root_has_no_up_and_hides_hidden_folders() {
        let mut rows = BrowseFolderRows::default();
        assert!(rows.rebuild(&snapshot(), request(1, false)));
        assert_eq!((rows.front_len(), rows.up_target()), (2, None));
        assert!(rows.rebuild(&snapshot(), request(1, true)));
        assert_eq!(rows.front_len(), 3);
    }

    #[test]
    fn subfolder_starts_with_up_and_slots_follow_the_layout() {
        let mut rows = BrowseFolderRows::default();
        rows.rebuild(&snapshot(), request(2, false));
        assert_eq!(rows.up_target(), Some(1));
        assert_eq!(rows.slot_at(0, 3), Slot::Folder(FolderRowRef::Up));
        assert_eq!(rows.slot_at(1, 3), Slot::Folder(FolderRowRef::Folder(5)));
        assert_eq!(rows.slot_at(2, 3), Slot::Media(0));
        assert_eq!(rows.slot_at(4, 3), Slot::Media(2));
        assert_eq!(rows.slot_at(5, 3), Slot::Empty);
    }

    #[test]
    fn created_folders_wait_at_the_end_until_reset() {
        let mut before = snapshot();
        before.nodes.get_mut(&1).unwrap().children = vec![2, 3];
        before.nodes.remove(&4);
        let mut rows = BrowseFolderRows::default();
        rows.rebuild(&before, request(1, false));
        assert_eq!(rows.front_len(), 1);

        // The folder 4 is created: appended first, then the snapshot (which lists it in name order) is rebuilt from.
        assert!(rows.append_created(4));
        assert!(!rows.append_created(4));
        assert!(rows.rebuild(&snapshot(), request(1, false)), "the first rebuild after an append reports it");
        assert!(!rows.rebuild(&snapshot(), request(1, false)), "a rebuild keeps the appended folder");
        assert_eq!((rows.front_len(), rows.tail_len()), (1, 1));
        assert_eq!(rows.slot_at(1 + 3, 3), Slot::Folder(FolderRowRef::Folder(4)));
        assert_eq!(rows.view_len(3), 5);

        rows.reset_tail();
        rows.rebuild(&snapshot(), request(1, false));
        assert_eq!((rows.front_len(), rows.tail_len()), (2, 0));
    }

    #[test]
    fn disabled_or_unknown_folder_yields_nothing() {
        let mut rows = BrowseFolderRows::default();
        rows.rebuild(&snapshot(), request(2, false));
        assert!(rows.rebuild(&snapshot(), FolderRowsRequest { enabled: false, ..request(2, false) }));
        assert_eq!(rows.folder_rows(), 0);
        assert!(!rows.rebuild(&snapshot(), request(99, false)));
    }

    #[test]
    fn view_ids_round_trip() {
        for row in [FolderRowRef::Up, FolderRowRef::Folder(7), FolderRowRef::Folder(123_456)] {
            assert_eq!(FolderRowRef::from_view_id(row.view_id()), Some(row));
        }
        assert_eq!(FolderRowRef::from_view_id(5), None);
        assert_eq!(FolderRowRef::from_view_id(0), None);
    }
}
