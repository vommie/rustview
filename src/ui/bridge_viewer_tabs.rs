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
use crate::viewer::{HighResImageLoader, TabId, TabPayload, ViewerTabManager};
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::sync::Arc;
use std::time::Instant;

/// Central coordinator for Viewer Tab interactions, asynchronous image loading, and UI synchronization.
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
        window.on_tab_selected(move |tab_id| {
            tracing::info!(tab_id, "[BridgeViewerTabs: Select] Tab selected by user");
            tm_select.set_active_tab(tab_id as TabId);
            Self::sync_ui(&win_select, &tm_select, &lib_select, &set_select, &st_select);
        });

        // 3. Tab Closed (Close button or middle click)
        let win_close = window.clone_strong();
        let tm_close = tab_manager.clone();
        let lib_close = library.clone();
        let st_close = state.clone();
        let set_close = settings.clone();
        window.on_tab_closed(move |tab_id| {
            tracing::info!(tab_id, "[BridgeViewerTabs: Close] Tab closed by user");
            tm_close.close_tab(tab_id as TabId);
            Self::sync_ui(&win_close, &tm_close, &lib_close, &set_close, &st_close);
        });

        // 4. Viewer Toolbar Close Tab Button (triggered via Escape hotkey)
        let win_v_close = window.clone_strong();
        let tm_v_close = tab_manager.clone();
        let lib_v_close = library.clone();
        let st_v_close = state.clone();
        let set_v_close = settings.clone();
        window.on_viewer_close_tab(move || {
            if let Some(active_id) = tm_v_close.active_tab_id() {
                tracing::info!(
                    tab_id = active_id,
                    "[BridgeViewerTabs: ViewerClose] Closed active tab from viewer toolbar"
                );
                tm_v_close.close_tab(active_id);
                Self::sync_ui(&win_v_close, &tm_v_close, &lib_v_close, &set_v_close, &st_v_close);
            }
        });

        // 5. Viewer Toolbar Refresh Button (Reuses the exact same Full Refresh Flow as Context Menu)
        let win_v_refresh = window.clone_strong();
        let tm_v_refresh = tab_manager.clone();
        let lib_v_refresh = library.clone();
        let thumb_v_refresh = thumbnails;
        let set_v_refresh = settings.clone();
        let st_v_refresh = state.clone();
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
        });

        // 6. Previous / Next Image Navigation in active list (In-place within current tab)
        let win_prev = window.clone_strong();
        let tm_prev = tab_manager.clone();
        let lib_prev = library.clone();
        let st_prev = state.clone();
        let set_prev = settings.clone();
        window.on_viewer_previous(move || {
            Self::navigate_adjacent(&win_prev, &tm_prev, &lib_prev, &set_prev, &st_prev, false);
        });

        let win_next = window.clone_strong();
        let tm_next = tab_manager;
        let lib_next = library;
        let st_next = state;
        let set_next = settings;
        window.on_viewer_next(move || {
            Self::navigate_adjacent(&win_next, &tm_next, &lib_next, &set_next, &st_next, true);
        });
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

    /// Navigates to the adjacent (previous or next) image in the current active list while keeping the same tab open in-place.
    pub fn navigate_adjacent(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
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

                    let tm_async = tab_manager.clone();
                    let w_weak = window.as_weak();
                    let lib_async = library.clone();
                    let st_async = state.clone();
                    let set_async = settings.clone();

                    if let Ok(file_path) = library.resolve_media_path(target_id) {
                        rayon::spawn(move || {
                            let load_res = HighResImageLoader::load_full_resolution(&file_path);

                            let _ = w_weak.upgrade_in_event_loop(move |w| {
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
                Err(e) => {
                    tracing::error!(target_id, error = %e, "[BridgeViewerTabs: NavigateError] Failed to navigate tab to media");
                }
            }
        }
    }

    /// Opens a media file in a viewer tab and asynchronously decodes the high-resolution image in the background.
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

                if is_new {
                    let tm_async = tab_manager.clone();
                    let w_weak = window.as_weak();
                    let lib_async = library.clone();
                    let st_async = state.clone();
                    let set_async = settings.clone();

                    if let Ok(file_path) = library.resolve_media_path(media_id) {
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
                }

                tracing::info!(
                    media_id,
                    tab_id,
                    is_new,
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

            if let TabPayload::Image(ref img) = active_tab.payload {
                window.set_viewer_is_loading(img.is_loading);
                window.set_viewer_error(img.error_message.clone().into());

                if let Some(ref buf) = img.high_res_pixels {
                    window.set_viewer_image(Image::from_rgb8(buf.clone()));
                } else {
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
