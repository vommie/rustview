use super::bridge_image_fullscreen::ImageFullscreenDialogController;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::tab_grid::{MasonryRect, ResponsiveBreakpoints, TabGridMasonryEngine, TabGridTile};
use super::{MainWindow, TabGridItemModel};
use crate::library::Library;
use crate::settings::SettingsManager;
use crate::viewer::{
    get_slint_window_xid, HighResImageLoader, SubwindowGeometry, TabId, TabPayload,
    ViewerTabManager, ViewportBounds, X11VideoSubwindow,
};
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::sync::Arc;
use std::time::Instant;

/// Central coordinator bridge for the TabGridView (Masonry View for Open Tabs).
pub struct BridgeTabGrid;

impl BridgeTabGrid {
    /// Synchronizes open tabs into the Slint `TabGridView` Masonry layout, and positions live video subwindows on tiles.
    pub fn sync_tab_grid_ui(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t0 = Instant::now();
        let sessions = tab_manager.list_tabs();
        let total_tabs = sessions.len();

        let app_settings = settings.get();
        let is_auto_cols = state.read().tab_grid_auto_columns;
        let manual_cols = state.read().tab_grid_columns;

        let win_size = window.window().size();
        let scale = window.window().scale_factor();

        // In Tab-Grid mode (mode 2), both sidebars and resizers are hidden.
        // The available viewport width is therefore the full window width in logical pixels.
        let viewport_w = (win_size.width as f32 / scale).max(100.0);

        let effective_cols = if is_auto_cols {
            ResponsiveBreakpoints::compute_auto_columns(viewport_w)
        } else {
            manual_cols
        };

        window.set_tab_grid_columns(effective_cols as i32);
        window.set_tab_grid_auto_columns(is_auto_cols);

        if total_tabs == 0 || window.get_active_view_mode() != 2 {
            if total_tabs == 0 {
                window.set_tab_grid_items(ModelRc::new(VecModel::from(vec![])));
                window.set_tab_grid_content_height(0.0_f32);
            }
            {
                let mut st = state.write();
                st.cached_tab_grid_rects.clear();
                st.cached_tab_grid_total_h = 0.0;
            }
            tab_manager.hide_all_video_subwindows();
            StatusBarBridge::update_metrics(window, state);
            return;
        }

        // 1. Collect aspect ratios for each open tab
        let aspect_ratios: Vec<f32> = sessions
            .iter()
            .map(|s| {
                let is_video = matches!(s.payload, TabPayload::Video(_));
                let (w, h) = match s.payload {
                    TabPayload::Image(ref img) => (img.orig_width, img.orig_height),
                    TabPayload::Video(_) => (s.record.width, s.record.height),
                };
                TabGridTile::compute_aspect_ratio(w, h, is_video)
            })
            .collect();

        // 2. Compute Masonry Coordinates
        let (p_h, p_v, spacing, padding) = (
            app_settings.appearance.grid_item_padding_horizontal as f32,
            app_settings.appearance.grid_item_padding_vertical as f32,
            app_settings.appearance.grid_spacing as f32,
            app_settings.appearance.grid_padding as f32,
        );

        let (rects, total_h) = TabGridMasonryEngine::compute_layout(
            &aspect_ratios,
            viewport_w,
            effective_cols,
            padding,
            spacing,
            p_v,
            26.0,
        );

        {
            let mut st = state.write();
            st.cached_tab_grid_rects = rects.clone();
            st.cached_tab_grid_total_h = total_h;
        }

        // 3. Assemble Slint Models for image & video tabs
        let mut missing_image_tasks = Vec::new();

        let slint_items: Vec<TabGridItemModel> = sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                let r = &rects[idx];
                let is_video = matches!(s.payload, TabPayload::Video(_));

                let (slint_img, is_loading, err_msg, orig_w, orig_h) = match s.payload {
                    TabPayload::Image(ref img) => {
                        if img.high_res_pixels.is_none() && !img.is_loading && img.error_message.is_empty() {
                            missing_image_tasks.push((s.id, s.media_id, s.file_path.clone()));
                        }
                        let image_obj = if let Some(ref buf) = img.high_res_pixels {
                            Image::from_rgb8(buf.clone())
                        } else {
                            Image::default()
                        };
                        (image_obj, img.is_loading, img.error_message.clone(), img.orig_width, img.orig_height)
                    }
                    TabPayload::Video(ref vid) => {
                        (Image::default(), vid.is_loading, vid.error_message.clone(), s.record.width, s.record.height)
                    }
                };

                TabGridItemModel {
                    id: s.id as i32,
                    media_id: s.media_id as i32,
                    title: s.title.clone().into(),
                    format_str: s.format.default_extension().into(),
                    is_video,
                    rating: s.record.rating as i32,
                    has_tags: false,
                    image: slint_img,
                    is_loading,
                    error_message: err_msg.into(),
                    orig_width: orig_w as i32,
                    orig_height: orig_h as i32,
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                }
            })
            .collect();

        window.set_tab_grid_items(ModelRc::new(VecModel::from(slint_items)));
        window.set_tab_grid_content_height(total_h);

        // 4. Synchronize video subwindows directly on Masonry tiles
        let scroll_y = window.get_tab_grid_scroll_y();
        Self::sync_video_subwindows(
            window,
            tab_manager,
            &rects,
            scroll_y,
            p_h,
            p_v,
        );

        // 5. Trigger asynchronous loads for image buffers if needed
        for (tab_id, media_id, path) in missing_image_tasks {
            let tm_c = tab_manager.clone();
            let lib_c = library.clone();
            let set_c = settings.clone();
            let st_c = state.clone();
            let w_weak = window.as_weak();

            rayon::spawn(move || {
                let load_res = HighResImageLoader::load_full_resolution(&path);
                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    // Discard stale image decodes if the tab navigated away before completion
                    if tm_c.get_tab(tab_id).map(|t| t.media_id) != Some(media_id) {
                        tracing::debug!(
                            media_id,
                            tab_id,
                            "[TabGrid: DiscardStale] Discarding stale decoded image buffer because tab changed"
                        );
                        return;
                    }

                    match load_res {
                        Ok((w_px, h_px, pixel_buffer)) => {
                            tm_c.update_tab_image_buffer(tab_id, w_px, h_px, pixel_buffer);
                        }
                        Err(err) => {
                            tm_c.set_tab_error(tab_id, err.to_string());
                        }
                    }
                    Self::sync_tab_grid_ui(&w, &tm_c, &lib_c, &set_c, &st_c);
                });
            });
        }

        tracing::info!(
            total_tabs,
            viewport_width = viewport_w,
            columns = effective_cols,
            auto_columns = is_auto_cols,
            total_content_height_px = total_h,
            elapsed_us = t0.elapsed().as_micros(),
            "[TabGrid: Sync] Masonry Tab Grid layout computed and synchronized across full viewport width"
        );
    }

    /// Positions and renders running MPV video subwindows onto their corresponding Masonry tiles in the TabGrid.
    /// Clamps physical subwindow coordinates so the subwindow never renders above or below the viewport boundaries.
    pub fn sync_video_subwindows(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        rects: &[MasonryRect],
        scroll_y: f32,
        padding_h: f32,
        padding_v: f32,
    ) {
        if window.get_active_view_mode() != 2 {
            tab_manager.hide_all_video_subwindows();
            return;
        }

        let Some(main_xid) = get_slint_window_xid(window.window()) else {
            return;
        };

        let sessions = tab_manager.list_tabs();
        if sessions.is_empty() {
            tab_manager.hide_all_video_subwindows();
            return;
        }

        let scale = window.window().scale_factor();
        let win_size = window.window().size();

        let toolbar_h = 38.0f32; // TopToolbar
        let tab_grid_toolbar_h = 26.0f32; // TabGridPanelToolbar
        let content_top_y = toolbar_h + tab_grid_toolbar_h; // 64px
        let status_bar_h = 28.0f32;

        // In Tab-Grid mode (mode 2), both sidebars are hidden -> full viewport width is available
        let viewport_w = (win_size.width as f32 / scale).max(100.0);

        let viewport = ViewportBounds {
            x: 0,
            y: (content_top_y * scale).round() as i32,
            width: (viewport_w * scale).round().max(16.0) as u32,
            height: (((win_size.height as f32 / scale) - (content_top_y + status_bar_h)) * scale)
                .round()
                .max(16.0) as u32,
        };

        for (idx, session) in sessions.iter().enumerate() {
            if let TabPayload::Video(ref vid) = session.payload {
                let Some(r) = rects.get(idx) else {
                    let mut sub_guard = vid.subwindow.lock();
                    if let Some(ref mut sub) = *sub_guard {
                        sub.hide();
                    }
                    continue;
                };

                let header_h = 26.0f32;
                let video_logical_x = r.x + padding_h;
                let video_logical_y = content_top_y + r.y + header_h + padding_v + scroll_y;
                let video_logical_w = (r.width - (padding_h * 2.0)).max(16.0);
                let video_logical_h = (r.height - header_h - (padding_v * 2.0)).max(16.0);

                let geom = SubwindowGeometry {
                    x: (video_logical_x * scale).round() as i32,
                    y: (video_logical_y * scale).round() as i32,
                    width: (video_logical_w * scale).round().max(16.0) as u32,
                    height: (video_logical_h * scale).round().max(16.0) as u32,
                };

                // Wire TabGrid click and wheel traps on the video player
                let tab_id = session.id;
                let win_left = window.as_weak();
                vid.player.on_grid_left_click(move || {
                    let _ = win_left.upgrade_in_event_loop(move |w| {
                        w.invoke_tab_grid_item_left_clicked(tab_id as i32);
                    });
                });

                let win_mid = window.as_weak();
                vid.player.on_grid_middle_click(move || {
                    let _ = win_mid.upgrade_in_event_loop(move |w| {
                        w.invoke_tab_grid_item_middle_clicked(tab_id as i32);
                    });
                });

                let win_wheel_up = window.as_weak();
                vid.player.on_grid_wheel_up(move || {
                    let _ = win_wheel_up.upgrade_in_event_loop(move |w| {
                        w.invoke_scroll_media_by(40.0);
                    });
                });

                let win_wheel_dn = window.as_weak();
                vid.player.on_grid_wheel_down(move || {
                    let _ = win_wheel_dn.upgrade_in_event_loop(move |w| {
                        w.invoke_scroll_media_by(-40.0);
                    });
                });

                vid.player.set_tab_grid_mode(true);

                let mut sub_guard = vid.subwindow.lock();
                if let Some(ref mut sub) = *sub_guard {
                    if sub.parent_xid() != main_xid {
                        let _ = sub.reparent(main_xid, geom.x, geom.y, geom.width, geom.height);
                    }
                    let is_visible = sub.update_geometry_and_clip(geom, viewport);

                    tracing::debug!(
                        tab_id = session.id,
                        media_id = session.media_id,
                        phys_x = geom.x,
                        phys_y = geom.y,
                        phys_w = geom.width,
                        phys_h = geom.height,
                        is_visible,
                        "[TabGrid: VideoSubwindow] Real-time subwindow positioning and viewport clamping updated"
                    );
                } else if let Ok(mut sub) = X11VideoSubwindow::new(main_xid, geom.x, geom.y, geom.width, geom.height) {
                    let wid = sub.xid();
                    let _ = vid.player.attach_to_window(wid);
                    sub.update_geometry_and_clip(geom, viewport);
                    *sub_guard = Some(sub);

                    tracing::info!(
                        tab_id = session.id,
                        media_id = session.media_id,
                        subwindow_xid = wid,
                        phys_x = geom.x,
                        phys_y = geom.y,
                        phys_w = geom.width,
                        phys_h = geom.height,
                        "[TabGrid: VideoSubwindow] Created, attached, positioned and clamped video subwindow on Masonry tile"
                    );
                }
            }
        }
    }

    /// Handles Left-Click on a TabGrid item: switches to Tab Viewer view (mode 1) and foregrounds the tab (Requirement 5: seamless playback continuation).
    pub fn handle_item_left_clicked(
        tab_id: TabId,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        fullscreen_controller: &ImageFullscreenDialogController,
    ) {
        tracing::info!(
            tab_id,
            "[TabGrid: LeftClick] Opening selected tab in Tab Viewer mode"
        );

        // Hide all other video subwindows that were mapped on TabGrid tiles
        tab_manager.hide_video_subwindows_except(Some(tab_id));

        tab_manager.set_active_tab_internal(tab_id, true);
        window.set_active_view_mode(1);
        window.set_is_browse_active(false);
        window.set_is_tab_grid_active(false);

        BridgeViewerTabs::sync_ui(window, tab_manager, library, settings, state);
        BridgeViewerTabs::sync_video_embedding(window, tab_manager, fullscreen_controller, settings);
    }

    /// Handles Middle-Click on a TabGrid item: launches Fullscreen Viewer directly for that tab without changing active tab (Requirement 3: auto-plays).
    pub fn handle_item_middle_clicked(
        tab_id: TabId,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        fullscreen_controller: &ImageFullscreenDialogController,
    ) {
        tracing::info!(
            tab_id,
            "[TabGrid: MiddleClick] Launching fullscreen for media item directly from TabGrid (Requirement 3: auto-plays)"
        );

        fullscreen_controller.show_for_tab(tab_id, tab_manager, settings, window.as_weak());
    }

    /// Handles Close Button on a TabGrid item.
    pub fn handle_item_close_clicked(
        tab_id: TabId,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        tracing::info!(
            tab_id,
            "[TabGrid: CloseItem] Closing tab from TabGrid tile"
        );

        tab_manager.close_tab(tab_id);
        BridgeViewerTabs::sync_ui(window, tab_manager, library, settings, state);
        Self::sync_tab_grid_ui(window, tab_manager, library, settings, state);
    }

    /// Handles real-time scrolling in the TabGridView to keep video subwindows positioned and clamped in lockstep with UI tiles.
    pub fn handle_scrolled(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if window.get_active_view_mode() != 2 {
            tab_manager.hide_all_video_subwindows();
            return;
        }

        let app_settings = settings.get();
        let (p_h, p_v) = (
            app_settings.appearance.grid_item_padding_horizontal as f32,
            app_settings.appearance.grid_item_padding_vertical as f32,
        );

        let cached_rects = state.read().cached_tab_grid_rects.clone();
        let scroll_y = window.get_tab_grid_scroll_y();

        if !cached_rects.is_empty() {
            Self::sync_video_subwindows(
                window,
                tab_manager,
                &cached_rects,
                scroll_y,
                p_h,
                p_v,
            );
        } else {
            let sessions = tab_manager.list_tabs();
            if sessions.is_empty() {
                tab_manager.hide_all_video_subwindows();
                return;
            }

            let is_auto_cols = state.read().tab_grid_auto_columns;
            let manual_cols = state.read().tab_grid_columns;
            let win_size = window.window().size();
            let scale = window.window().scale_factor();

            // In Tab-Grid mode (mode 2), both sidebars are hidden -> full viewport width is available
            let viewport_w = (win_size.width as f32 / scale).max(100.0);

            let effective_cols = if is_auto_cols {
                ResponsiveBreakpoints::compute_auto_columns(viewport_w)
            } else {
                manual_cols
            };

            let aspect_ratios: Vec<f32> = sessions
                .iter()
                .map(|s| {
                    let is_video = matches!(s.payload, TabPayload::Video(_));
                    let (w, h) = match s.payload {
                        TabPayload::Image(ref img) => (img.orig_width, img.orig_height),
                        TabPayload::Video(_) => (s.record.width, s.record.height),
                    };
                    TabGridTile::compute_aspect_ratio(w, h, is_video)
                })
                .collect();

            let (spacing, padding) = (
                app_settings.appearance.grid_spacing as f32,
                app_settings.appearance.grid_padding as f32,
            );

            let (rects, total_h) = TabGridMasonryEngine::compute_layout(
                &aspect_ratios,
                viewport_w,
                effective_cols,
                padding,
                spacing,
                p_v,
                26.0,
            );

            {
                let mut st = state.write();
                st.cached_tab_grid_rects = rects.clone();
                st.cached_tab_grid_total_h = total_h;
            }

            Self::sync_video_subwindows(
                window,
                tab_manager,
                &rects,
                scroll_y,
                p_h,
                p_v,
            );
        }
    }
}
