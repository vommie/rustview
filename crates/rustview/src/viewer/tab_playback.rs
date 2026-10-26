//! The Multiview of a tab behind one interface for the tab manager: pause it, stop it, tell it about open menus.

use super::types::MediaTabSession;

impl MediaTabSession {
    /// The tab stops being the shown one: its players are paused (the user's pause state is kept).
    pub fn deactivate(&self) {
        self.multiview.instance.lock().deactivate();
    }

    /// A menu of the application opens or closes: no bar shows over the tab while it is open.
    pub fn set_menu_open(&self, open: bool) {
        self.multiview.instance.lock().set_menu_open(open);
    }

    /// The tab is closed: playback stops. Memory is released by the last owner of the instance.
    pub fn stop(&self) {
        self.multiview.instance.lock().stop_and_hide();
    }
}
