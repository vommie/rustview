//! The keys of the tab grid (Settings > Hotkeys > Tab-Grid): the arrow keys move a mark from item to item (it looks like the
//! hover frame), the other keys act on the marked item through the same callbacks as the mouse.

use crate::ui::MainWindow;
use crate::ui::state::UiState;
use crate::ui::tabs::grid::mark::{self, GridKey};
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::RwLock;
use rustview_core::util::cast::WrappingAs;
use slint::Model;
use std::sync::Arc;

/// Runs `key` in the grid. Returns whether it was used (a key without a mark to act on goes on).
pub fn handle(window: &MainWindow, tab_manager: &Arc<ViewerTabManager>, state: &RwLock<UiState>, key: GridKey) -> bool {
    let ids: Vec<TabId> = tab_manager.list_tabs().iter().map(|tab| tab.id).collect();
    if ids.is_empty() {
        return false;
    }
    let (marked, rects) = {
        let st = state.read();
        (
            st.tab_grid_mark.filter(|id| ids.contains(id)),
            st.cached_tab_grid_rects.clone(),
        )
    };
    match key {
        GridKey::Move(step) => {
            // The first arrow key shows the mark on the tab that was shown last (else the first item) without moving it.
            let shown = tab_manager.get_active_tab().map(|tab| tab.id);
            let next = match marked.and_then(|id| ids.iter().position(|i| *i == id)) {
                Some(from) => mark::neighbor(&rects, from, step).unwrap_or(from),
                None => ids.iter().position(|id| Some(*id) == shown).unwrap_or(0),
            };
            set_mark(window, state, ids.get(next).copied());
            if let Some(rect) = rects.get(next) {
                window.invoke_tab_grid_reveal(rect.y, rect.height);
            }
            true
        }
        GridKey::Open => marked.is_some_and(|id| {
            tracing::debug!(tab_id = id, "[TabGrid: Keys] Open the marked item");
            window.invoke_tab_grid_item_left_clicked(id.wrapping_as::<i32>());
            true
        }),
        GridKey::Fullscreen => marked.is_some_and(|id| {
            tracing::debug!(tab_id = id, "[TabGrid: Keys] Show the marked item in the fullscreen");
            window.invoke_tab_grid_item_middle_clicked(id.wrapping_as::<i32>());
            true
        }),
        GridKey::Close => marked.is_some_and(|id| {
            let position = ids.iter().position(|i| *i == id).unwrap_or(0);
            // The mark goes to the item that takes the place of the closed one (or the one before it at the end).
            let after = ids
                .get(position + 1)
                .or_else(|| position.checked_sub(1).and_then(|p| ids.get(p)))
                .copied();
            tracing::debug!(tab_id = id, "[TabGrid: Keys] Close the tab of the marked item");
            window.invoke_tab_grid_item_close_clicked(id.wrapping_as::<i32>());
            // A close that waits for the unsaved-changes question keeps the mark where it is.
            if tab_manager.get_tab(id).is_none() {
                set_mark(window, state, after);
            }
            true
        }),
    }
}

/// Moves the mark to `tab` (`None`: no mark) and updates the items that change.
pub fn set_mark(window: &MainWindow, state: &RwLock<UiState>, tab: Option<TabId>) {
    state.write().tab_grid_mark = tab;
    let items = window.get_tab_grid_items();
    for row in 0..items.row_count() {
        if let Some(mut item) = items.row_data(row) {
            let wanted = tab == Some(item.id.wrapping_as::<TabId>());
            if item.is_marked != wanted {
                item.is_marked = wanted;
                items.set_row_data(row, item);
            }
        }
    }
}
