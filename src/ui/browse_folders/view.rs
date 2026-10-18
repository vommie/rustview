//! The row of a folder or of the "Up" entry as the grid and the table show it. The data comes from the folder snapshot in memory.

use super::rows::FolderRowRef;
use crate::library::models::FolderHierarchySnapshot;
use crate::ui::MediaItemView;

/// Name of the navigation entry that leads to the parent folder.
pub const UP_LABEL: &str = "Up";

pub fn folder_row_view(
    snapshot: Option<&FolderHierarchySnapshot>,
    row: FolderRowRef,
    position: usize,
    is_selected: bool,
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
                if node.is_deleted {
                    view.flags_str = "Missing".into();
                }
            }
            view.format_str = "Folder".into();
        }
    }
    view
}
