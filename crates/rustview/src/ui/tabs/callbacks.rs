//! The callbacks of the tab bar and the tab view.

use crate::ui::multiview::MultiviewHandle;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tabs::dnd::TabDndAutoScroller;
use crate::ui::MainWindow;
use rustview_core::library::Library;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::sync::Arc;
use super::{BridgeViewerTabs, TabDragEndParams};

impl BridgeViewerTabs {
    /// Registers all tab-related UI callbacks on the Slint `MainWindow`.
    pub fn register_callbacks(
        window: &MainWindow,
        tab_manager: Arc<ViewerTabManager>,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
        fullscreen: MultiviewHandle,
    ) {
        let tab_autoscroller = TabDndAutoScroller::new();

        // 1. Browse Button Clicked (Switch to Media Browser view -> auto-pause active video)
        let win_browse = window.clone_strong();
        let tm_browse = tab_manager.clone();
        let lib_browse = library.clone();
        let st_browse = state.clone();
        let set_browse = settings.clone();
        let fs_browse = fullscreen.clone();
        window.on_browse_clicked(move || {
            tracing::info!(
                "[BridgeViewerTabs: Browse] User clicked Browse button. Switching to media browser and auto-pausing video tabs"
            );
            tm_browse.activate_browse();
            win_browse.set_is_tab_grid_active(false);
            win_browse.set_is_browse_active(true);
            win_browse.set_active_view_mode(0);
            Self::sync_ui(&win_browse, &tm_browse, &lib_browse, &set_browse, &st_browse);
            Self::sync_video_embedding(&win_browse, &tm_browse, &fs_browse, &set_browse);
        });

        // 2. Tab Selected in TopToolbar
        let win_select = window.clone_strong();
        let tm_select = tab_manager.clone();
        let lib_select = library.clone();
        let st_select = state.clone();
        let set_select = settings.clone();
        let fs_select = fullscreen.clone();
        window.on_tab_selected(move |tab_id| {
            tracing::info!(tab_id, "[BridgeViewerTabs: Select] Tab selected by user");
            tm_select.set_active_tab_internal(tab_id as TabId, true);
            win_select.set_is_tab_grid_active(false);
            win_select.set_is_browse_active(false);
            win_select.set_active_view_mode(1);
            Self::sync_ui(&win_select, &tm_select, &lib_select, &set_select, &st_select);
            Self::sync_video_embedding(&win_select, &tm_select, &fs_select, &set_select);
        });

        // 3. Tab Closed (Close button or middle click)
        let win_close = window.clone_strong();
        let tm_close = tab_manager.clone();
        let lib_close = library.clone();
        let st_close = state.clone();
        let set_close = settings.clone();
        let fs_close = fullscreen.clone();
        window.on_tab_closed(move |tab_id| {
            tracing::info!(tab_id, "[BridgeViewerTabs: Close] Tab closed by user");
            tm_close.close_tab(tab_id as TabId);
            Self::sync_ui(&win_close, &tm_close, &lib_close, &set_close, &st_close);
            Self::sync_video_embedding(&win_close, &tm_close, &fs_close, &set_close);
            fs_close.sync_tab(&tm_close);
            if win_close.get_active_view_mode() == 2 {
                BridgeTabGrid::sync_tab_grid_ui(
                    &win_close,
                    &tm_close,
                    &lib_close,
                    &set_close,
                    &st_close,
                );
            }
        });

        // 4. Tab Mouse Wheel Scrolling & Cycling Callback
        let win_wheel = window.clone_strong();
        let tm_wheel = tab_manager.clone();
        let lib_wheel = library.clone();
        let set_wheel = settings.clone();
        let st_wheel = state.clone();
        let fs_wheel = fullscreen.clone();
        window.on_tab_wheel_scrolled(move |is_next| {
            Self::cycle_active_tab(
                &win_wheel,
                &tm_wheel,
                &lib_wheel,
                &set_wheel,
                &st_wheel,
                &fs_wheel,
                is_next,
            );
            Self::sync_video_embedding(&win_wheel, &tm_wheel, &fs_wheel, &set_wheel);
        });

        // 5. Tab Drag & Drop Reordering Callbacks
        let win_dstart = window.clone_strong();
        let tm_dstart = tab_manager.clone();
        let st_dstart = state.clone();
        window.on_tab_drag_started(move |tab_id, s_idx, mouse_x, tab_w| {
            Self::handle_tab_drag_started(
                tab_id as TabId,
                s_idx as usize,
                mouse_x,
                tab_w,
                &win_dstart,
                &tm_dstart,
                &st_dstart,
            );
        });

        let win_dmove = window.clone_strong();
        let st_dmove = state.clone();
        let scroller_dmove = tab_autoscroller.clone();
        window.on_tab_drag_moved(move |mouse_x, view_w| {
            Self::handle_tab_drag_moved(
                mouse_x,
                view_w,
                &win_dmove,
                &st_dmove,
                &scroller_dmove,
            );
        });

        let win_dend = window.clone_strong();
        let tm_dend = tab_manager.clone();
        let lib_dend = library.clone();
        let set_dend = settings.clone();
        let st_dend = state.clone();
        let fs_dend = fullscreen.clone();
        let scroller_dend = tab_autoscroller.clone();
        window.on_tab_drag_ended(move |tab_id, _target_idx| {
            Self::handle_tab_drag_ended(TabDragEndParams {
                tab_id: tab_id as TabId,
                window: &win_dend,
                tab_manager: &tm_dend,
                library: &lib_dend,
                settings: &set_dend,
                state: &st_dend,
                fullscreen: &fs_dend,
                autoscroller: &scroller_dend,
            });
        });

        let win_dcancel = window.clone_strong();
        let tm_dcancel = tab_manager.clone();
        let lib_dcancel = library.clone();
        let set_dcancel = settings.clone();
        let st_dcancel = state.clone();
        let scroller_dcancel = tab_autoscroller;
        window.on_tab_drag_cancelled(move |_tab_id| {
            scroller_dcancel.stop();
            st_dcancel.write().active_tab_dnd_session = None;
            win_dcancel.set_is_dragging_tab(false);
            Self::sync_ui(
                &win_dcancel,
                &tm_dcancel,
                &lib_dcancel,
                &set_dcancel,
                &st_dcancel,
            );
        });

        // 6. Viewer Toolbar Close Tab Button / Keyboard Shortcut
        let win_v_close = window.clone_strong();
        let tm_v_close = tab_manager.clone();
        let lib_v_close = library.clone();
        let st_v_close = state.clone();
        let set_v_close = settings.clone();
        let fs_v_close = fullscreen.clone();
        window.on_viewer_close_tab(move || {
            if let Some(active_id) = tm_v_close.active_tab_id() {
                tracing::info!(
                    tab_id = active_id,
                    "[BridgeViewerTabs: ViewerClose] Closed active tab from viewer toolbar / shortcut"
                );
                tm_v_close.close_tab(active_id);
                Self::sync_ui(
                    &win_v_close,
                    &tm_v_close,
                    &lib_v_close,
                    &set_v_close,
                    &st_v_close,
                );
                Self::sync_video_embedding(&win_v_close, &tm_v_close, &fs_v_close, &set_v_close);
                fs_v_close.sync_tab(&tm_v_close);
                if win_v_close.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(
                        &win_v_close,
                        &tm_v_close,
                        &lib_v_close,
                        &set_v_close,
                        &st_v_close,
                    );
                }
            }
        });

        // 7. Viewer Toolbar Refresh Button (Guarded by is_system_busy)
        let win_v_refresh = window.clone_strong();
        let tm_v_refresh = tab_manager.clone();
        let lib_v_refresh = library.clone();
        let thumb_v_refresh = thumbnails;
        let set_v_refresh = settings.clone();
        let st_v_refresh = state.clone();
        let fs_v_refresh = fullscreen.clone();
        window.on_viewer_refresh(move || {
            if BridgeMedia::is_system_busy(&win_v_refresh, &thumb_v_refresh, &st_v_refresh) {
                StatusBarBridge::warn(
                    &win_v_refresh,
                    "Cannot refresh: another operation is already in progress.",
                );
                return;
            }
            tracing::info!("[BridgeViewerTabs: Refresh] Refresh requested from viewer toolbar");
            Self::refresh_active_tab(
                &win_v_refresh,
                &tm_v_refresh,
                &lib_v_refresh,
                &thumb_v_refresh,
                &set_v_refresh,
                &st_v_refresh,
            );
            fs_v_refresh.sync_tab(&tm_v_refresh);
        });

        // 8. Previous / Next Navigation in active list
        let win_prev = window.clone_strong();
        let tm_prev = tab_manager.clone();
        let lib_prev = library.clone();
        let st_prev = state.clone();
        let set_prev = settings.clone();
        let fs_prev = fullscreen.clone();
        window.on_viewer_previous(move || {
            Self::navigate_adjacent(
                &win_prev, &tm_prev, &lib_prev, &set_prev, &st_prev, &fs_prev, false,
            );
            Self::sync_video_embedding(&win_prev, &tm_prev, &fs_prev, &set_prev);
        });

        let win_next = window.clone_strong();
        let tm_next = tab_manager.clone();
        let lib_next = library.clone();
        let st_next = state.clone();
        let set_next = settings.clone();
        let fs_next = fullscreen.clone();
        window.on_viewer_next(move || {
            Self::navigate_adjacent(
                &win_next, &tm_next, &lib_next, &set_next, &st_next, &fs_next, true,
            );
            Self::sync_video_embedding(&win_next, &tm_next, &fs_next, &set_next);
        });

        // 10. The area of the tab view changed (window or sidebar resize): the Multiview of the active tab places its native video
        // windows again.
        let win_geom = window.clone_strong();
        window.on_multiview_viewport_changed(move || {
            if win_geom.get_active_view_mode() == 1 {
                Self::sync_video_embedding(&win_geom, &tab_manager, &fullscreen, &settings);
            }
        });
    }
}
