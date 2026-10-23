//! Callbacks of the tab grid (masonry view of the open tabs).

use super::UiBridge;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use slint::ComponentHandle;

impl UiBridge {
    pub(super) fn register_tab_grid_callbacks(&self) {
        let win_grid = self.window.clone_strong();
        let tm_grid = self.tab_manager.clone();
        let lib_grid = self.library.clone();
        let set_grid = self.settings.clone();
        let st_grid = self.state.clone();
        let fs_grid = self.fullscreen.clone();

        self.window.on_tab_grid_clicked(move || {
            tracing::info!("[UiBridge: TabGrid] Tab-Grid button clicked. Switching to Masonry Tab Grid view");
            tm_grid.set_all_videos_view_active(false);
            win_grid.set_active_view_mode(2);
            win_grid.set_is_browse_active(false);
            win_grid.set_is_tab_grid_active(true);
            BridgeViewerTabs::sync_ui(&win_grid, &tm_grid, &lib_grid, &set_grid, &st_grid);
            BridgeViewerTabs::sync_video_embedding(&win_grid, &tm_grid, &fs_grid, &set_grid);
            BridgeTabGrid::sync_tab_grid_ui(&win_grid, &tm_grid, &lib_grid, &set_grid, &st_grid);
            StatusBarBridge::update_metrics(&win_grid, &st_grid);
        });

        let win_left = self.window.clone_strong();
        let tm_left = self.tab_manager.clone();
        let lib_left = self.library.clone();
        let set_left = self.settings.clone();
        let st_left = self.state.clone();
        let fs_left = self.fullscreen.clone();
        self.window.on_tab_grid_item_left_clicked(move |tab_id| {
            BridgeTabGrid::handle_item_left_clicked(
                tab_id as u64,
                &win_left,
                &tm_left,
                &lib_left,
                &set_left,
                &st_left,
                &fs_left,
            );
        });

        let win_mid = self.window.clone_strong();
        let tm_mid = self.tab_manager.clone();
        let set_mid = self.settings.clone();
        let fs_mid = self.fullscreen.clone();
        self.window.on_tab_grid_item_middle_clicked(move |tab_id| {
            BridgeTabGrid::handle_item_middle_clicked(tab_id as u64, &win_mid, &tm_mid, &set_mid, &fs_mid);
        });

        let win_close = self.window.clone_strong();
        let tm_close = self.tab_manager.clone();
        let lib_close = self.library.clone();
        let set_close = self.settings.clone();
        let st_close = self.state.clone();
        self.window.on_tab_grid_item_close_clicked(move |tab_id| {
            BridgeTabGrid::handle_item_close_clicked(
                tab_id as u64,
                &win_close,
                &tm_close,
                &lib_close,
                &set_close,
                &st_close,
            );
        });

        let win_cols = self.window.clone_strong();
        let tm_cols = self.tab_manager.clone();
        let lib_cols = self.library.clone();
        let set_cols = self.settings.clone();
        let st_cols = self.state.clone();
        self.window.on_tab_grid_columns_changed(move |new_cols| {
            let cols = (new_cols as usize).clamp(1, 10);
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
            BridgeTabGrid::sync_tab_grid_ui(&win_cols, &tm_cols, &lib_cols, &set_cols, &st_cols);
        });

        let win_auto = self.window.clone_strong();
        let tm_auto = self.tab_manager.clone();
        let lib_auto = self.library.clone();
        let set_auto = self.settings.clone();
        let st_auto = self.state.clone();
        self.window.on_tab_grid_auto_columns_toggled(move |is_auto| {
            {
                let mut st = st_auto.write();
                st.tab_grid_auto_columns = is_auto;
            }
            {
                let mut app_settings = set_auto.get();
                app_settings.view.tab_grid_auto_columns = is_auto;
                set_auto.save_quiet_async(&app_settings);
            }
            BridgeTabGrid::sync_tab_grid_ui(&win_auto, &tm_auto, &lib_auto, &set_auto, &st_auto);
        });

        let win_close_all = self.window.clone_strong();
        let tm_close_all = self.tab_manager.clone();
        let lib_close_all = self.library.clone();
        let set_close_all = self.settings.clone();
        let st_close_all = self.state.clone();
        self.window.on_tab_grid_close_all_clicked(move || {
            tracing::info!("[UiBridge: TabGrid] Close all tabs clicked from TabGrid toolbar");
            tm_close_all.close_all_tabs();
            BridgeViewerTabs::sync_ui(
                &win_close_all,
                &tm_close_all,
                &lib_close_all,
                &set_close_all,
                &st_close_all,
            );
            BridgeTabGrid::sync_tab_grid_ui(
                &win_close_all,
                &tm_close_all,
                &lib_close_all,
                &set_close_all,
                &st_close_all,
            );
        });

        let win_geom = self.window.clone_strong();
        let tm_geom = self.tab_manager.clone();
        let lib_geom = self.library.clone();
        let set_geom = self.settings.clone();
        let st_geom = self.state.clone();
        self.window.on_tab_grid_viewport_geometry_changed(move |_vw, _vh| {
            if win_geom.get_active_view_mode() == 2 {
                BridgeTabGrid::sync_tab_grid_ui(&win_geom, &tm_geom, &lib_geom, &set_geom, &st_geom);
            }
        });

        let win_scroll = self.window.clone_strong();
        let tm_scroll = self.tab_manager.clone();
        let set_scroll = self.settings.clone();
        let st_scroll = self.state.clone();
        self.window.on_tab_grid_scrolled(move |_dx, _dy| {
            BridgeTabGrid::handle_scrolled(&win_scroll, &tm_scroll, &set_scroll, &st_scroll);
        });
    }
}
