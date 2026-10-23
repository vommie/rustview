use super::multiview::MultiviewHandle;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::multiview::grid_host::{GridHost, GridItemView};
use super::tab_grid::canvas::canvas_rect;
use super::tab_grid::{MasonryRect, ResponsiveBreakpoints, TabGridMasonryEngine, TabGridTile};
use super::tab_icon::tab_icon;
use super::{MainWindow, TabGridItemModel};
use rustview_core::library::Library;
use rustview_core::settings::SettingsManager;
use rustview_core::settings::schema::TAB_GRID_ITEM_HEADER_LOGICAL_PX;
use crate::viewer::multiview::model::TileKind;
use crate::viewer::{TabId, ViewerTabManager};
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
        _library: &Arc<Library>,
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
        let aspect_ratios: Vec<f32> = sessions.iter().map(TabGridTile::aspect_of).collect();

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
            TAB_GRID_ITEM_HEADER_LOGICAL_PX,
        );

        {
            let mut st = state.write();
            st.cached_tab_grid_rects = rects.clone();
            st.cached_tab_grid_total_h = total_h;
        }

        // 3. Assemble the Slint models: every tab is a Multiview, drawn as a miniature of its arrangement
        let scroll_y = window.get_tab_grid_scroll_y();
        let grid_host = GridHost::new(window, tab_manager);

        let slint_items: Vec<TabGridItemModel> = sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                let r = &rects[idx];
                let solo_kind = s.multiview.instance.lock().solo_tile().map(|(_, kind, _)| kind);
                let (width, height) = s.display_size();
                let multiview_item = grid_host
                    .as_ref()
                    .map(|host| host.place(&s.multiview.instance, canvas_rect(r, p_h, p_v, scroll_y)))
                    .unwrap_or_else(|| GridItemView {
                        tiles: Vec::new(),
                        gutters: Vec::new(),
                        screen: Default::default(),
                    });

                TabGridItemModel {
                    id: s.id as i32,
                    media_id: s.media_id as i32,
                    title: s.title.clone().into(),
                    format_str: s.format.default_extension().into(),
                    is_video: solo_kind == Some(TileKind::Video),
                    is_multiview: true,
                    icon: tab_icon(s),
                    rating: s.record.rating as i32,
                    has_tags: false,
                    image: Image::default(),
                    is_loading: false,
                    error_message: String::new().into(),
                    orig_width: width as i32,
                    orig_height: height as i32,
                    mv_tiles: ModelRc::new(VecModel::from(multiview_item.tiles)),
                    mv_gutters: ModelRc::new(VecModel::from(multiview_item.gutters)),
                    mv_screen: multiview_item.screen,
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                }
            })
            .collect();

        window.set_tab_grid_items(ModelRc::new(VecModel::from(slint_items)));
        window.set_tab_grid_content_height(total_h);

        // 4. Place the Multiviews (their video windows included) on their tiles
        Self::sync_video_subwindows(window, tab_manager, &rects, scroll_y, p_h, p_v);

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

    /// Places the Multiview of every tab on its Masonry tile (the video windows of its tiles follow, clamped to the viewport and
    /// cut out where menus and tooltips are), and hides the Multiviews of tabs that have no tile.
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

        let Some(grid_host) = GridHost::new(window, tab_manager) else {
            return;
        };

        for (idx, session) in tab_manager.list_tabs().iter().enumerate() {
            match rects.get(idx) {
                Some(r) => {
                    grid_host.place(&session.multiview.instance, canvas_rect(r, padding_h, padding_v, scroll_y));
                }
                None => session.multiview.instance.lock().deactivate(),
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
        fullscreen: &MultiviewHandle,
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
        BridgeViewerTabs::sync_video_embedding(window, tab_manager, fullscreen, settings);
    }

    /// Handles Middle-Click on a TabGrid item: launches Fullscreen Viewer directly for that tab without changing active tab (Requirement 3: auto-plays).
    pub fn handle_item_middle_clicked(
        tab_id: TabId,
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        fullscreen: &MultiviewHandle,
    ) {
        tracing::info!(
            tab_id,
            "[TabGrid: MiddleClick] Launching fullscreen for media item directly from TabGrid (Requirement 3: auto-plays)"
        );

        fullscreen.open_tab(tab_id, tab_manager, settings, window.as_weak());
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

            let aspect_ratios: Vec<f32> = sessions.iter().map(TabGridTile::aspect_of).collect();

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
                TAB_GRID_ITEM_HEADER_LOGICAL_PX,
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
