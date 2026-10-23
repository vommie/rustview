//! Building and showing the menu of a kind of item.

use super::calculate_all_active_menu_cutouts;
use super::{BridgeContextMenu, FolderMenuContext, MediaMenuContext};
use crate::ctx_menu::CtxMenu;
use crate::system::DesktopIntegration;
use crate::ui::MainWindow;
pub use crate::ui::context_menu::favorite::FavoriteContextMenu;
pub use crate::ui::context_menu::folder::FolderContextMenu;
pub use crate::ui::context_menu::media::MediaContextMenu;
pub use crate::ui::context_menu::presenter::CtxMenuPresenter;
pub use crate::ui::context_menu::tab::TabContextMenu;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::tab_selection::MenuTarget;
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::RwLock;
use rustview_core::library::models::{FolderId, MediaId};
use rustview_core::thumbnails::ThumbnailManager;
use std::sync::Arc;

impl BridgeContextMenu {
    pub fn show_media_menu(ctx: &MediaMenuContext<'_>, media_id: MediaId, x: f32, y: f32) {
        ctx.state.write().active_tab_dialog_target = None;
        let is_busy = BridgeMedia::is_system_busy(ctx.window, ctx.thumbnails, ctx.state);
        let show_hidden = ctx.settings.get().folders.show_hidden_folders;
        let sel_meta = MediaContextMenu::prepare_selection(ctx.window, ctx.library, ctx.state, media_id);

        let snapshot = {
            let existing = ctx.state.read().folder_hierarchy.clone();
            if let Some(s) = existing {
                s
            } else {
                let loaded = ctx.library.folders().load_hierarchy_snapshot().unwrap_or_default();
                let arc_snap = Arc::new(loaded);
                ctx.state.write().folder_hierarchy = Some(arc_snap.clone());
                arc_snap
            }
        };

        let menu = MediaContextMenu::build_menu(&sel_meta, is_busy, Some(&snapshot), show_hidden);
        Self::open_custom_menu(ctx.window, ctx.state, ctx.tab_manager, menu, x, y);
    }

    pub fn show_folder_menu(ctx: &FolderMenuContext<'_>, folder_id: FolderId, x: f32, y: f32) {
        let is_busy = BridgeMedia::is_system_busy(ctx.window, ctx.thumbnails, ctx.state);
        let app_settings = ctx.settings.get();
        let root_setting = app_settings.general.root_folder;
        let show_hidden_folders = app_settings.folders.show_hidden_folders;

        let snapshot = {
            let existing = ctx.state.read().folder_hierarchy.clone();
            if let Some(s) = existing {
                s
            } else {
                let loaded = ctx.library.folders().load_hierarchy_snapshot().unwrap_or_default();
                let arc_snap = Arc::new(loaded);
                ctx.state.write().folder_hierarchy = Some(arc_snap.clone());
                arc_snap
            }
        };

        let Some(target_node) = snapshot.get_node(folder_id).cloned() else {
            tracing::warn!(folder_id, "[ContextMenu: Folder] Folder node not found in snapshot");
            return;
        };

        {
            let mut st = ctx.state.write();
            st.active_folder_dialog_target = Some(folder_id);
            st.active_tab_dialog_target = None;
        }

        let is_root = root_setting.as_ref().is_some_and(|r| target_node.path == *r);

        let is_favorite = ctx.favorites.read().has_path(&target_node.path);

        let menu = FolderContextMenu::build_menu(
            &target_node,
            &snapshot,
            is_root,
            show_hidden_folders,
            is_busy,
            is_favorite,
        );

        Self::open_custom_menu(ctx.window, ctx.state, ctx.tab_manager, menu, x, y);
    }

    pub fn show_tab_menu(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        thumbnails: &ThumbnailManager,
        state: &Arc<RwLock<UiState>>,
        tab_id: TabId,
        x: f32,
        y: f32,
    ) {
        let tabs = tab_manager.list_tabs();
        let total_tabs = tabs.len();
        let Some(tab_idx) = tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };

        // Several selected tabs have a menu of their own; a right click outside the selection drops it
        let (target, selection_changed) = state.write().tab_selection.context_menu(tab_id);
        if selection_changed {
            BridgeViewerTabs::apply_tab_selection(window, state);
        }
        if target == MenuTarget::Selection {
            let order: Vec<TabId> = tabs.iter().map(|t| t.id).collect();
            let titles: Vec<String> = state
                .read()
                .tab_selection
                .in_order(&order)
                .iter()
                .map(|id| ViewerTabManager::compute_tab_title(&tabs, *id))
                .collect();
            state.write().active_tab_dialog_target = Some(tab_id);
            let menu = TabContextMenu::build_selection_menu(&titles);
            Self::open_custom_menu(window, state, tab_manager, menu, x, y + 10.0);
            return;
        }

        let tab = &tabs[tab_idx];
        let is_busy = BridgeMedia::is_system_busy(window, thumbnails, state);

        let associated_apps = if tab.single_media_id().is_some() && tab.file_path.exists() {
            DesktopIntegration::get_associated_apps(&tab.file_path)
        } else {
            Vec::new()
        };

        {
            let mut st = state.write();
            st.active_tab_dialog_target = Some(tab_id);
            st.active_associated_apps.clone_from(&associated_apps);
        }

        let menu = TabContextMenu::build_menu(tab, tab_idx, total_tabs, &tabs, &associated_apps, is_busy);
        Self::open_custom_menu(window, state, tab_manager, menu, x, y + 10.0);
    }

    pub fn show_favorite_menu(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        state: &Arc<RwLock<UiState>>,
        path_str: &str,
        alias_str: &str,
        x: f32,
        y: f32,
    ) {
        let path = std::path::PathBuf::from(path_str);
        let is_valid = path.is_dir();

        {
            let mut st = state.write();
            st.active_favorite_target = Some((alias_str.to_string(), path.clone()));
            st.active_tab_dialog_target = None;
            st.active_folder_dialog_target = None;
        }

        let menu = FavoriteContextMenu::build_menu(alias_str, &path, is_valid);
        Self::open_custom_menu(window, state, tab_manager, menu, x, y);
    }

    #[inline]
    pub fn open_custom_menu(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        tab_manager: &ViewerTabManager,
        menu: CtxMenu,
        x: f32,
        y: f32,
    ) {
        let mut st = state.write();
        CtxMenuPresenter::open(window, &mut st.active_context_menu, menu, x, y);
        let cutouts = calculate_all_active_menu_cutouts(window);
        tab_manager.set_menu_cutouts(cutouts);
    }
}
