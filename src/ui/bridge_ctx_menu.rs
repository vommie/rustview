pub use super::bridge_ctx_menu_favorite::FavoriteContextMenu;
pub use super::bridge_ctx_menu_folder::FolderContextMenu;
pub use super::bridge_ctx_menu_media::{MediaActionContext, MediaContextMenu, MediaSelectionMeta};
pub use super::bridge_ctx_menu_presenter::CtxMenuPresenter;
pub use super::bridge_ctx_menu_tab::{TabActionContext, TabContextMenu};
pub use super::bridge_ctx_menu_tag::TagContextMenu;
pub use super::bridge_file_dialog_conflict::FileOpsConflictDialogController;
pub use super::bridge_file_dialog_delete::DeleteFileDialogController;
pub use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
pub use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
pub use super::bridge_file_dialogs::{FileDialogContext, FileRenameContext, FileRenameDialogController};

use super::bridge_favorite_dialog_rename::FavoriteRenameDialogController;
use super::bridge_favorites::BridgeFavorites;
use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::multiview::MultiviewHandle;
use super::bridge_media::BridgeMedia;
use super::clipboard::ClipboardBridge;
use super::bridge_state::*;
use super::bridge_ctx_menu_browse::{BrowseActionContext, BrowseContextMenus, BrowseMenuContext};
use super::browse_folders::{BrowseFolders, FolderRowRef};
use super::bridge_tag_dialogs::TagDialogController;
use super::favorites::FavoriteManager;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{CtxMenu, CtxMenuDispatcher};
use crate::library::models::{FolderId, MediaId};
use crate::library::Library;
use crate::settings::SettingsManager;
use crate::system::DesktopIntegration;
use crate::thumbnails::ThumbnailManager;
use crate::viewer::{TabId, ViewerTabManager, XRectangle};
use parking_lot::RwLock;
use slint::{ComponentHandle, Model};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub struct MediaMenuContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Library,
    pub thumbnails: &'a ThumbnailManager,
    pub tab_manager: &'a ViewerTabManager,
    pub state: &'a Arc<RwLock<UiState>>,
    pub settings: &'a SettingsManager,
}

pub struct FolderMenuContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Library,
    pub thumbnails: &'a ThumbnailManager,
    pub settings: &'a SettingsManager,
    pub favorites: &'a Arc<RwLock<FavoriteManager>>,
    pub tab_manager: &'a ViewerTabManager,
    pub state: &'a Arc<RwLock<UiState>>,
}

pub struct ContextMenuRegistration {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub tab_manager: Arc<ViewerTabManager>,
    pub favorites: Arc<RwLock<FavoriteManager>>,
    pub state: Arc<RwLock<UiState>>,
    pub dispatcher: Arc<CtxMenuDispatcher>,
    pub rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    pub favorite_rename_dialog: FavoriteRenameDialogController,
    pub missing_file_dialog: MissingFileDialogController,
    pub delete_file_dialog: DeleteFileDialogController,
    pub remove_orphaned_dialog: RemoveOrphanedFileDialogController,
    pub conflict_dialog: FileOpsConflictDialogController,
    pub tag_dialogs: TagDialogController,
    pub folder_dialogs: FolderDialogController,
    pub fullscreen: MultiviewHandle,
    pub settings: Arc<SettingsManager>,
}

pub struct BridgeContextMenu;

impl BridgeContextMenu {
    pub fn register_callbacks(window: &MainWindow, reg: ContextMenuRegistration) {
        Self::register_media_menu(window, &reg.library, &reg.thumbnails, &reg.tab_manager, &reg.settings, &reg.state);
        Self::register_folder_menu(window, &reg.library, &reg.thumbnails, &reg.settings, &reg.favorites, &reg.tab_manager, &reg.state);
        Self::register_tab_menu(window, &reg.tab_manager, &reg.thumbnails, &reg.state);
        Self::register_favorite_menu(window, &reg.tab_manager, &reg.state);
        Self::register_popup_events(window, &reg.tab_manager, &reg.state);
        Self::register_action_dispatcher(window, reg);
    }

    fn register_media_menu(
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
                    BrowseMenuContext { window: &win_sel, library: &lib_sel, thumbnails: &th_sel, tab_manager: &tm_sel, state: &st },
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

    fn register_folder_menu(
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
        let (win_ctx, lib_ctx, th_ctx, tm_ctx) = (window.clone_strong(), library.clone(), thumbnails.clone(), tab_manager.clone());
        window.on_folder_row_context_menu(move |id, x, y| {
            let Some(FolderRowRef::Folder(folder_id)) = FolderRowRef::from_view_id(id) else {
                return;
            };
            BrowseFolders::select_row_for_menu(FolderRowRef::Folder(folder_id), &win_row, &lib_row, &st_row);
            if BrowseContextMenus::is_mixed_selection(&st_row.read()) {
                BrowseContextMenus::show_selection_menu(
                    BrowseMenuContext { window: &win_ctx, library: &lib_ctx, thumbnails: &th_ctx, tab_manager: &tm_ctx, state: &st_row },
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

        let (win_bg, lib_bg, th_bg, tm_bg, st_bg) = (window.clone_strong(), library.clone(), thumbnails.clone(), tab_manager.clone(), state.clone());
        window.on_browse_background_context_menu(move |x, y| {
            BrowseContextMenus::show_background_menu(
                BrowseMenuContext { window: &win_bg, library: &lib_bg, thumbnails: &th_bg, tab_manager: &tm_bg, state: &st_bg },
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

    fn register_tab_menu(
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

    fn register_favorite_menu(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_favorite_context_menu(move |path_str, alias_str, x, y| {
            Self::show_favorite_menu(
                &win,
                &tm,
                &st,
                path_str.as_str(),
                alias_str.as_str(),
                x,
                y,
            );
        });
    }

    fn register_popup_events(
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

    fn register_action_dispatcher(window: &MainWindow, reg: ContextMenuRegistration) {
        let win = window.clone_strong();
        let lib = reg.library;
        let thumb = reg.thumbnails;
        let tab_mgr = reg.tab_manager;
        let favs = reg.favorites;
        let st = reg.state;
        let set = reg.settings;
        let disp = reg.dispatcher;
        let ren_dlg = reg.rename_dialog;
        let fav_ren_dlg = reg.favorite_rename_dialog;
        let missing_file_dlg = reg.missing_file_dialog;
        let delete_file_dlg = reg.delete_file_dialog;
        let remove_orphaned_dlg = reg.remove_orphaned_dialog;
        let conflict_dlg = reg.conflict_dialog;
        let tag_dlg = reg.tag_dialogs;
        let fld_dlg = reg.folder_dialogs;
        let fs_ctrl = reg.fullscreen;

        window.on_context_menu_item_clicked(move |action_id| {
            CtxMenuPresenter::dismiss(&win);
            tab_mgr.clear_menu_cutouts();
            let action_u64 = action_id as u64;

            // 1. Favorites Action Handling (5000..6000)
            if (5000..6000).contains(&action_u64) {
                let target_fav = st.write().active_favorite_target.take();
                FavoriteContextMenu::handle_action(
                    action_u64,
                    &win,
                    &favs,
                    &fav_ren_dlg,
                    target_fav,
                );
                return;
            }

            // 2. Folder Actions: Add to Favorites
            if action_u64 == CTX_FOLDER_ADD_TO_FAVORITES {
                let target_folder_id = st.write().active_folder_dialog_target.take();
                if let Some(fid) = target_folder_id
                    && let Ok(Some(rec)) = lib.folders().get_folder(fid)
                {
                    let path = std::path::PathBuf::from(&rec.path);
                    let _ = BridgeFavorites::add_favorite(&win, &favs, path, None);
                }
                return;
            }

            // 2b. Actions that only the menus of the free space and of a mixed selection have
            if (CTX_BROWSE_SELECTION_OPEN_IN_FILE_MANAGER..=CTX_BROWSE_PASTE).contains(&action_u64) {
                BrowseContextMenus::handle_action(
                    action_u64,
                    &win,
                    BrowseActionContext {
                        library: &lib,
                        thumbnails: &thumb,
                        tab_manager: &tab_mgr,
                        settings: &set,
                        state: &st,
                        favorites: &favs,
                        delete_file_dialog: &delete_file_dlg,
                        remove_orphaned_file_dialog: &remove_orphaned_dlg,
                        folder_dialogs: &fld_dlg,
                        fullscreen: &fs_ctrl,
                    },
                );
                return;
            }

            // 3. Folder Actions: 3000..4000 (Open FM, Rename, Delete, Purge, Refresh) and Move (300_000..500_000)
            if (3000..4000).contains(&action_u64)
                || (CTX_FOLDER_MOVE_BASE..CTX_ACTION_COPY_TO_BASE).contains(&action_u64)
            {
                let target_folder_id = st.write().active_folder_dialog_target.take();
                let ctx = FolderDialogContext {
                    library: lib.clone(),
                    thumbnails: thumb.clone(),
                    tab_manager: tab_mgr.clone(),
                    settings: set.clone(),
                    state: st.clone(),
                };
                FolderContextMenu::handle_action(action_u64, &win, &fld_dlg, ctx, target_folder_id);
                return;
            }

            // 4. Tag Actions: 2000..3000 and Reparent (100_000..300_000)
            if (2000..3000).contains(&action_u64)
                || (CTX_TAG_MOVE_TARGET_BASE..CTX_FOLDER_MOVE_BASE).contains(&action_u64)
            {
                let bundle = tag_dlg.create_action_bundle(&win, &lib, &thumb, &set, &st);
                TagContextMenu::handle_action(action_u64, &win, &lib, &set, &st, bundle);
                return;
            }

            // 5. Tab Context Menu Invocations (4000..5000)
            let active_tab_target = st.write().active_tab_dialog_target.take();
            if let Some(target_tab_id) = active_tab_target {
                if (4000..5000).contains(&action_u64) {
                    TabContextMenu::handle_action(
                        action_u64,
                        TabActionContext {
                            window: &win,
                            tab_manager: &tab_mgr,
                            library: &lib,
                            settings: &set,
                            state: &st,
                            fullscreen: &fs_ctrl,
                        },
                        Some(target_tab_id),
                    );
                    return;
                }

                if let Some(tab) = tab_mgr.get_tab(target_tab_id) {
                    let media_action_ctx = MediaActionContext {
                        library: &lib,
                        thumbnails: &thumb,
                        tab_manager: &tab_mgr,
                        settings: &set,
                        state: &st,
                        dispatcher: &disp,
                        rename_dialog: &ren_dlg,
                        missing_file_dialog: &missing_file_dlg,
                        delete_file_dialog: &delete_file_dlg,
                        remove_orphaned_dialog: &remove_orphaned_dlg,
                        conflict_dialog: &conflict_dlg,
                        fullscreen: &fs_ctrl,
                    };

                    MediaContextMenu::handle_file_action_for_ids(
                        action_u64,
                        &win,
                        &media_action_ctx,
                        &[tab.media_id],
                    );
                    return;
                }
            }

            // 6. Media Grid / Table Selection Actions (including Copy To / Move To)
            MediaContextMenu::handle_action(
                action_u64,
                &win,
                &MediaActionContext {
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tab_mgr,
                    settings: &set,
                    state: &st,
                    dispatcher: &disp,
                    rename_dialog: &ren_dlg,
                    missing_file_dialog: &missing_file_dlg,
                    delete_file_dialog: &delete_file_dlg,
                    remove_orphaned_dialog: &remove_orphaned_dlg,
                    conflict_dialog: &conflict_dlg,
                    fullscreen: &fs_ctrl,
                },
            );
        });
    }

    pub fn show_media_menu(
        ctx: MediaMenuContext<'_>,
        media_id: MediaId,
        x: f32,
        y: f32,
    ) {
        ctx.state.write().active_tab_dialog_target = None;
        let is_busy = BridgeMedia::is_system_busy(ctx.window, ctx.thumbnails, ctx.state);
        let show_hidden = ctx.settings.get().folders.show_hidden_folders;
        let sel_meta = MediaContextMenu::prepare_selection(ctx.window, ctx.library, ctx.state, media_id);

        let snapshot = {
            let existing = ctx.state.read().folder_hierarchy.clone();
            match existing {
                Some(s) => s,
                None => {
                    let loaded = ctx.library.folders().load_hierarchy_snapshot().unwrap_or_default();
                    let arc_snap = Arc::new(loaded);
                    ctx.state.write().folder_hierarchy = Some(arc_snap.clone());
                    arc_snap
                }
            }
        };

        let menu = MediaContextMenu::build_menu(&sel_meta, is_busy, Some(&snapshot), show_hidden);
        Self::open_custom_menu(ctx.window, ctx.state, ctx.tab_manager, menu, x, y);
    }

    pub fn show_folder_menu(
        ctx: FolderMenuContext<'_>,
        folder_id: FolderId,
        x: f32,
        y: f32,
    ) {
        let is_busy = BridgeMedia::is_system_busy(ctx.window, ctx.thumbnails, ctx.state);
        let app_settings = ctx.settings.get();
        let root_setting = app_settings.general.root_folder;
        let show_hidden_folders = app_settings.folders.show_hidden_folders;

        let snapshot = {
            let existing = ctx.state.read().folder_hierarchy.clone();
            match existing {
                Some(s) => s,
                None => {
                    let loaded = ctx.library.folders().load_hierarchy_snapshot().unwrap_or_default();
                    let arc_snap = Arc::new(loaded);
                    ctx.state.write().folder_hierarchy = Some(arc_snap.clone());
                    arc_snap
                }
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

        let is_root = root_setting
            .as_ref()
            .map(|r| target_node.path == *r)
            .unwrap_or(false);

        let is_favorite = ctx.favorites.read().has_path(&target_node.path);

        let menu = FolderContextMenu::build_menu(
            &target_node,
            &snapshot,
            is_root,
            show_hidden_folders,
            is_busy,
            is_favorite,
            ClipboardBridge::folder_entry(ctx.window, ctx.thumbnails, ctx.state, target_node.is_deleted),
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

        let tab = &tabs[tab_idx];
        let is_busy = BridgeMedia::is_system_busy(window, thumbnails, state);

        let associated_apps = if !tab.is_multiview() && tab.file_path.exists() {
            DesktopIntegration::get_associated_apps(&tab.file_path)
        } else {
            Vec::new()
        };

        {
            let mut st = state.write();
            st.active_tab_dialog_target = Some(tab_id);
            st.active_associated_apps = associated_apps.clone();
        }

        let menu = TabContextMenu::build_menu(
            tab,
            tab_idx,
            total_tabs,
            &tabs,
            &associated_apps,
            is_busy,
        );
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

/// Computes MPV cutouts for all currently mapped menu level rectangles.
pub fn calculate_all_active_menu_cutouts(
    window: &MainWindow,
) -> Vec<XRectangle> {
    let scale = window.window().scale_factor();
    let levels = window.get_context_menu_levels();
    let count = levels.row_count();

    if count == 0 {
        return Vec::new();
    }

    let mut all_cutouts = Vec::new();

    for i in 0..count {
        if let Some(lvl) = levels.row_data(i) {
            let h = lvl.max_height;
            let cutouts = make_rounded_cutout_rectangles(lvl.x, lvl.y, lvl.width, h, scale);
            all_cutouts.extend(cutouts);
        }
    }

    all_cutouts
}

pub fn make_rounded_cutout_rectangles(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scale: f32,
) -> Vec<XRectangle> {
    let px = (x * scale).round() as i16;
    let py = (y * scale).round() as i16;
    let pw = (w * scale).round() as u16;
    let ph = (h * scale).round() as u16;
    let pr = ((6.0 * scale).round() as i16).max(2).min(ph as i16 / 3);

    vec![
        XRectangle {
            x: px,
            y: py + pr,
            width: pw,
            height: ph.saturating_sub((pr * 2) as u16).max(1),
        },
        XRectangle {
            x: px + 1,
            y: py + 2,
            width: pw.saturating_sub(2).max(1),
            height: ph.saturating_sub(4).max(1),
        },
        XRectangle {
            x: px + 2,
            y: py + 1,
            width: pw.saturating_sub(4).max(1),
            height: ph.saturating_sub(2).max(1),
        },
        XRectangle {
            x: px + pr,
            y: py,
            width: pw.saturating_sub((pr * 2) as u16).max(1),
            height: ph,
        },
    ]
}
