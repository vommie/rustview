use super::multiview::MultiviewHandle;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tab_grid::BridgeTabGrid;
use super::bridge_tags::BridgeTags;
use super::multiview::tab_host;
use super::tab_dnd::{TabDndAutoScroller, TabDndGeometry, TabDndSession};
use super::tab_icon::tab_icon;
use super::{MainWindow, TabItemModel};
use crate::library::models::MediaId;
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use crate::viewer::{
    get_slint_window_xid, HighResImageLoader, TabId, TabPayload, ViewerTabManager,
    X11VideoSubwindow,
};
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};
use std::sync::Arc;
use std::time::Instant;

/// Parameter bundle for concluding a tab drag & drop session.
pub struct TabDragEndParams<'a> {
    pub tab_id: TabId,
    pub window: &'a MainWindow,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub library: &'a Arc<Library>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub fullscreen: &'a MultiviewHandle,
    pub autoscroller: &'a TabDndAutoScroller,
}

/// Central coordinator for Viewer Tab interactions, asynchronous image loading, MPV video playback, fullscreen display, Tab DND reordering, and UI synchronization.
pub struct BridgeViewerTabs;

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

        // 9a. Double-click on Tab Media Viewer -> Toggle Fullscreen
        let win_fs = window.clone_strong();
        let tm_fs = tab_manager.clone();
        let set_fs = settings.clone();
        let fs_ctrl = fullscreen.clone();
        window.on_viewer_image_clicked(move || {
            if fs_ctrl.canvas_double_click() {
                tracing::info!(
                    "[BridgeViewerTabs: DoubleClick] Double-click detected on viewer canvas. Launching fullscreen viewer window (auto-play)..."
                );
                fs_ctrl.open_active_tab(&tm_fs, &set_fs, win_fs.as_weak());
            }
        });

        // 9b. Middle-click on Tab Media Viewer -> Toggle Fullscreen directly
        let win_fs_mid = window.clone_strong();
        let tm_fs_mid = tab_manager.clone();
        let set_fs_mid = settings.clone();
        let fs_ctrl_mid = fullscreen.clone();
        window.on_viewer_image_middle_clicked(move || {
            tracing::info!(
                "[BridgeViewerTabs: MiddleClick] Middle-click detected on viewer canvas. Launching fullscreen viewer window directly (auto-play)..."
            );
            fs_ctrl_mid.open_active_tab(&tm_fs_mid, &set_fs_mid, win_fs_mid.as_weak());
        });

        // 10a. The area of a Multiview tab changed (window or sidebar resize): place its native video windows
        let win_mv_geom = window.clone_strong();
        let tm_mv_geom = tab_manager.clone();
        let set_mv_geom = settings.clone();
        let fs_mv_geom = fullscreen.clone();
        window.on_multiview_viewport_changed(move || {
            if win_mv_geom.get_viewer_is_multiview() {
                Self::sync_video_embedding(&win_mv_geom, &tm_mv_geom, &fs_mv_geom, &set_mv_geom);
            }
        });

        // 10. Viewport Geometry Changed Callback
        let win_geom = window.clone_strong();
        let tm_geom = tab_manager;
        let set_geom = settings;
        let fs_geom = fullscreen;
        window.on_viewer_viewport_geometry_changed(move |_vx, _vy, _vw, _vh| {
            Self::sync_video_embedding(&win_geom, &tm_geom, &fs_geom, &set_geom);
        });
    }

    /// Helper wiring interactive fullscreen, close, and F2 rename callbacks onto an MpvPlayer instance.
    pub fn wire_player_callbacks(
        player: &Arc<crate::viewer::MpvPlayer>,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        fullscreen: &MultiviewHandle,
        settings: &Arc<SettingsManager>,
    ) {
        let fs_toggle = fullscreen.clone();
        let tm_toggle = tab_manager.clone();
        let set_toggle = settings.clone();
        let win_toggle = window.as_weak();

        player.on_toggle_fullscreen(move || {
            let fs = fs_toggle.clone();
            let tm = tm_toggle.clone();
            let set = set_toggle.clone();
            let _ = win_toggle.upgrade_in_event_loop(move |w| {
                if fs.is_isolated_open() {
                    tracing::info!(
                        "[BridgeViewerTabs: MpvDblClick] Fullscreen exit requested via MPV event"
                    );
                    fs.request_close();
                } else if w.get_active_view_mode() == 1 {
                    tracing::info!(
                        "[BridgeViewerTabs: MpvDblClick] Fullscreen enter requested via MPV event (auto-play)"
                    );
                    fs.open_active_tab(&tm, &set, w.as_weak());
                }
            });
        });

        let win_close = window.as_weak();
        player.on_close_tab(move || {
            let _ = win_close.upgrade_in_event_loop(move |w| {
                tracing::info!(
                    "[BridgeViewerTabs: MpvCloseTab] Ctrl+W intercepted from MPV video player. Invoking viewer_close_tab..."
                );
                w.invoke_viewer_close_tab();
            });
        });

        let win_ren = window.as_weak();
        player.on_rename_requested(move || {
            let _ = win_ren.upgrade_in_event_loop(move |w| {
                tracing::info!(
                    "[BridgeViewerTabs: MpvRename] F2 intercepted from MPV video player. Invoking request_rename..."
                );
                w.invoke_request_rename();
            });
        });
    }

    /// Cycles the active viewer tab with cyclic wrapping (mouse wheel on tab bar) and synchronizes UI & video embedding.
    pub fn cycle_active_tab(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        fullscreen: &MultiviewHandle,
        is_next: bool,
    ) {
        let total_tabs = tab_manager.tab_count();
        if total_tabs <= 1 {
            return;
        }

        let t0 = Instant::now();
        if let Some(new_tab_id) = tab_manager.cycle_tab(is_next) {
            let all_tabs = tab_manager.list_tabs();
            let new_title = ViewerTabManager::compute_tab_title(&all_tabs, new_tab_id);

            tracing::info!(
                new_tab_id,
                title = %LogObscurer::filename(&new_title),
                is_next,
                total_tabs,
                elapsed_us = t0.elapsed().as_micros(),
                "[BridgeViewerTabs: CycleTab] Cycled active tab via mouse wheel with automatic scroll into view"
            );

            window.set_is_tab_grid_active(false);
            window.set_is_browse_active(false);
            window.set_active_view_mode(1);
            Self::sync_ui(window, tab_manager, library, settings, state);
            fullscreen.sync_tab(tab_manager);
        }
    }

    /// Handles tab drag initiation: computes tab widths, creates ghost item preview, and starts DND session.
    pub fn handle_tab_drag_started(
        tab_id: TabId,
        source_index: usize,
        mouse_x: f32,
        tab_width: f32,
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        state: &Arc<RwLock<UiState>>,
    ) {
        let tabs = tab_manager.list_tabs();
        let total_tabs = tabs.len();
        if total_tabs <= 1 {
            return;
        }

        let Some(source_tab) = tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };

        let tab_ids: Vec<TabId> = tabs.iter().map(|t| t.id).collect();

        let tab_models = window.get_tabs();
        let mut widths = Vec::with_capacity(total_tabs);
        for i in 0..total_tabs {
            let w = if i == source_index && tab_width > 20.0 {
                tab_width
            } else if let Some(t_session) = tabs.get(i) {
                let has_rating =
                    window.get_viewer_show_tab_rating_badge() && t_session.record.rating > 0;
                TabDndGeometry::estimate_tab_width(&t_session.title, has_rating)
            } else {
                120.0
            };
            widths.push(w);
        }

        let slots = TabDndGeometry::compute_tab_slots(&tab_ids, &widths, 4.0, 2.0);
        let grab_offset = if let Some(s) = slots.get(source_index) {
            (mouse_x - s.start_x).clamp(0.0, s.width)
        } else {
            tab_width / 2.0
        };

        let session = TabDndSession::new(tab_id, source_index, grab_offset, mouse_x, slots);

        window.set_is_dragging_tab(true);
        window.set_drag_ghost_x(session.ghost_left_x());
        window.set_drag_ghost_title(source_tab.title.clone().into());
        window.set_drag_ghost_format(source_tab.format.default_extension().into());
        window.set_drag_ghost_icon(tab_icon(source_tab));
        window.set_drag_ghost_rating(source_tab.record.rating as i32);
        window.set_drag_ghost_is_active(Some(tab_id) == tab_manager.active_tab_id());

        for i in 0..tab_models.row_count() {
            if let Some(mut m) = tab_models.row_data(i) {
                let is_ph = m.id == tab_id as i32;
                if m.is_placeholder != is_ph {
                    m.is_placeholder = is_ph;
                    tab_models.set_row_data(i, m);
                }
            }
        }

        state.write().active_tab_dnd_session = Some(session);

        tracing::info!(
            tab_id,
            source_index,
            title = %LogObscurer::filename(&source_tab.title),
            mouse_x,
            "[TabDnd: Start] Tab drag gesture initiated with ghost preview"
        );
    }

    /// Handles pointer movement during tab drag: calculates target slot and applies smooth displacement to neighbor tabs.
    pub fn handle_tab_drag_moved(
        mouse_x: f32,
        view_width: f32,
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        autoscroller: &TabDndAutoScroller,
    ) {
        let (drag_tab_id, source_idx, cur_target_idx, ghost_x, shifts) = {
            let mut st = state.write();
            let Some(ref mut session) = st.active_tab_dnd_session else {
                return;
            };

            session.current_cursor_x = mouse_x;
            let center_x = session.dragged_center_x();
            let new_target_idx = TabDndGeometry::calculate_target_index(
                &session.slots,
                session.source_index,
                center_x,
                session.target_index,
            );

            session.target_index = new_target_idx;
            let shifts = TabDndGeometry::compute_shift_offsets(
                &session.slots,
                session.source_index,
                new_target_idx,
                4.0,
            );

            (
                session.dragged_tab_id,
                session.source_index,
                new_target_idx,
                session.ghost_left_x(),
                shifts,
            )
        };

        window.set_drag_ghost_x(ghost_x);

        let tab_models = window.get_tabs();
        let total_rows = tab_models.row_count();

        for i in 0..total_rows {
            if let Some(mut item) = tab_models.row_data(i) {
                let target_shift = shifts.get(i).copied().unwrap_or(0.0);
                let is_ph = item.id == drag_tab_id as i32;

                if (item.shift_x - target_shift).abs() > 0.1 || item.is_placeholder != is_ph {
                    item.shift_x = target_shift;
                    item.is_placeholder = is_ph;
                    tab_models.set_row_data(i, item);
                }
            }
        }

        let win_weak = window.as_weak();
        autoscroller.update_cursor(mouse_x, view_width, move |dx| {
            if let Some(w) = win_weak.upgrade() {
                w.invoke_scroll_tab_bar_by(dx);
            }
        });

        tracing::trace!(
            tab_id = drag_tab_id,
            source_index = source_idx,
            target_index = cur_target_idx,
            ghost_x = ghost_x,
            "[TabDnd: Move] Drag moved and neighbor shift offsets updated"
        );
    }

    /// Handles drop release: executes tab reordering in `ViewerTabManager` if target index changed and resyncs UI.
    pub fn handle_tab_drag_ended(params: TabDragEndParams<'_>) {
        params.autoscroller.stop();

        let session_opt = params.state.write().active_tab_dnd_session.take();
        params.window.set_is_dragging_tab(false);

        let Some(session) = session_opt else {
            Self::sync_ui(
                params.window,
                params.tab_manager,
                params.library,
                params.settings,
                params.state,
            );
            return;
        };

        let t_elapsed = session.start_time.elapsed().as_millis();
        let source_idx = session.source_index;
        let target_idx = session.target_index;

        if source_idx != target_idx {
            tracing::info!(
                tab_id = params.tab_id,
                source_index = source_idx,
                target_index = target_idx,
                reorder_time_ms = t_elapsed,
                "[TabDnd: Commit] Committed drag & drop tab reorder"
            );

            params.tab_manager.move_tab(params.tab_id, target_idx);

            let all_tabs = params.tab_manager.list_tabs();
            let title = ViewerTabManager::compute_tab_title(&all_tabs, params.tab_id);
            StatusBarBridge::success(
                params.window,
                format!("Reordered tab '{}' to position {}", title, target_idx + 1),
            );
        } else {
            tracing::debug!(
                tab_id = params.tab_id,
                source_index = source_idx,
                "[TabDnd: NoOp] Tab dropped in original slot"
            );
        }

        Self::sync_ui(
            params.window,
            params.tab_manager,
            params.library,
            params.settings,
            params.state,
        );
        Self::sync_video_embedding(
            params.window,
            params.tab_manager,
            params.fullscreen,
            params.settings,
        );

        if params.window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(
                params.window,
                params.tab_manager,
                params.library,
                params.settings,
                params.state,
            );
        }
    }

    /// Ensures the active tab item is fully visible in the tab bar by scrolling the tab bar horizontally if needed.
    pub fn scroll_active_tab_into_view(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        settings: &SettingsManager,
    ) {
        let Some(active_id) = tab_manager.active_tab_id() else {
            return;
        };

        let tabs = tab_manager.list_tabs();
        if tabs.len() <= 1 {
            return;
        }

        let show_rating = settings.get().viewer.tabs.show_rating_badge;
        if let Some((tab_x, tab_w)) =
            TabDndGeometry::find_tab_slot_geometry(&tabs, active_id, show_rating)
        {
            tracing::debug!(
                active_id,
                tab_x,
                tab_w,
                "[BridgeViewerTabs: ScrollIntoView] Scrolling tab bar to ensure active tab is visible"
            );
            window.invoke_scroll_tab_into_view(tab_x, tab_w);
        }
    }

    /// Synchronizes native video subwindow visibility, exact X/Y positioning, DPI scaling, and MPV input hooks.
    pub fn sync_video_embedding(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        fullscreen: &MultiviewHandle,
        settings: &Arc<SettingsManager>,
    ) {
        // Only a fullscreen that is isolated from the Browse view (it shows a tab's own player) owns the video windows; a
        // Multiview that follows the Browse selection leaves the embedding of tab videos alone.
        if fullscreen.is_isolated_open() {
            return;
        }

        let view_mode = window.get_active_view_mode();

        if view_mode == 0 {
            tab_host::suspend();
            tab_manager.hide_all_video_subwindows();
            return;
        }

        if view_mode == 2 {
            tab_host::suspend();
            return;
        }

        let active_tab = tab_manager.get_active_tab();
        let active_tab_id = active_tab.as_ref().map(|t| t.id);

        tab_manager.hide_video_subwindows_except(active_tab_id);

        if let Some(multiview) = active_tab.as_ref().and_then(|t| t.multiview()) {
            tab_host::sync(window, tab_manager, &multiview.instance);
            return;
        }
        tab_host::suspend();

        let is_video = active_tab
            .as_ref()
            .map(|t| matches!(t.payload, TabPayload::Video(_)))
            .unwrap_or(false);

        let file_exists = active_tab
            .as_ref()
            .map(|t| t.file_path.is_file())
            .unwrap_or(false);

        let has_error = active_tab
            .as_ref()
            .map(|t| match t.payload {
                TabPayload::Video(ref vid) => !vid.error_message.is_empty(),
                TabPayload::Image(ref img) => !img.error_message.is_empty(),
                TabPayload::Multiview(_) => false,
            })
            .unwrap_or(false);

        if !is_video || !file_exists || has_error {
            tab_manager.hide_all_video_subwindows();
            return;
        }

        let Some(main_xid) = get_slint_window_xid(window.window()) else {
            return;
        };

        let scale = window.window().scale_factor();
        let win_size = window.window().size();

        let logical_win_w = win_size.width as f32 / scale;
        let logical_win_h = win_size.height as f32 / scale;

        let sidebar_r = window.get_sidebar_right_width();
        let resizer_r = 6.0f32;
        let toolbar_top = 38.0f32;
        let toolbar_tab = 26.0f32;
        let status_bar_h = 28.0f32;

        let logical_x = 0.0f32;
        let logical_y = toolbar_top + toolbar_tab;
        let logical_w = (logical_win_w - (sidebar_r + resizer_r)).max(16.0);
        let logical_h = (logical_win_h - (logical_y + status_bar_h)).max(16.0);

        let phys_x = (logical_x * scale).round() as i32;
        let phys_y = (logical_y * scale).round() as i32;
        let phys_w = (logical_w * scale).round() as u32;
        let phys_h = (logical_h * scale).round() as u32;

        tab_manager.set_main_viewport_info(main_xid, phys_x, phys_y, phys_w, phys_h);

        if let Some(active_session) = active_tab
            && let TabPayload::Video(ref vid) = active_session.payload
        {
            Self::wire_player_callbacks(
                &vid.player,
                window,
                tab_manager,
                fullscreen,
                settings,
            );

            let mut sub_guard = vid.subwindow.lock();
            if let Some(ref mut sub) = *sub_guard {
                if sub.parent_xid() == main_xid {
                    sub.move_resize(phys_x, phys_y, phys_w, phys_h);
                } else {
                    let _ = sub.reparent(main_xid, phys_x, phys_y, phys_w, phys_h);
                }
                sub.show();
                tab_manager.reapply_active_cutouts_to_subwindow(sub);
            } else if let Ok(mut sub) =
                X11VideoSubwindow::new(main_xid, phys_x, phys_y, phys_w, phys_h)
            {
                let wid = sub.xid();
                let _ = vid.player.attach_to_window(wid);
                sub.show();
                tab_manager.reapply_active_cutouts_to_subwindow(&mut sub);
                *sub_guard = Some(sub);
            }

            vid.player.set_tab_grid_mode(false);
            vid.player.set_view_active(true);
        }
    }

    /// Triggers a full media refresh for the currently active tab item without altering the Browse View selection.
    pub fn refresh_active_tab(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if BridgeMedia::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(
                window,
                "Cannot refresh active tab: another operation is already in progress.",
            );
            return;
        }

        let active_media_ids = tab_manager.get_active_media_ids();
        if active_media_ids.is_empty() {
            return;
        }

        tracing::info!(
            media_ids = ?active_media_ids,
            "[BridgeViewerTabs: RefreshActive] Executing unified full media refresh for active tab"
        );

        BridgeMedia::refresh_selected_media(
            window,
            library,
            thumbnails,
            tab_manager,
            settings,
            state,
        );
    }

    /// Navigates to the adjacent (previous or next) media item in the current active list while keeping the same tab open in-place.
    pub fn navigate_adjacent(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        fullscreen: &MultiviewHandle,
        is_next: bool,
    ) {
        let active_media_id = match tab_manager.get_active_media_id() {
            Some(id) => id,
            None => return,
        };

        let target_media_id_opt = {
            let st = state.read();
            let current_pos = if !st.active_indices.is_empty() {
                st.active_indices.iter().position(|&idx| {
                    st.all_folder_media
                        .get(idx)
                        .map(|m| m.record.id == active_media_id)
                        .unwrap_or(false)
                })
            } else {
                st.all_folder_media
                    .iter()
                    .position(|m| m.record.id == active_media_id)
            };

            if let Some(pos) = current_pos {
                let target_pos = if is_next {
                    pos.checked_add(1)
                } else {
                    pos.checked_sub(1)
                };

                target_pos.and_then(|t_idx| {
                    if !st.active_indices.is_empty() {
                        st.active_indices
                            .get(t_idx)
                            .and_then(|&idx| st.all_folder_media.get(idx))
                            .map(|m| m.record.id)
                    } else {
                        st.all_folder_media.get(t_idx).map(|m| m.record.id)
                    }
                })
            } else {
                None
            }
        };

        if let Some(target_id) = target_media_id_opt {
            tracing::info!(
                active_media_id,
                target_media_id = target_id,
                is_next,
                "[BridgeViewerTabs: Navigate] Navigating in-place within current active tab session"
            );

            match tab_manager.navigate_active_tab_to_media(target_id) {
                Ok(tab_id) => {
                    Self::sync_ui(window, tab_manager, library, settings, state);
                    fullscreen.sync_tab(tab_manager);

                    let target_session = tab_manager.get_tab(tab_id);
                    let is_image = target_session
                        .as_ref()
                        .map(|t| matches!(t.payload, TabPayload::Image(_)))
                        .unwrap_or(false);
                    let file_exists = target_session
                        .as_ref()
                        .map(|t| t.file_path.is_file())
                        .unwrap_or(false);

                    if let Some(session) = target_session
                        && let TabPayload::Video(ref vid) = session.payload
                        && file_exists
                    {
                        Self::wire_player_callbacks(
                            &vid.player,
                            window,
                            tab_manager,
                            fullscreen,
                            settings,
                        );
                    }

                    if is_image && file_exists && let Ok(file_path) = library.resolve_media_path(target_id) {
                        let tm_async = tab_manager.clone();
                        let w_weak = window.as_weak();
                        let lib_async = library.clone();
                        let st_async = state.clone();
                        let set_async = settings.clone();
                        let fs_async = fullscreen.clone();

                        rayon::spawn(move || {
                            let load_res = HighResImageLoader::load_full_resolution(&file_path);

                            let _ = w_weak.upgrade_in_event_loop(move |w| {
                                if tm_async.get_tab(tab_id).map(|t| t.media_id) != Some(target_id) {
                                    tracing::debug!(
                                        target_id,
                                        tab_id,
                                        "[BridgeViewerTabs: DiscardStale] Discarding stale decoded image buffer because tab navigated away"
                                    );
                                    return;
                                }

                                match load_res {
                                    Ok((w_px, h_px, pixel_buffer)) => {
                                        tracing::info!(
                                            media_id = target_id,
                                            tab_id = tab_id,
                                            dimensions = format!("{}x{}", w_px, h_px),
                                            "[BridgeViewerTabs: Navigate] High-res buffer loaded for adjacent image"
                                        );
                                        tm_async.update_tab_image_buffer(tab_id, w_px, h_px, pixel_buffer);
                                    }
                                    Err(err) => {
                                        tracing::warn!(
                                            media_id = target_id,
                                            tab_id = tab_id,
                                            error = %err,
                                            "[BridgeViewerTabs: Navigate] Failed to load buffer for adjacent image"
                                        );
                                        tm_async.set_tab_error(tab_id, err.to_string());
                                    }
                                }
                                Self::sync_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                                fs_async.sync_tab(&tm_async);
                            });
                        });
                    }
                }
                Err(e) => {
                    tracing::error!(
                        target_id,
                        error = %e,
                        "[BridgeViewerTabs: NavigateError] Failed to navigate tab to media"
                    );
                }
            }
        }
    }

    /// Opens multiple media items in foreground viewer tabs.
    pub fn open_multiple_media_tabs(
        media_ids: &[MediaId],
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        fullscreen: &MultiviewHandle,
    ) {
        if media_ids.is_empty() {
            return;
        }

        if media_ids.len() == 1 {
            Self::open_media_tab(
                media_ids[0],
                window,
                tab_manager,
                library,
                settings,
                state,
                fullscreen,
            );
            return;
        }

        let t0 = Instant::now();
        let total = media_ids.len();

        tracing::info!(
            total_items = total,
            "[BridgeViewerTabs: OpenMultipleForeground] Opening {} media tabs with tail focus activation...",
            total
        );

        let app_settings = settings.get();

        // 1. Open items 0..(total - 1) in background
        for &media_id in &media_ids[..total - 1] {
            let is_video = {
                let st = state.read();
                if let Some(item) = st.all_folder_media.find(media_id) {
                    matches!(item.record.media_type, MediaType::Video)
                } else if let Ok(Some(record)) = library.media().get(media_id) {
                    matches!(record.media_type, MediaType::Video)
                } else {
                    false
                }
            };

            let allow_multiple = if is_video {
                app_settings.viewer.videos.allow_multiple_tabs
            } else {
                app_settings.viewer.images.allow_multiple_tabs
            };

            if let Ok((tab_id, is_new)) =
                tab_manager.open_media_in_background(media_id, allow_multiple)
                && !is_video
                && is_new
                && let Ok(file_path) = library.resolve_media_path(media_id)
                && file_path.is_file()
            {
                let tm_async = tab_manager.clone();
                let w_weak = window.as_weak();
                let lib_async = library.clone();
                let st_async = state.clone();
                let set_async = settings.clone();

                rayon::spawn(move || {
                    let load_res = HighResImageLoader::load_full_resolution(&file_path);
                    let _ = w_weak.upgrade_in_event_loop(move |w| {
                        if tm_async.get_tab(tab_id).map(|t| t.media_id) != Some(media_id) {
                            return;
                        }
                        match load_res {
                            Ok((w_px, h_px, pixel_buffer)) => {
                                tm_async.update_tab_image_buffer(tab_id, w_px, h_px, pixel_buffer);
                            }
                            Err(err) => {
                                tm_async.set_tab_error(tab_id, err.to_string());
                            }
                        }
                        Self::sync_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                    });
                });
            }
        }

        // 2. Open the last item in foreground
        let last_id = media_ids[total - 1];
        let is_last_video = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.find(last_id) {
                matches!(item.record.media_type, MediaType::Video)
            } else if let Ok(Some(record)) = library.media().get(last_id) {
                matches!(record.media_type, MediaType::Video)
            } else {
                false
            }
        };

        let last_allow_multiple = if is_last_video {
            app_settings.viewer.videos.allow_multiple_tabs
        } else {
            app_settings.viewer.images.allow_multiple_tabs
        };

        match tab_manager.open_or_focus_media(last_id, last_allow_multiple) {
            Ok((last_tab_id, is_new)) => {
                window.set_is_tab_grid_active(false);
                window.set_is_browse_active(false);
                window.set_active_view_mode(1);

                Self::sync_ui(window, tab_manager, library, settings, state);

                let file_exists = tab_manager
                    .get_tab(last_tab_id)
                    .map(|t| t.file_path.is_file())
                    .unwrap_or(false);

                if is_last_video {
                    if file_exists {
                        Self::sync_video_embedding(
                            window,
                            tab_manager,
                            fullscreen,
                            settings,
                        );
                    } else {
                        tab_manager.hide_all_video_subwindows();
                    }
                } else if is_new && file_exists && let Ok(file_path) = library.resolve_media_path(last_id) {
                    let tm_async = tab_manager.clone();
                    let w_weak = window.as_weak();
                    let lib_async = library.clone();
                    let st_async = state.clone();
                    let set_async = settings.clone();

                    rayon::spawn(move || {
                        let load_res = HighResImageLoader::load_full_resolution(&file_path);
                        let _ = w_weak.upgrade_in_event_loop(move |w| {
                            if tm_async.get_tab(last_tab_id).map(|t| t.media_id) != Some(last_id) {
                                return;
                            }
                            match load_res {
                                Ok((w_px, h_px, pixel_buffer)) => {
                                    tm_async.update_tab_image_buffer(last_tab_id, w_px, h_px, pixel_buffer);
                                }
                                Err(err) => {
                                    tm_async.set_tab_error(last_tab_id, err.to_string());
                                }
                            }
                            Self::sync_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                        });
                    });
                }
            }
            Err(e) => {
                tracing::error!(
                    media_id = last_id,
                    error = %e,
                    "[BridgeViewerTabs: OpenMultipleError] Failed to open last media tab in foreground"
                );
            }
        }

        let all_tabs = tab_manager.list_tabs();
        let last_title = ViewerTabManager::compute_tab_title(
            &all_tabs,
            tab_manager.active_tab_id().unwrap_or(0),
        );

        StatusBarBridge::success(
            window,
            format!("Opened {} tabs (Foreground: '{}')", total, last_title),
        );

        tracing::info!(
            total_opened = total,
            elapsed_us = t0.elapsed().as_micros(),
            "[BridgeViewerTabs: OpenMultipleForeground] Finished multi-tab foreground opening"
        );
    }

    /// Opens multiple media items in background viewer tabs without altering active tab or view mode.
    pub fn open_multiple_media_tabs_background(
        media_ids: &[MediaId],
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if media_ids.is_empty() {
            return;
        }

        let t0 = Instant::now();
        let total = media_ids.len();
        let app_settings = settings.get();

        tracing::info!(
            total_items = total,
            "[BridgeViewerTabs: OpenMultipleBackground] Opening {} media tabs in background...",
            total
        );

        let mut opened_count = 0usize;

        for &media_id in media_ids {
            let is_video = {
                let st = state.read();
                if let Some(item) = st.all_folder_media.find(media_id) {
                    matches!(item.record.media_type, MediaType::Video)
                } else if let Ok(Some(record)) = library.media().get(media_id) {
                    matches!(record.media_type, MediaType::Video)
                } else {
                    false
                }
            };

            let allow_multiple = if is_video {
                app_settings.viewer.videos.allow_multiple_tabs
            } else {
                app_settings.viewer.images.allow_multiple_tabs
            };

            if let Ok((tab_id, is_new)) =
                tab_manager.open_media_in_background(media_id, allow_multiple)
            {
                opened_count += 1;
                if !is_video
                    && is_new
                    && let Ok(file_path) = library.resolve_media_path(media_id)
                    && file_path.is_file()
                {
                    let tm_async = tab_manager.clone();
                    let w_weak = window.as_weak();
                    let lib_async = library.clone();
                    let st_async = state.clone();
                    let set_async = settings.clone();

                    rayon::spawn(move || {
                        let load_res = HighResImageLoader::load_full_resolution(&file_path);
                        let _ = w_weak.upgrade_in_event_loop(move |w| {
                            if tm_async.get_tab(tab_id).map(|t| t.media_id) != Some(media_id) {
                                return;
                            }
                            match load_res {
                                Ok((w_px, h_px, pixel_buffer)) => {
                                    tm_async.update_tab_image_buffer(
                                        tab_id,
                                        w_px,
                                        h_px,
                                        pixel_buffer,
                                    );
                                }
                                Err(err) => {
                                    tm_async.set_tab_error(tab_id, err.to_string());
                                }
                            }
                            Self::sync_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                            if w.get_active_view_mode() == 2 {
                                BridgeTabGrid::sync_tab_grid_ui(
                                    &w,
                                    &tm_async,
                                    &lib_async,
                                    &set_async,
                                    &st_async,
                                );
                            }
                        });
                    });
                }
            }
        }

        Self::sync_ui(window, tab_manager, library, settings, state);

        if window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(window, tab_manager, library, settings, state);
        }

        StatusBarBridge::success(
            window,
            format!("Opened {} tabs in background", opened_count),
        );

        tracing::info!(
            opened_count,
            total_requested = total,
            elapsed_us = t0.elapsed().as_micros(),
            "[BridgeViewerTabs: OpenMultipleBackground] Finished opening tabs in background"
        );
    }

    /// Opens a media file (Image or Video) in a viewer tab and initiates background loading or playback.
    pub fn open_media_tab(
        media_id: MediaId,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        fullscreen: &MultiviewHandle,
    ) {
        let t0 = Instant::now();

        let is_video = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.find(media_id) {
                matches!(item.record.media_type, MediaType::Video)
            } else if let Ok(Some(record)) = library.media().get(media_id) {
                matches!(record.media_type, MediaType::Video)
            } else {
                false
            }
        };

        let app_settings = settings.get();
        let allow_multiple = if is_video {
            app_settings.viewer.videos.allow_multiple_tabs
        } else {
            app_settings.viewer.images.allow_multiple_tabs
        };

        match tab_manager.open_or_focus_media(media_id, allow_multiple) {
            Ok((tab_id, is_new)) => {
                window.set_is_tab_grid_active(false);
                Self::sync_ui(window, tab_manager, library, settings, state);

                let is_video_session = tab_manager
                    .get_tab(tab_id)
                    .map(|t| matches!(t.payload, TabPayload::Video(_)))
                    .unwrap_or(false);

                let file_exists = tab_manager
                    .get_tab(tab_id)
                    .map(|t| t.file_path.is_file())
                    .unwrap_or(false);

                if is_video_session {
                    if file_exists {
                        Self::sync_video_embedding(window, tab_manager, fullscreen, settings);
                    } else {
                        tab_manager.hide_all_video_subwindows();
                    }
                } else if is_new && file_exists && let Ok(file_path) = library.resolve_media_path(media_id) {
                    let tm_async = tab_manager.clone();
                    let w_weak = window.as_weak();
                    let lib_async = library.clone();
                    let st_async = state.clone();
                    let set_async = settings.clone();

                    tracing::info!(
                        media_id,
                        tab_id,
                        path = %LogObscurer::path(&file_path),
                        "[BridgeViewerTabs: AsyncLoad] Spawning high-resolution background decode task..."
                    );

                    rayon::spawn(move || {
                        let t_decode = Instant::now();
                        let load_res = HighResImageLoader::load_full_resolution(&file_path);

                        let _ = w_weak.upgrade_in_event_loop(move |w| {
                            if tm_async.get_tab(tab_id).map(|t| t.media_id) != Some(media_id) {
                                tracing::debug!(
                                    media_id,
                                    tab_id,
                                    "[BridgeViewerTabs: DiscardStale] Discarding stale decoded image buffer because tab navigated away"
                                );
                                return;
                            }

                            match load_res {
                                Ok((w_px, h_px, pixel_buffer)) => {
                                    tracing::info!(
                                        media_id,
                                        tab_id,
                                        dimensions = format!("{}x{}", w_px, h_px),
                                        decode_time_ms = t_decode.elapsed().as_millis(),
                                        "[BridgeViewerTabs: AsyncLoad] High-res image decoded successfully, updating tab buffer"
                                    );
                                    tm_async.update_tab_image_buffer(tab_id, w_px, h_px, pixel_buffer);
                                }
                                Err(err) => {
                                    tracing::error!(
                                        media_id,
                                        tab_id,
                                        error = %err,
                                        "[BridgeViewerTabs: AsyncLoad] Failed to decode full-resolution image"
                                    );
                                    tm_async.set_tab_error(tab_id, err.to_string());
                                }
                            }
                            Self::sync_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                        });
                    });
                }

                tracing::info!(
                    media_id,
                    tab_id,
                    is_new,
                    is_video = is_video_session,
                    file_exists,
                    allow_multiple,
                    open_time_us = t0.elapsed().as_micros(),
                    "[BridgeViewerTabs: Open] Tab opened and UI synchronized"
                );
            }
            Err(e) => {
                tracing::error!(
                    media_id,
                    error = %e,
                    "[BridgeViewerTabs: OpenError] Failed to open media tab"
                );
                StatusBarBridge::error(
                    window,
                    format!("Failed to open tab for media #{}: {}", media_id, e),
                );
            }
        }
    }

    /// Opens a media file (Image or Video) in a background viewer tab without stealing focus or switching active view mode.
    /// Returns the tab (an existing one if duplicates are not allowed), or `None` if it could not be opened.
    pub fn open_media_tab_background(
        media_id: MediaId,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) -> Option<TabId> {
        let t0 = Instant::now();

        let (is_video, filename_hint) = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.find(media_id) {
                (
                    matches!(item.record.media_type, MediaType::Video),
                    item.record.filename.clone(),
                )
            } else if let Ok(Some(record)) = library.media().get(media_id) {
                (
                    matches!(record.media_type, MediaType::Video),
                    record.filename,
                )
            } else {
                (false, format!("Media #{}", media_id))
            }
        };

        let app_settings = settings.get();
        let allow_multiple = if is_video {
            app_settings.viewer.videos.allow_multiple_tabs
        } else {
            app_settings.viewer.images.allow_multiple_tabs
        };

        match tab_manager.open_media_in_background(media_id, allow_multiple) {
            Ok((tab_id, is_new)) => {
                Self::sync_ui(window, tab_manager, library, settings, state);

                if window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(window, tab_manager, library, settings, state);
                }

                let is_video_session = tab_manager
                    .get_tab(tab_id)
                    .map(|t| matches!(t.payload, TabPayload::Video(_)))
                    .unwrap_or(false);

                let file_exists = tab_manager
                    .get_tab(tab_id)
                    .map(|t| t.file_path.is_file())
                    .unwrap_or(false);

                if !is_video_session && is_new && file_exists && let Ok(file_path) = library.resolve_media_path(media_id) {
                    let tm_async = tab_manager.clone();
                    let w_weak = window.as_weak();
                    let lib_async = library.clone();
                    let st_async = state.clone();
                    let set_async = settings.clone();

                    tracing::info!(
                        media_id,
                        tab_id,
                        path = %LogObscurer::path(&file_path),
                        "[BridgeViewerTabs: AsyncLoadBackground] Spawning high-resolution background decode task for background tab..."
                    );

                    rayon::spawn(move || {
                        let t_decode = Instant::now();
                        let load_res = HighResImageLoader::load_full_resolution(&file_path);

                        let _ = w_weak.upgrade_in_event_loop(move |w| {
                            if tm_async.get_tab(tab_id).map(|t| t.media_id) != Some(media_id) {
                                tracing::debug!(
                                    media_id,
                                    tab_id,
                                    "[BridgeViewerTabs: DiscardStale] Discarding stale decoded image buffer because tab changed"
                                );
                                return;
                            }

                            match load_res {
                                Ok((w_px, h_px, pixel_buffer)) => {
                                    tracing::info!(
                                        media_id,
                                        tab_id,
                                        dimensions = format!("{}x{}", w_px, h_px),
                                        decode_time_ms = t_decode.elapsed().as_millis(),
                                        "[BridgeViewerTabs: AsyncLoadBackground] High-res image decoded successfully for background tab"
                                    );
                                    tm_async.update_tab_image_buffer(
                                        tab_id,
                                        w_px,
                                        h_px,
                                        pixel_buffer,
                                    );
                                }
                                Err(err) => {
                                    tracing::error!(
                                        media_id,
                                        tab_id,
                                        error = %err,
                                        "[BridgeViewerTabs: AsyncLoadBackground] Failed to decode full-resolution image for background tab"
                                    );
                                    tm_async.set_tab_error(tab_id, err.to_string());
                                }
                            }
                            Self::sync_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                            if w.get_active_view_mode() == 2 {
                                BridgeTabGrid::sync_tab_grid_ui(
                                    &w,
                                    &tm_async,
                                    &lib_async,
                                    &set_async,
                                    &st_async,
                                );
                            }
                        });
                    });
                }

                let all_tabs = tab_manager.list_tabs();
                let tab_title = ViewerTabManager::compute_tab_title(&all_tabs, tab_id);
                StatusBarBridge::success(
                    window,
                    format!(
                        "Opened '{}' in background tab",
                        if tab_title.is_empty() {
                            filename_hint
                        } else {
                            tab_title
                        }
                    ),
                );

                tracing::info!(
                    media_id,
                    tab_id,
                    is_new,
                    is_video = is_video_session,
                    file_exists,
                    allow_multiple,
                    open_time_us = t0.elapsed().as_micros(),
                    "[BridgeViewerTabs: OpenBackground] Tab opened in background and UI synchronized"
                );
                Some(tab_id)
            }
            Err(e) => {
                tracing::error!(
                    media_id,
                    error = %e,
                    "[BridgeViewerTabs: OpenBackgroundError] Failed to open background media tab"
                );
                StatusBarBridge::error(
                    window,
                    format!("Failed to open background tab for media #{}: {}", media_id, e),
                );
                None
            }
        }
    }

    /// Synchronizes open tabs, active view modes, viewer image buffers, right sidebar locking, and auto-scrolls the active tab into view.
    pub fn sync_ui(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        // Every tab that exists from here on is rendered below, so this render counts for the revision read now.
        tab_manager.mark_tab_ui_synced(tab_manager.tab_ui_revision());
        let tabs = tab_manager.list_tabs();
        let active_id_opt = tab_manager.active_tab_id();
        let is_tab_grid = window.get_is_tab_grid_active();
        let is_browse = !is_tab_grid && active_id_opt.is_none();

        // 1. Prepare Tab Models for the Top Toolbar
        let tab_models: Vec<TabItemModel> = tabs
            .iter()
            .map(|t| TabItemModel {
                id: t.id as i32,
                media_id: t.media_id as i32,
                title: t.title.clone().into(),
                format_str: t.format.default_extension().into(),
                is_active: !is_tab_grid && Some(t.id) == active_id_opt,
                icon: tab_icon(t),
                rating: t.record.rating as i32,
                has_tags: false,
                shift_x: 0.0,
                is_placeholder: false,
            })
            .collect();

        window.set_tabs(ModelRc::new(VecModel::from(tab_models)));
        window.set_is_browse_active(is_browse);
        window.set_is_tab_grid_active(is_tab_grid);

        let target_mode = if is_tab_grid {
            2
        } else if is_browse {
            0
        } else {
            1
        };
        window.set_active_view_mode(target_mode);

        let bg_mode_idx = match settings.get().viewer.background_mode.to_lowercase().as_str() {
            "deep black" | "black" => 1,
            "transparent checkerboard" | "checkerboard" => 2,
            _ => 0,
        };
        window.set_viewer_background_mode(bg_mode_idx);

        // 2. Synchronize Tab View Content when in single tab viewer mode
        if target_mode == 1 && let Some(active_tab) = tab_manager.get_active_tab() {
            let is_video = active_tab.is_video();
            window.set_viewer_is_video(is_video);
            window.set_viewer_is_multiview(active_tab.is_multiview());
            window.set_viewer_filename(active_tab.title.clone().into());

            let (display_w, display_h) = if active_tab.is_multiview() {
                active_tab.display_size()
            } else {
                (active_tab.record.width, active_tab.record.height)
            };
            let res_str = if display_w > 0 && display_h > 0 {
                format!("{} x {}", display_w, display_h)
            } else {
                String::new()
            };
            window.set_viewer_resolution(res_str.into());

            let (has_prev, has_next) = if active_tab.is_multiview() {
                (false, false)
            } else {
                let st = state.read();
                let current_pos = if !st.active_indices.is_empty() {
                    st.active_indices.iter().position(|&idx| {
                        st.all_folder_media
                            .get(idx)
                            .map(|m| m.record.id == active_tab.media_id)
                            .unwrap_or(false)
                    })
                } else {
                    st.all_folder_media
                        .iter()
                        .position(|m| m.record.id == active_tab.media_id)
                };

                let total_available = if !st.active_indices.is_empty() {
                    st.active_indices.len()
                } else {
                    st.all_folder_media.len()
                };

                if let Some(pos) = current_pos {
                    (pos > 0, pos + 1 < total_available)
                } else {
                    (false, false)
                }
            };

            window.set_viewer_has_previous(has_prev);
            window.set_viewer_has_next(has_next);

            match active_tab.payload {
                TabPayload::Image(ref img) => {
                    window.set_viewer_is_loading(img.is_loading);
                    window.set_viewer_error(img.error_message.clone().into());

                    if let Some(ref buf) = img.high_res_pixels {
                        window.set_viewer_image(Image::from_rgb8(buf.clone()));
                    } else {
                        window.set_viewer_image(Image::default());
                    }
                }
                TabPayload::Video(ref vid) => {
                    window.set_viewer_is_loading(vid.is_loading);
                    window.set_viewer_error(vid.error_message.clone().into());
                    window.set_viewer_image(Image::default());
                }
                TabPayload::Multiview(_) => {
                    window.set_viewer_is_loading(false);
                    window.set_viewer_error("".into());
                    window.set_viewer_image(Image::default());
                }
            }

            {
                let mut st = state.write();
                st.active_tab_media_ids = active_tab.member_ids();
            }
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
            StatusBarBridge::update_metrics(window, state);

            Self::scroll_active_tab_into_view(window, tab_manager, settings);
        } else {
            window.set_viewer_is_video(false);
            window.set_viewer_is_multiview(false);
            {
                let mut st = state.write();
                st.active_tab_media_ids.clear();
            }
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
            StatusBarBridge::update_metrics(window, state);
        }
    }
}
