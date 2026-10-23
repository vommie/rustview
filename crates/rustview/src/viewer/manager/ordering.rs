//! Moving and cycling tabs.

use super::ViewerTabManager;
use crate::viewer::types::TabId;

impl ViewerTabManager {
    #[inline]
    pub fn move_tab_to_start(&self, tab_id: TabId) -> bool {
        self.move_tab(tab_id, 0)
    }

    #[inline]
    pub fn move_to_start(&self, tab_id: TabId) -> bool {
        self.move_tab_to_start(tab_id)
    }

    pub fn move_tab_left(&self, tab_id: TabId) -> bool {
        let Some(pos) = self.get_tab_index(tab_id) else {
            return false;
        };
        if pos > 0 { self.move_tab(tab_id, pos - 1) } else { false }
    }

    #[inline]
    pub fn move_left(&self, tab_id: TabId) -> bool {
        self.move_tab_left(tab_id)
    }

    pub fn move_tab_right(&self, tab_id: TabId) -> bool {
        let Some(pos) = self.get_tab_index(tab_id) else {
            return false;
        };
        let count = self.tab_count();
        if pos + 1 < count {
            self.move_tab(tab_id, pos + 1)
        } else {
            false
        }
    }

    #[inline]
    pub fn move_right(&self, tab_id: TabId) -> bool {
        self.move_tab_right(tab_id)
    }

    pub fn move_tab_to_end(&self, tab_id: TabId) -> bool {
        let count = self.sessions_count();
        if count > 0 {
            self.move_tab(tab_id, count - 1)
        } else {
            false
        }
    }

    #[inline]
    pub fn move_to_end(&self, tab_id: TabId) -> bool {
        self.move_tab_to_end(tab_id)
    }

    /// Cycles the active tab forward (`is_next = true`) or backward (`is_next = false`) with cyclic wrapping.
    pub fn cycle_tab(&self, is_next: bool) -> Option<TabId> {
        let tabs = self.tabs.read();
        let total = tabs.len();
        if total <= 1 {
            return None;
        }

        let current_active = *self.active_tab_id.read();
        let current_idx = current_active.and_then(|id| tabs.index_of(id));

        let target_idx = match current_idx {
            Some(idx) => {
                if is_next {
                    (idx + 1) % total
                } else {
                    (idx + total - 1) % total
                }
            }
            None => {
                if is_next {
                    0
                } else {
                    total - 1
                }
            }
        };

        let target_tab = tabs.get_at(target_idx)?;
        let target_id = target_tab.id;
        drop(tabs);

        if self.set_active_tab_internal(target_id, false) {
            Some(target_id)
        } else {
            None
        }
    }
}
