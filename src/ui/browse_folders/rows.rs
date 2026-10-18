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
