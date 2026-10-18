//! Media dragged onto the folder rows of the Browse view. The view reports the row under the pointer; this module decides whether
//! it is a valid drop target and shows it. What a drop does is the flow of the folder tree (`BridgeDnd`): the same Copy / Move /
//! Cancel popup, the same conflict handling and the same file operations.

use super::BrowseFolders;
use super::rows::FolderRowRef;
use crate::library::models::FolderId;
use crate::ui::MainWindow;
use crate::ui::bridge_state::UiState;
use parking_lot::RwLock;

impl BrowseFolders {
    /// The folder a medium dropped onto the folder row `row_id` goes into: the folder itself, the parent for the "..." row.
    /// `None` for no row, for a "..." row without parent and for a folder that is missing on disk.
    pub fn drop_target(st: &UiState, row_id: i32) -> Option<FolderId> {
        let folder = match FolderRowRef::from_view_id(row_id)? {
            FolderRowRef::Up => st.folder_rows.up_target()?,
            FolderRowRef::Folder(id) => id,
        };
        let node = st.folder_hierarchy.as_ref()?.get_node(folder)?;
        (!node.is_deleted).then_some(folder)
    }

    /// The pointer of a drag moved over (or off) a folder row: the row lights up when a drop there would work.
    pub fn handle_drag_over_row(row_id: i32, window: &MainWindow, state: &RwLock<UiState>) {
        if !window.get_is_dragging_media() {
            return;
        }
        let row = if Self::drop_target(&state.read(), row_id).is_some() { row_id } else { 0 };
        if window.get_drop_target_row_id() != row {
            window.set_drop_target_row_id(row);
        }
    }
}
