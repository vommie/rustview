//! Moving between tabs and within a tab: cycling, scrolling into view, previous/next medium, refresh and video embedding.

use super::BridgeViewerTabs;
use crate::ui::MainWindow;
use crate::ui::media::BridgeMedia;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::multiview::tab_host;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::dnd::TabDndGeometry;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use std::sync::Arc;
use std::time::Instant;

impl BridgeViewerTabs {
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
        if let Some((tab_x, tab_w)) = TabDndGeometry::find_tab_slot_geometry(&tabs, active_id, show_rating) {
            tracing::debug!(
                active_id,
                tab_x,
                tab_w,
                "[BridgeViewerTabs: ScrollIntoView] Scrolling tab bar to ensure active tab is visible"
            );
            window.invoke_scroll_tab_into_view(tab_x, tab_w);
        }
    }

    /// Puts the active tab's Multiview in the viewer area (or lets go of it in the Browse view and the tab grid) and hides the
    /// native video windows of every other tab.
    pub fn sync_video_embedding(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        fullscreen: &MultiviewHandle,
        _settings: &Arc<SettingsManager>,
    ) {
        // Only a fullscreen that is isolated from the Browse view (it shows a tab's own Multiview) owns the video windows; a
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

        // Tab view: every tab is a Multiview (one tile for a tab of one medium); the active one is placed in the viewer area.
        let active_tab = tab_manager.get_active_tab();
        tab_manager.hide_video_subwindows_except(active_tab.as_ref().map(|t| t.id));
        match active_tab {
            Some(tab) => tab_host::sync(window, tab_manager, &tab.multiview.instance),
            None => tab_host::suspend(),
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

        BridgeMedia::refresh_selected_media(window, library, thumbnails, tab_manager, settings, state);
    }

    /// Navigates to the adjacent (previous or next) media item in the current active list while keeping the same tab open in-place.
    /// Only a tab of exactly one medium pages (the toolbar buttons and the arrow keys); the title of the tab stays as it was.
    pub fn navigate_adjacent(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        fullscreen: &MultiviewHandle,
        is_next: bool,
    ) {
        tab_manager.sync_tab_identities();
        let Some(active_media_id) = tab_manager.get_active_media_id() else {
            return;
        };

        let target_media_id_opt = {
            let st = state.read();
            let current_pos = st.row_of(active_media_id);

            if let Some(pos) = current_pos {
                let target_pos = if is_next {
                    pos.checked_add(1)
                } else {
                    pos.checked_sub(1)
                };

                target_pos.and_then(|t_idx| {
                    if st.active_indices.is_empty() {
                        st.all_folder_media.get(t_idx).map(|m| m.record.id)
                    } else {
                        st.active_indices
                            .get(t_idx)
                            .and_then(|&idx| st.all_folder_media.get(idx))
                            .map(|m| m.record.id)
                    }
                })
            } else {
                None
            }
        };

        let Some(target_id) = target_media_id_opt else {
            return;
        };
        tracing::info!(
            active_media_id,
            target_media_id = target_id,
            is_next,
            "[BridgeViewerTabs: Navigate] Navigating in-place within current active tab session"
        );

        match tab_manager.navigate_active_tab_to_media(target_id) {
            Ok(_) => {
                Self::sync_ui(window, tab_manager, library, settings, state);
                fullscreen.sync_tab(tab_manager);
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
