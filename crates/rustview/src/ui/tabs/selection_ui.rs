//! The multi-selection of tabs in the UI: clicks with Ctrl/Shift, a click on free space, Esc, and the flags of the tab models.
//!
//! The selection itself is `TabSelection` in `UiState` (pure, unit tested); this module only forwards the clicks and keeps the
//! `is-selected` flag of the tab bar and tab grid models in line with it, row by row, without rebuilding the models.

use super::BridgeViewerTabs;
use crate::ui::MainWindow;
use crate::ui::state::UiState;
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::RwLock;
use rustview_core::util::cast::WrappingAs;
use slint::{ComponentHandle, Model};
use std::sync::Arc;

impl BridgeViewerTabs {
    /// Wires the clicks that select tabs: Ctrl/Shift+click in the tab bar and the tab grid, a click on free space, Esc.
    pub(super) fn register_selection_callbacks(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (win, tm, st) = (window.as_weak(), tab_manager.clone(), state.clone());
        window.on_tab_multi_selected(move |tab_id, ctrl, shift| {
            if let Some(win) = win.upgrade() {
                Self::click_tab_for_selection(&win, &tm, &st, tab_id.wrapping_as::<TabId>(), ctrl, shift);
            }
        });

        let (win, tm, st) = (window.as_weak(), tab_manager.clone(), state.clone());
        window.on_tab_grid_item_selection_clicked(move |tab_id, ctrl, shift| {
            if let Some(win) = win.upgrade() {
                Self::click_tab_for_selection(&win, &tm, &st, tab_id.wrapping_as::<TabId>(), ctrl, shift);
            }
        });

        let (win, st) = (window.as_weak(), state.clone());
        window.on_tab_selection_cleared(move || {
            let Some(win) = win.upgrade() else { return };
            let cleared = st.write().tab_selection.clear();
            if cleared {
                tracing::debug!("[BridgeViewerTabs: Selection] The selection of tabs was cleared");
                Self::apply_tab_selection(&win, &st);
            }
        });
    }

    /// A Ctrl and/or Shift click on a tab (or a tab-grid item): the selection changes, the shown tab does not.
    pub fn click_tab_for_selection(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        state: &RwLock<UiState>,
        tab_id: TabId,
        ctrl: bool,
        shift: bool,
    ) {
        let order: Vec<TabId> = tab_manager.list_tabs().iter().map(|t| t.id).collect();
        let selected = {
            let mut st = state.write();
            st.tab_selection.click(&order, tab_id, ctrl, shift);
            st.tab_selection.len()
        };
        tracing::debug!(
            tab_id,
            ctrl,
            shift,
            selected,
            "[BridgeViewerTabs: Selection] Tab clicked for selection"
        );
        Self::apply_tab_selection(window, state);
    }

    /// Brings the `is-selected` flag of every row of the tab bar and the tab grid in line with the selection.
    pub fn apply_tab_selection(window: &MainWindow, state: &RwLock<UiState>) {
        let selection = state.read().tab_selection.clone();
        window.set_has_tab_selection(!selection.is_empty());

        let tabs = window.get_tabs();
        for row in 0..tabs.row_count() {
            if let Some(mut item) = tabs.row_data(row) {
                let wanted = selection.contains(item.id.wrapping_as::<TabId>());
                if item.is_selected != wanted {
                    item.is_selected = wanted;
                    tabs.set_row_data(row, item);
                }
            }
        }

        let items = window.get_tab_grid_items();
        for row in 0..items.row_count() {
            if let Some(mut item) = items.row_data(row) {
                let wanted = selection.contains(item.id.wrapping_as::<TabId>());
                if item.is_selected != wanted {
                    item.is_selected = wanted;
                    items.set_row_data(row, item);
                }
            }
        }
    }
}
