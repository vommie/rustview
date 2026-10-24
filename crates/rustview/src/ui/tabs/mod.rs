pub mod dnd;
pub mod grid;
pub mod grid_bridge;
pub mod icon;
pub mod merge_limit;
pub mod tab_selection;
pub mod unsaved_marks;

use crate::ui::media::inspector::BridgeInspector;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::dnd::TabDndAutoScroller;
use crate::ui::tabs::icon::tab_icon;
use crate::ui::tags::BridgeTags;
use crate::ui::{MainWindow, TabItemModel};
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::settings::SettingsManager;
use rustview_core::util::cast::WrappingAs;
use slint::{ModelRc, VecModel};
use std::sync::Arc;

mod callbacks;
mod drag;
mod merge;
mod navigation;
mod open;
mod selection_ui;

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
        // Tiles come and go inside a tab: its description follows what it shows now.
        tab_manager.sync_tab_identities();
        let tabs = tab_manager.list_tabs();
        let selection = {
            let ids: Vec<TabId> = tabs.iter().map(|t| t.id).collect();
            let mut st = state.write();
            st.tab_selection.retain_existing(&ids);
            st.tab_selection.clone()
        };
        let active_id_opt = tab_manager.active_tab_id();
        let is_tab_grid = window.get_is_tab_grid_active();
        let is_browse = !is_tab_grid && active_id_opt.is_none();

        // 1. Prepare Tab Models for the Top Toolbar
        let tab_models: Vec<TabItemModel> = tabs
            .iter()
            .map(|t| {
                // Taken before the struct literal: a lock inside it would be held until the literal is built, and `tab_icon`
                // takes the same lock.
                let has_unsaved = t.multiview.instance.lock().has_unsaved_edits();
                TabItemModel {
                    id: t.id.wrapping_as::<i32>(),
                    media_id: t.media_id.wrapping_as::<i32>(),
                    title: t.title.clone().into(),
                    format_str: t.format.default_extension().into(),
                    is_active: !is_tab_grid && Some(t.id) == active_id_opt,
                    is_selected: selection.contains(t.id),
                    icon: tab_icon(t),
                    rating: i32::from(t.record.rating),
                    has_tags: false,
                    shift_x: 0.0,
                    is_placeholder: false,
                    has_unsaved,
                }
            })
            .collect();

        window.set_tabs(ModelRc::new(VecModel::from(tab_models)));
        window.set_has_tab_selection(!selection.is_empty());
        window.set_is_browse_active(is_browse);
        window.set_is_tab_grid_active(is_tab_grid);

        let target_mode = if is_tab_grid { 2 } else { i32::from(!is_browse) };
        window.set_active_view_mode(target_mode);

        let bg_mode_idx = match settings.get().viewer.background_mode.to_lowercase().as_str() {
            "deep black" | "black" => 1,
            "transparent checkerboard" | "checkerboard" => 2,
            _ => 0,
        };
        window.set_viewer_background_mode(bg_mode_idx);

        // 2. Synchronize the toolbar of the tab view: title, size, previous/next of a tab of one medium
        if target_mode == 1
            && let Some(active_tab) = tab_manager.get_active_tab()
        {
            window.set_viewer_tab_id(active_tab.id.wrapping_as::<i32>());
            window.set_viewer_filename(active_tab.title.clone().into());

            let solo = active_tab.single_media_id();
            let (display_w, display_h) = if solo.is_some() {
                (active_tab.record.width, active_tab.record.height)
            } else {
                active_tab.display_size()
            };
            let res_str = if display_w > 0 && display_h > 0 {
                format!("{display_w} x {display_h}")
            } else {
                String::new()
            };
            window.set_viewer_resolution(res_str.into());

            Self::sync_panels(window, tab_manager, library, state);

            Self::scroll_active_tab_into_view(window, tab_manager, settings);
        } else {
            window.set_viewer_tab_id(-1);
            {
                let mut st = state.write();
                st.active_tab_media_ids.clear();
                st.active_tab_needs_selection = false;
            }
            window.set_tab_needs_selection(false);
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
            StatusBarBridge::update_metrics(window, state);
        }
    }

    /// The panels (inspector, Tag & Rate, tag history, status line) follow the selected medium of the shown tab: after the tab
    /// changed, and when another medium of its Multiview is selected. A tab of several media without a selected one leaves
    /// them asking for a selection. UI thread only; no lock may be held by the caller.
    pub fn sync_panels(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        library: &Library,
        state: &RwLock<UiState>,
    ) {
        let target = tab_manager.get_active_tab().map(|tab| tab.panel_media_id());
        {
            let mut st = state.write();
            st.active_tab_media_ids = target.flatten().into_iter().collect();
            st.active_tab_needs_selection = matches!(target, Some(None));
        }
        window.set_tab_needs_selection(matches!(target, Some(None)));
        tracing::debug!(
            selected = ?target.flatten(),
            needs_selection = matches!(target, Some(None)),
            "[BridgeViewerTabs: Panels] The panels follow the selected medium of the tab"
        );
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
    }
}
