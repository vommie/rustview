//! The row of a folder or of the "Up" entry as the grid and the table show it. The data comes from the folder snapshot in memory.

use super::rows::FolderRowRef;
use super::times::{FolderStamp, FolderTimes};
use crate::ui::MediaItemView;
use crate::ui::models::format_timestamp;
use rustview_core::library::models::{CachedFolderNode, FolderHierarchySnapshot};
use slint::SharedString;
use std::sync::Arc;

/// Name of the navigation entry that leads to the parent folder.
pub const UP_LABEL: &str = "...";

/// The date strings of a folder (empty where unknown).
pub fn stamp_labels(stamp: Option<FolderStamp>) -> (SharedString, SharedString) {
    let label = |secs: Option<i64>| {
        secs.map(|s| SharedString::from(format_timestamp(s)))
            .unwrap_or_default()
    };
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
            .map(|s| {
                node.children
                    .iter()
                    .filter(|id| s.get_node(**id).is_some_and(|c| c.is_deleted))
                    .count()
            })
            .unwrap_or(0);
        Self {
            missing: node.is_deleted,
            hidden: node.is_hidden,
            empty: !node.is_deleted
                && node.recursive_media_count == 0
                && node.subfolder_count == 0
                && missing_children == 0,
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
                view.path_str = node
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into();
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
