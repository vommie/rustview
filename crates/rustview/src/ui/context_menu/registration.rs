//! Registering the context menu handlers of every kind of menu.

use super::calculate_all_active_menu_cutouts;
use super::{BridgeContextMenu, FolderMenuContext, MediaMenuContext};
use crate::ui::MainWindow;
use crate::ui::browse_folders::{BrowseFolders, FolderRowRef};
use crate::ui::context_menu::browse::{BrowseContextMenus, BrowseMenuContext};
pub use crate::ui::context_menu::presenter::CtxMenuPresenter;
use crate::ui::favorites::manager::FavoriteManager;
use crate::ui::state::{LastActivePanel, UiState};
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use slint::ComponentHandle;
use std::sync::Arc;

impl BridgeContextMenu {
    pub(super) fn register_media_menu(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let th = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        let lib_sel = library.clone();
        let th_sel = thumbnails.clone();
        let tm_sel = tab_manager.clone();
        let win_sel = window.clone_strong();
        window.on_media_context_menu(move |id, x, y| {
            st.write().last_active_panel = LastActivePanel::Media;
            // A medium that is part of a selection with folders in it: the menu of the whole selection.
            let in_mixed_selection = {
                let st = st.read();
                st.file_selection.is_selected(id as u64) && BrowseContextMenus::is_mixed_selection(&st)
            };
            if in_mixed_selection {
                BrowseContextMenus::show_selection_menu(
                    BrowseMenuContext {
                        window: &win_sel,
                        library: &lib_sel,
                        thumbnails: &th_sel,
                        tab_manager: &tm_sel,
                        state: &st,
                    },
                    x,
                    y,
                );
                return;
            }
            Self::show_media_menu(
                MediaMenuContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &th,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                },
                id as u64,
                x,
                y,
            );
        });
    }

    pub(super) fn register_folder_menu(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        favorites: &Arc<RwLock<FavoriteManager>>,
        tab_manager: &Arc<ViewerTabManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (win_row, lib_row, th_row, set_row, favs_row, tm_row, st_row) = (
            window.clone_strong(),
            library.clone(),
            thumbnails.clone(),
            settings.clone(),
            favorites.clone(),
            tab_manager.clone(),
            state.clone(),
        );
        // A folder row of the Browse view opens exactly the menu of the same folder in the tree ("..." has none); a row inside a
        // selection of several items opens the menu of that selection.
        let (win_ctx, lib_ctx, th_ctx, tm_ctx) = (
            window.clone_strong(),
            library.clone(),
            thumbnails.clone(),
            tab_manager.clone(),
        );
        window.on_folder_row_context_menu(move |id, x, y| {
            let Some(FolderRowRef::Folder(folder_id)) = FolderRowRef::from_view_id(id) else {
                return;
            };
            BrowseFolders::select_row_for_menu(FolderRowRef::Folder(folder_id), &win_row, &lib_row, &st_row);
            if BrowseContextMenus::is_mixed_selection(&st_row.read()) {
                BrowseContextMenus::show_selection_menu(
                    BrowseMenuContext {
                        window: &win_ctx,
                        library: &lib_ctx,
                        thumbnails: &th_ctx,
                        tab_manager: &tm_ctx,
                        state: &st_row,
                    },
                    x,
                    y,
                );
                return;
            }
            Self::show_folder_menu(
                FolderMenuContext {
                    window: &win_row,
                    library: &lib_row,
                    thumbnails: &th_row,
                    settings: &set_row,
                    favorites: &favs_row,
                    tab_manager: &tm_row,
                    state: &st_row,
                },
                folder_id,
                x,
                y,
            );
        });

        let (win_bg, lib_bg, th_bg, tm_bg, st_bg) = (
            window.clone_strong(),
            library.clone(),
            thumbnails.clone(),
            tab_manager.clone(),
            state.clone(),
        );
        window.on_browse_background_context_menu(move |x, y| {
            BrowseContextMenus::show_background_menu(
                BrowseMenuContext {
                    window: &win_bg,
                    library: &lib_bg,
                    thumbnails: &th_bg,
                    tab_manager: &tm_bg,
                    state: &st_bg,
                },
                x,
                y,
            );
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let th = thumbnails.clone();
        let set = settings.clone();
        let favs = favorites.clone();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_folder_context_menu(move |id, x, y| {
            st.write().last_active_panel = LastActivePanel::Folder;
            Self::show_folder_menu(
                FolderMenuContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &th,
                    settings: &set,
                    favorites: &favs,
                    tab_manager: &tm,
                    state: &st,
                },
                id as u64,
                x,
                y,
            );
        });
    }

    pub(super) fn register_tab_menu(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let tm = tab_manager.clone();
        let th = thumbnails.clone();
        let st = state.clone();
        window.on_tab_context_menu(move |id, x, y| {
            Self::show_tab_menu(&win, &tm, &th, &st, id as u64, x, y);
        });
    }

    pub(super) fn register_favorite_menu(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_favorite_context_menu(move |path_str, alias_str, x, y| {
            Self::show_favorite_menu(&win, &tm, &st, path_str.as_str(), alias_str.as_str(), x, y);
        });
    }

    pub(super) fn register_popup_events(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        _state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let tm_sub = tab_manager.clone();

        window.on_context_menu_request_submenu(move |depth, visible_row_y, m_idx| {
            CtxMenuPresenter::handle_submenu_request(&win, None, depth as usize, visible_row_y, m_idx as usize);

            let all_cutouts = calculate_all_active_menu_cutouts(&win);
            tm_sub.set_menu_cutouts(all_cutouts);
        });

        let win = window.clone_strong();
        let tm = tab_manager.clone();
        window.on_context_menu_dismissed(move || {
            CtxMenuPresenter::dismiss(&win);
            tm.clear_menu_cutouts();
        });
    }
}
