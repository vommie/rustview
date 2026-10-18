//! Folders in the Browse view: the optional rows in front of the media (grid and table), the navigation entry "Up", their clicks and
//! the "Show Folders" toggle, and the "New Folder" button state. The row data is `rows` (pure), `view` builds the Slint rows.

mod actions;
pub mod rows;
pub mod view;

pub use rows::{BrowseFolderRows, FolderRowRef, FolderRowsRequest, Slot, UP_ROW_ID};
pub use view::folder_row_view;

use crate::library::Library;
use crate::library::models::FolderHierarchySnapshot;
use crate::settings::schema::FolderSettings;
use crate::ui::bridge_folders::BridgeFolders;
use crate::ui::bridge_state::UiState;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct BrowseFolders;

impl BrowseFolders {
    /// The snapshot the folder rows are built from; only loaded while folder rows are wanted (no cost otherwise).
    pub fn snapshot_if_wanted(library: &Library, state: &RwLock<UiState>) -> Option<Arc<FolderHierarchySnapshot>> {
        let wanted = {
            let st = state.read();
            st.show_folders && st.is_plain_folder_view()
        };
        wanted.then(|| BridgeFolders::ensure_snapshot(library, state))
    }

    /// Brings the folder rows of `st` in line with the snapshot (`None` = no folder rows wanted). Returns whether they changed.
    pub fn rebuild_rows(st: &mut UiState, snapshot: Option<&FolderHierarchySnapshot>, folders: &FolderSettings) -> bool {
        match snapshot {
            Some(snapshot) => st.rebuild_folder_rows(snapshot, folders.show_hidden_folders, folders.show_deleted_folders),
            None => st.rebuild_folder_rows(&FolderHierarchySnapshot::default(), folders.show_hidden_folders, folders.show_deleted_folders),
        }
    }
}
