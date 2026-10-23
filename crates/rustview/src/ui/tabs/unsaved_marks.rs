//! The dot of a tab (and its tab-grid item) that holds pictures with unsaved edits. The edits count up an epoch whenever a
//! picture becomes dirty or clean (`viewer::edit::unsaved_epoch`); the rows are only looked at when it moved.

use crate::ui::MainWindow;
use crate::viewer::{TabId, ViewerTabManager};
use rustview_core::util::cast::WrappingAs;
use slint::Model;
use std::cell::Cell;

thread_local! {
    static SEEN: Cell<u64> = const { Cell::new(0) };
}

/// Brings the dots up to date when the edits changed since the last look.
pub fn sync_if_changed(window: &MainWindow, tab_manager: &ViewerTabManager) {
    let epoch = crate::viewer::edit::unsaved_epoch();
    if SEEN.with(|s| s.replace(epoch)) == epoch {
        return;
    }
    let unsaved = |id: i32| {
        tab_manager
            .get_tab(id.wrapping_as::<TabId>())
            .is_some_and(|tab| tab.multiview.instance.lock().has_unsaved_edits())
    };
    let tabs = window.get_tabs();
    for row in 0..tabs.row_count() {
        if let Some(mut item) = tabs.row_data(row) {
            let wanted = unsaved(item.id);
            if item.has_unsaved != wanted {
                item.has_unsaved = wanted;
                tabs.set_row_data(row, item);
            }
        }
    }
    let items = window.get_tab_grid_items();
    for row in 0..items.row_count() {
        if let Some(mut item) = items.row_data(row) {
            let wanted = unsaved(item.id);
            if item.has_unsaved != wanted {
                item.has_unsaved = wanted;
                items.set_row_data(row, item);
            }
        }
    }
}
