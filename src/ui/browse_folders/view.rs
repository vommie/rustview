//! The row of a folder or of the "Up" entry as the grid and the table show it. The data comes from the folder snapshot in memory.

use super::rows::FolderRowRef;
use super::times::{FolderStamp, FolderTimes};
use crate::library::models::{CachedFolderNode, FolderHierarchySnapshot};
use crate::ui::MediaItemView;
use crate::ui::models::format_timestamp;
use slint::SharedString;
use std::sync::Arc;

/// Name of the navigation entry that leads to the parent folder.
pub const UP_LABEL: &str = "...";

/// The date strings of a folder (empty where unknown).
pub fn stamp_labels(stamp: Option<FolderStamp>) -> (SharedString, SharedString) {
    let label = |secs: Option<i64>| secs.map(|s| SharedString::from(format_timestamp(s))).unwrap_or_default();
    stamp.map(|s| (label(s.modified), label(s.created))).unwrap_or_default()
}

/// What the database knows about the state of a folder, for the badges and the Flags column. Memory only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderFlags {
    pub missing: bool,
    pub hidden: bool,
    pub empty: bool,
    /// Direct subfolders that are flagged missing on disk.
    pub missing_children: usize,
}

impl FolderFlags {
    pub fn of(node: &CachedFolderNode, snapshot: Option<&FolderHierarchySnapshot>) -> Self {
        let missing_children = snapshot
            .map(|s| node.children.iter().filter(|id| s.get_node(**id).is_some_and(|c| c.is_deleted)).count())
            .unwrap_or(0);
        Self {
            missing: node.is_deleted,
            hidden: node.is_hidden,
            empty: !node.is_deleted && node.recursive_media_count == 0 && node.subfolder_count == 0 && missing_children == 0,
            missing_children: if node.is_deleted { 0 } else { missing_children },
        }
    }

    /// The text of the Flags column of the table: every state that applies.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.missing {
            parts.push("Missing".to_string());
        }
        if self.hidden {
            parts.push("Hidden".to_string());
        }
        if self.empty {
            parts.push("Empty".to_string());
        }
        if self.missing_children > 0 {
            parts.push(format!("{} missing", self.missing_children));
        }
        parts.join(", ")
    }
}

pub fn folder_row_view(
    snapshot: Option<&FolderHierarchySnapshot>,
    row: FolderRowRef,
    position: usize,
    is_selected: bool,
    times: &Arc<FolderTimes>,
) -> MediaItemView {
    let mut view = MediaItemView {
        id: row.view_id(),
        index: position as i32,
        is_selected,
        is_folder: true,
        is_folder_up: row == FolderRowRef::Up,
        ..Default::default()
    };

    match row {
        FolderRowRef::Up => view.filename = UP_LABEL.into(),
        FolderRowRef::Folder(id) => {
            if let Some(node) = snapshot.and_then(|s| s.get_node(id)) {
                view.filename = node.name.as_str().into();
                view.path_str = node.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default().into();
                view.is_deleted = node.is_deleted;
                let flags = FolderFlags::of(node, snapshot);
                view.folder_hidden = flags.hidden;
                view.folder_empty = flags.empty;
                view.folder_missing_children = flags.missing_children as i32;
                view.flags_str = flags.label().into();
                if !node.is_deleted {
                    (view.modified_date_str, view.created_date_str) = stamp_labels(times.get(id, &node.path));
                }
            }
            view.format_str = "Folder".into();
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn node(id: u64, deleted: bool, hidden: bool, subfolders: u32, media: u32, children: Vec<u64>) -> CachedFolderNode {
        CachedFolderNode {
            id,
            parent_id: None,
            name: format!("n{id}"),
            path: PathBuf::from(format!("/m/n{id}")),
            depth: 1,
            is_hidden: hidden,
            is_deleted: deleted,
            subfolder_count: subfolders,
            media_count: media,
            recursive_media_count: media,
            children,
        }
    }

    fn snapshot(nodes: Vec<CachedFolderNode>) -> FolderHierarchySnapshot {
        let mut s = FolderHierarchySnapshot::default();
        for n in nodes {
            s.nodes.insert(n.id, n);
        }
        s
    }

    #[test]
    fn plain_folder_has_no_flags() {
        let n = node(1, false, false, 1, 4, vec![]);
        assert_eq!(FolderFlags::of(&n, None), FolderFlags::default());
        assert_eq!(FolderFlags::default().label(), "");
    }

    #[test]
    fn states_come_from_the_node() {
        let flags = FolderFlags::of(&node(1, true, true, 0, 0, vec![]), None);
        assert!(flags.missing && flags.hidden && !flags.empty, "a missing folder is not called empty");
        assert_eq!(flags.label(), "Missing, Hidden");
        let empty = FolderFlags::of(&node(2, false, false, 0, 0, vec![]), None);
        assert_eq!(empty.label(), "Empty");
    }

    #[test]
    fn missing_children_are_counted_among_the_direct_subfolders_only() {
        let parent = node(1, false, false, 1, 0, vec![2, 3, 4]);
        let snap = snapshot(vec![parent.clone(), node(2, true, false, 0, 0, vec![]), node(3, true, false, 0, 0, vec![]), node(4, false, false, 0, 1, vec![])]);
        let flags = FolderFlags::of(&parent, Some(&snap));
        assert_eq!(flags.missing_children, 2);
        assert!(!flags.empty, "missing children keep the folder from being empty");
        assert_eq!(flags.label(), "2 missing");
    }

    #[test]
    fn a_missing_folder_does_not_report_missing_children() {
        let parent = node(1, true, false, 1, 0, vec![2]);
        let snap = snapshot(vec![parent.clone(), node(2, true, false, 0, 0, vec![])]);
        assert_eq!(FolderFlags::of(&parent, Some(&snap)).missing_children, 0);
    }

    #[test]
    fn the_row_view_carries_the_flags() {
        let parent = node(1, false, true, 0, 0, vec![2]);
        let snap = snapshot(vec![parent, node(2, true, false, 0, 0, vec![])]);
        let times = FolderTimes::new();
        let view = folder_row_view(Some(&snap), FolderRowRef::Folder(1), 0, false, &times);
        assert!(view.folder_hidden && !view.folder_empty);
        assert_eq!(view.folder_missing_children, 1);
        assert_eq!(view.flags_str, "Hidden, 1 missing");
        let up = folder_row_view(Some(&snap), FolderRowRef::Up, 0, false, &times);
        assert!(up.flags_str.is_empty() && !up.folder_hidden);
    }
}
