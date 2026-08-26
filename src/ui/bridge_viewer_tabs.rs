use super::bridge_image_fullscreen::ImageFullscreenDialogController;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::{MainWindow, TabItemModel};
use crate::library::models::MediaId;
use crate::library::Library;
use crate::settings::{LogObscurer, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use crate::viewer::{
    get_slint_window_xid, HighResImageLoader, TabId, TabPayload, ViewerTabManager,
    X11VideoSubwindow,
};
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::sync::Arc;
use std::time::Instant;

/// Central coordinator for Viewer Tab interactions, asynchronous image loading, MPV video playback, fullscreen display, and UI synchronization.
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
        fullscreen_controller: ImageFullscreenDialogController,
    ) {
        // 1. Browse Button Clicked (Switch to Media Browser view)
        let win_browse = window.clone_strong();
        let tm_browse = tab_manager.clone();
        let lib_browse = library.clone();
        let st_browse = state.clone();
        let set_browse = settings.clone();
        window.on_browse_clicked(move || {
            tracing::info!("[BridgeViewerTabs: Browse] User clicked Browse button. Switching to media browser");
            tm_browse.activate_browse();
            Self::sync_ui(&win_browse, &tm_browse, &lib_browse, &set_browse, &st_browse);
        });

        // 2. Tab Selected in TopToolbar
        let win_select = window.clone_strong();
        let tm_select = tab_manager.clone();
        let lib_select = library.clone();
        let st_select = state.clone();
        let set_select = settings.clone();
        let fs_select = fullscreen_controller.clone();
        window.on_tab_selected(move |tab_id| {
            tracing::info!(tab_id, "[BridgeViewerTabs: Select] Tab selected by user");
            tm_select.set_active_tab(tab_id as TabId);
            Self::sync_ui(&win_select, &tm_select, &lib_select, &set_select, &st_select);
            Self::sync_video_embedding(&win_select, &tm_select, &fs_select);
        });

        // 3. Tab Closed (Close button or middle click)
        let win_close = window.clone_strong();
        let tm_close = tab_manager.clone();
        let lib_close = library.clone();
        let st_close = state.clone();
        let set_close = settings.clone();
        let fs_close = fullscreen_controller.clone();
        window.on_tab_closed(move |tab_id| {
            tracing::info!(tab_id, "[BridgeViewerTabs: Close] Tab closed by user");
            tm_close.close_tab(tab_id as TabId);
            Self::sync_ui(&win_close, &tm_close, &lib_close, &set_close, &st_close);
            Self::sync_video_embedding(&win_close, &tm_close, &fs_close);
            fs_close.sync_active_tab(&tm_close, &set_close);
        });

        // 4. Viewer Toolbar Close Tab Button (triggered via Escape hotkey)
        let win_v_close = window.clone_strong();
        let tm_v_close = tab_manager.clone();
        let lib_v_close = library.clone();
        let st_v_close = state.clone();
        let set_v_close = settings.clone();
        let fs_v_close = fullscreen_controller.clone();
        window.on_viewer_close_tab(move || {
            if let Some(active_id) = tm_v_close.active_tab_id() {
                tracing::info!(
                    tab_id = active_id,
                    "[BridgeViewerTabs: ViewerClose] Closed active tab from viewer toolbar"
                );
                tm_v_close.close_tab(active_id);
                Self::sync_ui(&win_v_close, &tm_v_close, &lib_v_close, &set_v_close, &st_v_close);
                Self::sync_video_embedding(&win_v_close, &tm_v_close, &fs_v_close);
                fs_v_close.sync_active_tab(&tm_v_close, &set_v_close);
            }
        });

        // 5. Viewer Toolbar Refresh Button (Reuses the exact same Full Refresh Flow as Context Menu)
        let win_v_refresh = window.clone_strong();
        let tm_v_refresh = tab_manager.clone();
        let lib_v_refresh = library.clone();
        let thumb_v_refresh = thumbnails;
        let set_v_refresh = settings.clone();
        let st_v_refresh = state.clone();
        let fs_v_refresh = fullscreen_controller.clone();
        window.on_viewer_refresh(move || {
            tracing::info!("[BridgeViewerTabs: Refresh] Refresh requested from viewer toolbar");
            Self::refresh_active_tab(
                &win_v_refresh,
                &tm_v_refresh,
                &lib_v_refresh,
                &thumb_v_refresh,
                &set_v_refresh,
                &st_v_refresh,
            );
            fs_v_refresh.sync_active_tab(&tm_v_refresh, &set_v_refresh);
        });

        // 6. Previous / Next Navigation in active list (In-place within current tab)
        let win_prev = window.clone_strong();
        let tm_prev = tab_manager.clone();
        let lib_prev = library.clone();
        let st_prev = state.clone();
        let set_prev = settings.clone();
        let fs_prev = fullscreen_controller.clone();
        window.on_viewer_previous(move || {
            Self::navigate_adjacent(&win_prev, &tm_prev, &lib_prev, &set_prev, &st_prev, &fs_prev, false);
            Self::sync_video_embedding(&win_prev, &tm_prev, &fs_prev);
        });

        let win_next = window.clone_strong();
        let tm_next = tab_manager.clone();
        let lib_next = library;
        let st_next = state;
        let set_next = settings.clone();
        let fs_next = fullscreen_controller.clone();
        window.on_viewer_next(move || {
            Self::navigate_adjacent(&win_next, &tm_next, &lib_next, &set_next, &st_next, &fs_next, true);
            Self::sync_video_embedding(&win_next, &tm_next, &fs_next);
        });

        // 7. Double-click on Tab Media Viewer -> Toggle Fullscreen (with strict Main UI lock and seamless video reparenting)
        let win_fs = window.clone_strong();
        let tm_fs = tab_manager.clone();
        let set_fs = settings.clone();
        let fs_ctrl = fullscreen_controller.clone();
        window.on_viewer_image_clicked(move || {
            if fs_ctrl.check_double_click() {
                tracing::info!("[BridgeViewerTabs: DoubleClick] Double-click detected on viewer canvas. Launching fullscreen viewer window...");
                fs_ctrl.show(&tm_fs, &set_fs, win_fs.as_weak());
            }
        });

        // 8. Intercept Double-Click / 'f' Fullscreen Toggle directly from native MPV player
        let fs_toggle = fullscreen_controller.clone();
        let tm_toggle = tab_manager.clone();
        let set_toggle = settings;
        let win_toggle = window.as_weak();
        tab_manager.player().on_toggle_fullscreen(move || {
            let fs = fs_toggle.clone();
            let tm = tm_toggle.clone();
            let set = set_toggle.clone();
            let _ = win_toggle.upgrade_in_event_loop(move |w| {
                if fs.is_open() {
                    tracing::info!("[BridgeViewerTabs: MpvDblClick] Fullscreen exit requested via MPV double-click");
                    fs.close();
                } else {
                    tracing::info!("[BridgeViewerTabs: MpvDblClick] Fullscreen enter requested via MPV double-click");
                    fs.show(&tm, &set, w.as_weak());
                }
            });
        });

        // 9. Dismiss Fullscreen from Main UI if Escape or backdrop is clicked while locked
        let fs_modal = fullscreen_controller.clone();
        window.on_modal_dismiss_requested(move || {
            tracing::info!("[BridgeViewerTabs: ModalDismiss] Main UI modal dismiss triggered. Closing active fullscreen window if present.");
            fs_modal.close();
        });

        // 10. Viewport Geometry Changed Callback (Tracks Slint Layout and resizes X11 Subwindow)
        let win_geom = window.clone_strong();
        let tm_geom = tab_manager;
        let fs_geom = fullscreen_controller;
        window.on_viewer_viewport_geometry_changed(move |_vx, _vy, _vw, _vh| {
            Self::sync_video_embedding(&win_geom, &tm_geom, &fs_geom);
        });
    }

    /// Synchronizes native video subwindow visibility, exact X/Y positioning and DPI-aware dimensioning with Slint viewport.
    pub fn sync_video_embedding(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        fullscreen_controller: &ImageFullscreenDialogController,
    ) {
        if fullscreen_controller.is_open() {
            return;
        }

        let active_tab = tab_manager.get_active_tab();
        let is_video = active_tab
            .as_ref()
            .map(|t| matches!(t.payload, TabPayload::Video(_)))
            .unwrap_or(false);

        if !is_video {
            if let Some(ref mut sub) = *tab_manager.video_subwindow().lock() {
                sub.hide();
            }
            return;
        }

        let Some(main_xid) = get_slint_window_xid(window.window()) else {
            return;
        };

        let scale = window.window().scale_factor();
        let win_size = window.window().size(); // Physical pixel dimensions

        let logical_win_w = win_size.width as f32 / scale;
        let logical_win_h = win_size.height as f32 / scale;

        let sidebar_r = window.get_sidebar_right_width(); // Logical pixels
        let resizer_r = 6.0f32; // Resizer bar thickness
        let toolbar_top = 38.0f32; // TopToolbar height
        let toolbar_tab = 26.0f32; // ImageTabPanelToolbar height
        let status_bar_h = 28.0f32; // StatusBar height

        // In Tab View (Mode 1), left sidebar and left resizer are 0px
        let logical_x = 0.0f32;
        let logical_y = toolbar_top + toolbar_tab;
        let logical_w = (logical_win_w - (sidebar_r + resizer_r)).max(16.0);
        let logical_h = (logical_win_h - (logical_y + status_bar_h)).max(16.0);

        let phys_x = (logical_x * scale).round() as i32;
        let phys_y = (logical_y * scale).round() as i32;
        let phys_w = (logical_w * scale).round() as u32;
        let phys_h = (logical_h * scale).round() as u32;

        fullscreen_controller.set_main_viewport_info(main_xid, phys_x, phys_y, phys_w, phys_h);

        let mut sub_guard = tab_manager.video_subwindow().lock();
        if let Some(ref mut sub) = *sub_guard {
            if sub.parent_xid() == main_xid {
                sub.move_resize(phys_x, phys_y, phys_w, phys_h);
                sub.show();
            } else {
                let _ = sub.reparent(main_xid, phys_x, phys_y, phys_w, phys_h);
                sub.show();
            }
        } else if let Ok(mut sub) = X11VideoSubwindow::new(main_xid, phys_x, phys_y, phys_w, phys_h) {
            let wid = sub.xid();
            let _ = tab_manager.player().attach_to_window(wid);
            sub.show();
            *sub_guard = Some(sub);
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
        let active_media_id = match tab_manager.get_active_media_id() {
            Some(id) => id,
            None => return,
        };

        tracing::info!(
            media_id = active_media_id,
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
        fullscreen_controller: &ImageFullscreenDialogController,
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
                    st.all_folder_media.get(idx).map(|m| m.record.id == active_media_id).unwrap_or(false)
                })
            } else {
                st.all_folder_media.iter().position(|m| m.record.id == active_media_id)
            };

            if let Some(pos) = current_pos {
                let target_pos = if is_next {
                    pos.checked_add(1)
                } else {
                    pos.checked_sub(1)
                };

                target_pos.and_then(|t_idx| {
                    if !st.active_indices.is_empty() {
                        st.active_indices.get(t_idx).and_then(|&idx| st.all_folder_media.get(idx)).map(|m| m.record.id)
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
                    fullscreen_controller.sync_active_tab(tab_manager, settings);

                    let is_image = tab_manager
                        .get_tab(tab_id)
                        .map(|t| matches!(t.payload, TabPayload::Image(_)))
                        .unwrap_or(false);

                    if is_image && let Ok(file_path) = library.resolve_media_path(target_id) {
                        let tm_async = tab_manager.clone();
                        let w_weak = window.as_weak();
                        let lib_async = library.clone();
                        let st_async = state.clone();
                        let set_async = settings.clone();
                        let fs_async = fullscreen_controller.clone();

                        rayon::spawn(move || {
                            let load_res = HighResImageLoader::load_full_resolution(&file_path);

                            let _ = w_weak.upgrade_in_event_loop(move |w| {
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
                                fs_async.sync_active_tab(&tm_async, &set_async);
                            });
                        });
                    }
                }
                Err(e) => {
                    tracing::error!(target_id, error = %e, "[BridgeViewerTabs: NavigateError] Failed to navigate tab to media");
                }
            }
        }
    }

    /// Opens a media file (Image or Video) in a viewer tab and initiates background loading or playback.
    pub fn open_media_tab(
        media_id: MediaId,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t0 = Instant::now();
        match tab_manager.open_or_focus_media(media_id) {
            Ok((tab_id, is_new)) => {
                Self::sync_ui(window, tab_manager, library, settings, state);

                let is_video = tab_manager
                    .get_tab(tab_id)
                    .map(|t| matches!(t.payload, TabPayload::Video(_)))
                    .unwrap_or(false);

                if is_video {
                    let fs_dummy = ImageFullscreenDialogController::new();
                    Self::sync_video_embedding(window, tab_manager, &fs_dummy);
                } else if is_new && let Ok(file_path) = library.resolve_media_path(media_id) {
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
                    is_video,
                    open_time_us = t0.elapsed().as_micros(),
                    "[BridgeViewerTabs: Open] Tab opened and UI synchronized"
                );
            }
            Err(e) => {
                tracing::error!(media_id, error = %e, "[BridgeViewerTabs: OpenError] Failed to open media tab");
                StatusBarBridge::error(
                    window,
                    format!("Failed to open tab for media #{}: {}", media_id, e),
                );
            }
        }
    }

    /// Synchronizes open tabs, active view modes, viewer image buffers, and right sidebar locking to Slint UI.
    pub fn sync_ui(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let tabs = tab_manager.list_tabs();
        let active_id_opt = tab_manager.active_tab_id();
        let is_browse = active_id_opt.is_none();

        // 1. Prepare Tab Models for the Top Toolbar
        let tab_models: Vec<TabItemModel> = tabs
            .iter()
            .map(|t| TabItemModel {
                id: t.id as i32,
                media_id: t.media_id as i32,
                title: t.filename.clone().into(),
                format_str: t.format.default_extension().into(),
                is_active: Some(t.id) == active_id_opt,
                is_video: matches!(t.payload, TabPayload::Video(_)),
                rating: t.record.rating as i32,
                has_tags: false,
            })
            .collect();

        window.set_tabs(ModelRc::new(VecModel::from(tab_models)));
        window.set_is_browse_active(is_browse);
        window.set_active_view_mode(if is_browse { 0 } else { 1 });

        // Synchronize Viewer Canvas Background from Settings
        let bg_mode_idx = match settings.get().viewer.background_mode.to_lowercase().as_str() {
            "deep black" | "black" => 1,
            "transparent checkerboard" | "checkerboard" => 2,
            _ => 0,
        };
        window.set_viewer_background_mode(bg_mode_idx);

        // 2. Synchronize Tab View Content when a tab is active
        if let Some(active_tab) = tab_manager.get_active_tab() {
            let is_video = matches!(active_tab.payload, TabPayload::Video(_));
            window.set_viewer_is_video(is_video);
            window.set_viewer_filename(active_tab.filename.clone().into());

            let res_str = if active_tab.record.width > 0 && active_tab.record.height > 0 {
                format!("{} × {}", active_tab.record.width, active_tab.record.height)
            } else {
                String::new()
            };
            window.set_viewer_resolution(res_str.into());

            // Compute adjacent navigation availability
            let (has_prev, has_next) = {
                let st = state.read();
                let current_pos = if !st.active_indices.is_empty() {
                    st.active_indices.iter().position(|&idx| {
                        st.all_folder_media.get(idx).map(|m| m.record.id == active_tab.media_id).unwrap_or(false)
                    })
                } else {
                    st.all_folder_media.iter().position(|m| m.record.id == active_tab.media_id)
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
            }

            // 3. Update active tab media ID in state for inspector and tag panel without destroying Browse selection
            {
                let mut st = state.write();
                st.active_tab_media_id = Some(active_tab.media_id);
            }
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
            StatusBarBridge::update_metrics(window, state);
        } else {
            // 4. Returning to Browse Mode: clear active tab media ID and restore full selection
            window.set_viewer_is_video(false);
            {
                let mut st = state.write();
                st.active_tab_media_id = None;
            }
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
            StatusBarBridge::update_metrics(window, state);
        }
    }
}
