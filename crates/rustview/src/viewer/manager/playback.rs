//! Which tabs play: only the shown tab's Multiview plays, every other one is paused (the user's pause state is kept), and an open
//! context menu of the main window keeps the bars of every tab away.

use super::ViewerTabManager;
use crate::viewer::types::TabId;
use std::sync::atomic::Ordering;

impl ViewerTabManager {
    /// Pauses the Multiview of every tab except `except_tab_id`.
    pub fn deactivate_tabs_except(&self, except_tab_id: Option<TabId>) {
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            if Some(tab.id) != except_tab_id {
                tab.deactivate();
            }
        }
    }

    /// Pauses the Multiview of every tab (the Browse view or the tab grid took over; whoever shows a tab next places it again).
    pub fn deactivate_all_tabs(&self) {
        self.deactivate_tabs_except(None);
    }

    /// A context menu of the main window opened or closed: no bar shows over any tab meanwhile.
    pub fn set_menu_open(&self, open: bool) {
        if self.menu_open.swap(open, Ordering::AcqRel) == open {
            return;
        }
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            tab.set_menu_open(open);
        }
    }
}
