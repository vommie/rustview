//! Media dragged onto the folder rows of the Browse view. The view reports the row under the pointer; this module decides whether
//! it is a valid drop target and shows it. What a drop does is the flow of the folder tree (`BridgeDnd`): the same Copy / Move /
//! Cancel popup, the same conflict handling and the same file operations.

use super::BrowseFolders;
use super::rows::FolderRowRef;
use rustview_core::library::models::FolderId;
use crate::ui::MainWindow;
use crate::ui::bridge_media::BridgeMedia;
use crate::ui::bridge_state::UiState;
use crate::ui::rubberband_selection::{MEDIA_HEADER_BAR_HEIGHT_PX, RubberbandAutoScroller};
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::sync::Arc;

/// Height of the bars above the Browse view in the main window (`main.slint` adds it to every point it reports).
const BROWSE_VIEW_TOP_PX: f32 = 64.0;
/// Thickness of the resizer bar between the sidebar and the Browse view (`resizer_bar.slint`).
const RESIZER_THICKNESS_PX: f32 = 6.0;
/// Height of the status bar below the Browse view.
const STATUS_BAR_HEIGHT_PX: f32 = 28.0;

/// Scrolls the Browse view while a medium is dragged near its top or bottom edge, so that a folder row that is out of sight can be
/// reached; the drop target follows the scroll although the pointer rests.
#[derive(Default, Clone)]
pub struct BrowseDragScroller {
    inner: RubberbandAutoScroller,
}

impl BrowseDragScroller {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stop(&self) {
        self.inner.stop();
    }

    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// The pointer of a medium drag is at (`x`, `y`) in window coordinates.
    pub fn update(&self, window: &MainWindow, state: &Arc<RwLock<UiState>>, x: f32, y: f32) {
        let scale = window.window().scale_factor().max(0.1);
        let window_h = window.window().size().height as f32 / scale;
        let left = window.get_sidebar_left_width() + RESIZER_THICKNESS_PX;
        let right = left + window.get_media_view_width();
        if x < left || x > right {
            self.inner.stop();
            return;
        }
        let top = BROWSE_VIEW_TOP_PX + MEDIA_HEADER_BAR_HEIGHT_PX;
        let bottom = window_h - STATUS_BAR_HEIGHT_PX;

        let weak = window.as_weak();
        let state = state.clone();
        self.inner.update_cursor_in(y, top, bottom, move |delta_y| {
            let Some(window) = weak.upgrade() else { return };
            if !window.get_is_dragging_media() {
                return;
            }
            window.invoke_scroll_media_by(delta_y);
            let row = window.invoke_drag_row_id_at(window.get_drag_x(), window.get_drag_y());
            BrowseFolders::handle_drag_over_row(row, &window, &state);
        });
    }
}

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
        // No drop target while a scan runs (memory only; the drive is checked when the files are dropped).
        let blocked = BridgeMedia::is_scan_running(window, state);
        let row = if !blocked && Self::drop_target(&state.read(), row_id).is_some() { row_id } else { 0 };
        if window.get_drop_target_row_id() != row {
            window.set_drop_target_row_id(row);
        }
    }
}
