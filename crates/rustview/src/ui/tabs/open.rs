//! Opening media in tabs (foreground and background).

use super::BridgeViewerTabs;
use crate::ui::MainWindow;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::MediaId;
use rustview_core::media::MediaType;
use rustview_core::settings::SettingsManager;
use std::sync::Arc;
use std::time::Instant;

impl BridgeViewerTabs {
    /// Whether more than one tab may show `media_id` (the settings of the viewer for images and videos).
    pub(super) fn allow_multiple_tabs(
        media_id: MediaId,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) -> bool {
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
        let viewer = settings.get().viewer;
        if is_video {
            viewer.videos.allow_multiple_tabs
        } else {
            viewer.images.allow_multiple_tabs
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
            Self::open_media_tab(media_ids[0], window, tab_manager, library, settings, state, fullscreen);
            return;
        }

        let t0 = Instant::now();
        let total = media_ids.len();

        tracing::info!(
            total_items = total,
            "[BridgeViewerTabs: OpenMultipleForeground] Opening {} media tabs with tail focus activation...",
            total
        );

        // 1. Open items 0..(total - 1) in background
        for &media_id in &media_ids[..total - 1] {
            let allow_multiple = Self::allow_multiple_tabs(media_id, library, settings, state);
            if let Err(e) = tab_manager.open_media_in_background(media_id, allow_multiple) {
                tracing::error!(media_id, error = %e, "[BridgeViewerTabs: OpenMultipleError] Failed to open a background media tab");
            }
        }

        // 2. Open the last item in foreground
        let last_id = media_ids[total - 1];
        let last_allow_multiple = Self::allow_multiple_tabs(last_id, library, settings, state);

        match tab_manager.open_or_focus_media(last_id, last_allow_multiple) {
            Ok(_) => {
                window.set_is_tab_grid_active(false);
                window.set_is_browse_active(false);
                window.set_active_view_mode(1);

                Self::sync_ui(window, tab_manager, library, settings, state);
                Self::sync_video_embedding(window, tab_manager, fullscreen, settings);
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
        let last_title = ViewerTabManager::compute_tab_title(&all_tabs, tab_manager.active_tab_id().unwrap_or(0));

        StatusBarBridge::success(window, format!("Opened {total} tabs (Foreground: '{last_title}')"));

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

        tracing::info!(
            total_items = total,
            "[BridgeViewerTabs: OpenMultipleBackground] Opening {} media tabs in background...",
            total
        );

        let mut opened_count = 0usize;

        for &media_id in media_ids {
            let allow_multiple = Self::allow_multiple_tabs(media_id, library, settings, state);
            if tab_manager.open_media_in_background(media_id, allow_multiple).is_ok() {
                opened_count += 1;
            }
        }

        Self::sync_ui(window, tab_manager, library, settings, state);

        if window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(window, tab_manager, settings, state);
        }

        StatusBarBridge::success(window, format!("Opened {opened_count} tabs in background"));

        tracing::info!(
            opened_count,
            total_requested = total,
            elapsed_us = t0.elapsed().as_micros(),
            "[BridgeViewerTabs: OpenMultipleBackground] Finished opening tabs in background"
        );
    }

    /// Opens a medium in a viewer tab (a Multiview of one tile, started when the tab is shown) and shows the tab.
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
        let allow_multiple = Self::allow_multiple_tabs(media_id, library, settings, state);

        match tab_manager.open_or_focus_media(media_id, allow_multiple) {
            Ok((tab_id, is_new)) => {
                window.set_is_tab_grid_active(false);
                window.set_is_browse_active(false);
                window.set_active_view_mode(1);
                Self::sync_ui(window, tab_manager, library, settings, state);
                Self::sync_video_embedding(window, tab_manager, fullscreen, settings);

                tracing::info!(
                    media_id,
                    tab_id,
                    is_new,
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
                StatusBarBridge::error(window, format!("Failed to open tab for media #{media_id}: {e}"));
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
        let allow_multiple = Self::allow_multiple_tabs(media_id, library, settings, state);

        match tab_manager.open_media_in_background(media_id, allow_multiple) {
            Ok((tab_id, is_new)) => {
                Self::sync_ui(window, tab_manager, library, settings, state);

                if window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(window, tab_manager, settings, state);
                }

                let all_tabs = tab_manager.list_tabs();
                let tab_title = ViewerTabManager::compute_tab_title(&all_tabs, tab_id);
                StatusBarBridge::success(window, format!("Opened '{tab_title}' in background tab"));

                tracing::info!(
                    media_id,
                    tab_id,
                    is_new,
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
                    format!("Failed to open background tab for media #{media_id}: {e}"),
                );
                None
            }
        }
    }
}
