//! Native windows of a tab behind one interface: the Multiview of a tab owns one video window per video tile and none for images.
//! The tab manager asks the tab instead of knowing what it owns.

use super::types::MediaTabSession;
use super::x11::XRectangle;

impl MediaTabSession {
    /// The tab stops being the shown one: its windows are hidden and its players paused (the user's pause state is kept).
    pub fn deactivate_native(&self) {
        self.multiview.instance.lock().deactivate();
    }

    /// The tab grid is entered (`active == false`) or left (`active == true`): a Multiview is hidden either way and placed again by
    /// whoever shows it next.
    pub fn set_grid_view_active(&self, _active: bool) {
        self.multiview.instance.lock().deactivate();
    }

    /// Cuts the areas of open menus and tooltips out of every visible window of the tab so they stay on top of the video.
    pub fn apply_overlay_cutouts(&self, cutouts: &[XRectangle]) {
        self.multiview.instance.lock().set_overlay_cutouts(cutouts);
    }

    /// A menu of the application opens or closes: the video windows of the tab take no pointer input while it is open.
    pub fn set_menu_open(&self, open: bool) {
        self.multiview.instance.lock().set_menu_open(open);
    }

    /// The tab is closed: playback stops and the windows disappear. Memory is released by the last owner of the instance.
    pub fn stop_native(&self) {
        self.multiview.instance.lock().stop_and_hide();
    }
}
