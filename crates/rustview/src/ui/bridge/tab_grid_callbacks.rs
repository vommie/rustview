//! Callbacks of the tab grid (masonry view of the open tabs). Every callback holds the main window weakly: a strong handle in
//! a callback of the window itself would keep the window alive through its own callbacks.

use super::UiBridge;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use rustview_core::util::cast::{SaturatingAs, WrappingAs};

use slint::ComponentHandle;

impl UiBridge {
    pub(super) fn register_tab_grid_callbacks(&self) {
        let win_grid = self.window.as_weak();
        let tm_grid = self.tab_manager.clone();
        let lib_grid = self.library.clone();
        let set_grid = self.settings.clone();
        let st_grid = self.state.clone();
        let fs_grid = self.fullscreen.clone();
        self.window.on_tab_grid_clicked(move || {
            let Some(window) = win_grid.upgrade() else {
                return;
            };
            tracing::info!("[UiBridge: TabGrid] Tab-Grid button clicked. Switching to Masonry Tab Grid view");
            // The keyboard mark starts hidden every time the grid opens.
            st_grid.write().tab_grid_mark = None;
            tm_grid.deactivate_all_tabs();
            window.set_active_view_mode(2);
            window.set_is_browse_active(false);
            window.set_is_tab_grid_active(true);
            BridgeViewerTabs::sync_ui(&window, &tm_grid, &lib_grid, &set_grid, &st_grid);
            BridgeViewerTabs::sync_video_embedding(&window, &tm_grid, &fs_grid, &set_grid);
            BridgeTabGrid::sync_tab_grid_ui(&window, &tm_grid, &set_grid, &st_grid);
            StatusBarBridge::update_metrics(&window, &st_grid);
        });

        let win_key = self.window.as_weak();
        let tm_key = self.tab_manager.clone();
        let st_key = self.state.clone();
        self.window.on_tab_grid_key(move |code| {
            let (Some(window), Some(key)) = (win_key.upgrade(), crate::ui::tabs::grid::mark::GridKey::from_code(code))
            else {
                return false;
            };
            crate::ui::tabs::grid_keys::handle(&window, &tm_key, &st_key, key)
        });

        let win_left = self.window.as_weak();
        let tm_left = self.tab_manager.clone();
        let lib_left = self.library.clone();
        let set_left = self.settings.clone();
        let st_left = self.state.clone();
        let fs_left = self.fullscreen.clone();
        self.window.on_tab_grid_item_left_clicked(move |tab_id| {
            if let Some(window) = win_left.upgrade() {
                BridgeTabGrid::handle_item_left_clicked(
                    tab_id.wrapping_as::<u64>(),
                    &window,
                    &tm_left,
                    &lib_left,
                    &set_left,
                    &st_left,
                    &fs_left,
                );
            }
        });

        let win_full = self.window.as_weak();
        let tm_full = self.tab_manager.clone();
        let set_full = self.settings.clone();
        let fs_full = self.fullscreen.clone();
        self.window.on_open_tab_fullscreen(move |tab_id| {
            if let Some(window) = win_full.upgrade() {
                BridgeTabGrid::open_tab_fullscreen(tab_id.wrapping_as::<u64>(), &window, &tm_full, &set_full, &fs_full);
            }
        });

        let win_close = self.window.as_weak();
        let tm_close = self.tab_manager.clone();
        let lib_close = self.library.clone();
        let set_close = self.settings.clone();
        let st_close = self.state.clone();
        let fs_close = self.fullscreen.clone();
        self.window.on_tab_grid_item_close_clicked(move |tab_id| {
            let Some(window) = win_close.upgrade() else {
                return;
            };
            if crate::ui::image_edit::close_guard::defer_tab_close(&fs_close, vec![tab_id.wrapping_as::<u64>()]) {
                return;
            }
            BridgeTabGrid::handle_item_close_clicked(
                tab_id.wrapping_as::<u64>(),
                &window,
                &tm_close,
                &lib_close,
                &set_close,
                &st_close,
            );
        });

        let win_cols = self.window.as_weak();
        let tm_cols = self.tab_manager.clone();
        let set_cols = self.settings.clone();
        let st_cols = self.state.clone();
        self.window.on_tab_grid_columns_changed(move |new_cols| {
            let Some(window) = win_cols.upgrade() else {
                return;
            };
            let cols = new_cols.saturating_as::<usize>().clamp(1, 10);
            {
                let mut st = st_cols.write();
                st.tab_grid_columns = cols;
                st.tab_grid_auto_columns = false;
            }
            {
                let mut app_settings = set_cols.get();
                app_settings.view.tab_grid_columns = cols;
                app_settings.view.tab_grid_auto_columns = false;
                set_cols.save_quiet_async(&app_settings);
            }
            BridgeTabGrid::sync_tab_grid_ui(&window, &tm_cols, &set_cols, &st_cols);
        });

        let win_auto = self.window.as_weak();
        let tm_auto = self.tab_manager.clone();
        let set_auto = self.settings.clone();
        let st_auto = self.state.clone();
        self.window.on_tab_grid_auto_columns_toggled(move |is_auto| {
            let Some(window) = win_auto.upgrade() else {
                return;
            };
            st_auto.write().tab_grid_auto_columns = is_auto;
            {
                let mut app_settings = set_auto.get();
                app_settings.view.tab_grid_auto_columns = is_auto;
                set_auto.save_quiet_async(&app_settings);
            }
            BridgeTabGrid::sync_tab_grid_ui(&window, &tm_auto, &set_auto, &st_auto);
        });

        let win_close_all = self.window.as_weak();
        let tm_close_all = self.tab_manager.clone();
        let lib_close_all = self.library.clone();
        let set_close_all = self.settings.clone();
        let st_close_all = self.state.clone();
        let fs_close_all = self.fullscreen.clone();
        self.window.on_tab_grid_close_all_clicked(move || {
            let Some(window) = win_close_all.upgrade() else {
                return;
            };
            tracing::info!("[UiBridge: TabGrid] Close all tabs clicked from TabGrid toolbar");
            let all: Vec<u64> = tm_close_all.list_tabs().iter().map(|t| t.id).collect();
            if crate::ui::image_edit::close_guard::defer_tab_close(&fs_close_all, all) {
                return;
            }
            tm_close_all.close_all_tabs();
            BridgeViewerTabs::sync_ui(&window, &tm_close_all, &lib_close_all, &set_close_all, &st_close_all);
            BridgeTabGrid::sync_tab_grid_ui(&window, &tm_close_all, &set_close_all, &st_close_all);
        });

        let win_geom = self.window.as_weak();
        let tm_geom = self.tab_manager.clone();
        let set_geom = self.settings.clone();
        let st_geom = self.state.clone();
        self.window.on_tab_grid_viewport_geometry_changed(move |_vw, _vh| {
            if let Some(window) = win_geom.upgrade()
                && window.get_active_view_mode() == 2
            {
                BridgeTabGrid::sync_tab_grid_ui(&window, &tm_geom, &set_geom, &st_geom);
            }
        });

        let win_scroll = self.window.as_weak();
        let tm_scroll = self.tab_manager.clone();
        self.window.on_tab_grid_scrolled(move |_dx, _dy| {
            if let Some(window) = win_scroll.upgrade() {
                BridgeTabGrid::handle_scrolled(&window, &tm_scroll);
            }
        });
    }
}
