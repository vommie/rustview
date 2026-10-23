//! Clipping of the native video windows around menus and tooltips.

use super::ViewerTabManager;
use crate::viewer::types::TabId;
use crate::viewer::x11::{X11VideoSubwindow, XRectangle};

impl ViewerTabManager {
    /// Hides all video subwindows except for the specified `except_tab_id`.
    pub fn hide_video_subwindows_except(&self, except_tab_id: Option<TabId>) {
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            if Some(tab.id) != except_tab_id {
                tab.deactivate_native();
            }
        }
    }

    /// Sets view-active and interactivity state across all video tabs (used when entering/leaving `TabGrid` mode).
    pub fn set_all_videos_view_active(&self, active: bool) {
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            tab.set_grid_view_active(active);
        }
    }

    // =========================================================================
    // Dynamic XShape Overlay Clipping (Menu & Tooltip Layer Hierarchy)
    // =========================================================================

    /// Updates active context menu cutouts with deduplication to eliminate redundant `XShape` calls.
    pub fn set_menu_cutouts(&self, cutouts: Vec<XRectangle>) {
        {
            let current = self.active_menu_cutouts.read();
            if *current == cutouts {
                return;
            }
        }
        let open = !cutouts.is_empty();
        *self.active_menu_cutouts.write() = cutouts;
        self.apply_combined_cutouts_to_all_subwindows();
        self.apply_menu_open_to_all_subwindows(open);
    }

    /// Clears active context menu cutouts with deduplication.
    pub fn clear_menu_cutouts(&self) {
        {
            let mut writer = self.active_menu_cutouts.write();
            if writer.is_empty() {
                return;
            }
            writer.clear();
        }
        self.apply_combined_cutouts_to_all_subwindows();
        self.apply_menu_open_to_all_subwindows(false);
    }

    /// Whether the video windows let the pointer through (a menu is open) in every tab.
    fn apply_menu_open_to_all_subwindows(&self, open: bool) {
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            tab.set_menu_open(open);
        }
    }

    /// Updates active tooltip cutouts (Z: 200) with deduplication.
    pub fn set_tooltip_cutouts(&self, cutouts: Vec<XRectangle>) {
        {
            let current = self.active_tooltip_cutouts.read();
            if *current == cutouts {
                return;
            }
        }
        *self.active_tooltip_cutouts.write() = cutouts;
        self.apply_combined_cutouts_to_all_subwindows();
    }

    /// Clears active tooltip cutouts with deduplication.
    pub fn clear_tooltip_cutouts(&self) {
        {
            let mut writer = self.active_tooltip_cutouts.write();
            if writer.is_empty() {
                return;
            }
            writer.clear();
        }
        self.apply_combined_cutouts_to_all_subwindows();
    }

    /// Returns all combined cutouts (both menu and tooltip).
    pub fn get_combined_cutouts(&self) -> Vec<XRectangle> {
        let mut combined = self.active_menu_cutouts.read().clone();
        let tooltips = self.active_tooltip_cutouts.read();
        combined.extend_from_slice(&tooltips);
        combined
    }

    /// Re-applies all active cutouts to a specific subwindow (e.g. during scroll or resize).
    pub fn reapply_active_cutouts_to_subwindow(&self, sub: &mut X11VideoSubwindow) {
        let combined = self.get_combined_cutouts();
        if combined.is_empty() {
            sub.reset_clip_shape();
        } else {
            sub.clip_cutout_rectangles(&combined);
        }
    }

    /// Re-applies all combined cutouts (menus + tooltips) across EVERY visible video subwindow on screen.
    pub fn apply_combined_cutouts_to_all_subwindows(&self) {
        let combined = self.get_combined_cutouts();
        let tabs = self.tabs.read();

        for tab in tabs.all() {
            tab.apply_overlay_cutouts(&combined);
        }
    }
}
