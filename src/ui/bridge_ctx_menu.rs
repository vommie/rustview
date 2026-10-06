pub use super::bridge_ctx_menu_favorite::FavoriteContextMenu;
pub use super::bridge_ctx_menu_folder::{FolderContextMenu, FolderMenuBuildParams};
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
use super::bridge_folders::{FolderTreeBuilder, FolderTreeNode};
use super::bridge_image_fullscreen::ImageFullscreenDialogController;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_tag_dialogs::TagDialogController;
use super::favorites::FavoriteManager;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{CtxMenu, CtxMenuItem, CtxMenuItemKind, CtxMenuDispatcher};
use crate::library::models::{FolderId, FolderRecord, MediaId};
use crate::library::Library;
use crate::settings::SettingsManager;
use crate::system::DesktopIntegration;
use crate::thumbnails::ThumbnailManager;
use crate::viewer::{TabId, ViewerTabManager, XRectangle};
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// Context parameter bundle for opening media file context menus.
pub struct MediaMenuContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Library,
    pub thumbnails: &'a ThumbnailManager,
    pub tab_manager: &'a ViewerTabManager,
    pub state: &'a Arc<RwLock<UiState>>,
}

/// Context parameter bundle for opening folder tree context menus.
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
    pub fullscreen_controller: ImageFullscreenDialogController,
    pub settings: Arc<SettingsManager>,
}

pub struct BridgeContextMenu;

impl BridgeContextMenu {
    pub fn register_callbacks(window: &MainWindow, reg: ContextMenuRegistration) {
        Self::register_media_menu(window, &reg.library, &reg.thumbnails, &reg.tab_manager, &reg.state);
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
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let th = thumbnails.clone();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_media_context_menu(move |id, x, y| {
            st.write().last_active_panel = LastActivePanel::Media;
            Self::show_media_menu(
                MediaMenuContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &th,
                    tab_manager: &tm,
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
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let st = state.clone();
        let tm_sub = tab_manager.clone();
        let active_submenus: Arc<RwLock<HashMap<i32, usize>>> = Arc::new(RwLock::new(HashMap::new()));
        let active_submenus_event = active_submenus.clone();

        window.on_context_menu_request_submenu(move |depth, m_idx| {
            let active = st.read().active_context_menu.clone();
            CtxMenuPresenter::handle_submenu_request(&win, active.as_ref(), depth, m_idx as usize);

            {
                let mut map = active_submenus_event.write();
                if depth == 0 {
                    if m_idx > 0 {
                        map.clear();
                        map.insert(1, m_idx as usize);
                    } else {
                        map.clear();
                    }
                } else if m_idx > 0 {
                    let next_depth = depth + 1;
                    map.retain(|&d, _| d < next_depth);
                    map.insert(next_depth, m_idx as usize);
                } else {
                    let max_depth = depth;
                    map.retain(|&d, _| d <= max_depth);
                }
            }

            let sub_map = active_submenus_event.read().clone();
            let all_cutouts = calculate_all_active_menu_cutouts(&win, active.as_ref(), &sub_map);
            tm_sub.set_menu_cutouts(all_cutouts);
        });

        let win = window.clone_strong();
        let tm = tab_manager.clone();
        let active_submenus_dismiss = active_submenus;
        window.on_context_menu_dismissed(move || {
            active_submenus_dismiss.write().clear();
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
        let fs_ctrl = reg.fullscreen_controller;

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

            // 2. Folder Actions
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

            if action_u64 >= CTX_FOLDER_MOVE_BASE || (3000..4000).contains(&action_u64) {
                let target_folder_id = st.write().active_folder_dialog_target.take();
                let ctx = FolderDialogContext {
                    library: lib.clone(),
                    thumbnails: thumb.clone(),
                    settings: set.clone(),
                    state: st.clone(),
                };
                FolderContextMenu::handle_action(action_u64, &win, &fld_dlg, ctx, target_folder_id);
                return;
            }

            // 3. Tag Actions
            if action_u64 >= CTX_TAG_MOVE_TARGET_BASE || (2000..3000).contains(&action_u64) {
                let bundle = tag_dlg.create_action_bundle(&win, &lib, &thumb, &set, &st);
                TagContextMenu::handle_action(action_u64, &win, &lib, &set, &st, bundle);
                return;
            }

            // 4. Tab Context Menu Invocations (Both tab lifecycle and unified file actions)
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
                            fullscreen_controller: &fs_ctrl,
                        },
                        Some(target_tab_id),
                    );
                    return;
                }

                // If a unified file action was clicked on a tab context menu:
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
                        fullscreen_controller: &fs_ctrl,
                    };

                    tracing::info!(
                        target_tab_id,
                        media_id = tab.media_id,
                        action_id = action_u64,
                        "[BridgeContextMenu] Routing unified file action from tab context menu to shared handler"
                    );

                    MediaContextMenu::handle_file_action_for_ids(
                        action_u64,
                        &win,
                        &media_action_ctx,
                        &[tab.media_id],
                    );
                    return;
                }
            }

            // 5. Standard Media Grid / Table Selection Actions
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
                    fullscreen_controller: &fs_ctrl,
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
        let sel_meta = MediaContextMenu::prepare_selection(ctx.window, ctx.library, ctx.state, media_id);
        let menu = MediaContextMenu::build_menu(
            sel_meta.selected_count,
            sel_meta.is_single_image,
            sel_meta.has_video,
            sel_meta.all_selected_deleted,
            is_busy,
            &sel_meta.associated_apps,
        );
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
        let support_dot_hidden_files = app_settings.folders.support_dot_hidden_files;

        let mut st = ctx.state.write();
        st.active_folder_dialog_target = Some(folder_id);
        st.active_tab_dialog_target = None;

        if st.cached_folder_tree.is_none()
            && let Some(ref root_dir) = root_setting
        {
            let folders = ctx.library.folders().list_folders().unwrap_or_default();
            let media_counts = ctx.library.folders().get_all_media_counts().unwrap_or_default();
            let built_tree = FolderTreeBuilder::build_hierarchy(root_dir, &folders, &media_counts);
            st.cached_folder_tree = Some(built_tree);
            st.cached_media_counts = media_counts;
        }

        let UiState {
            cached_folder_tree: ref tree_opt,
            ref mut dot_hidden_cache,
            ..
        } = *st;

        let Some(tree) = tree_opt.as_ref() else {
            return;
        };

        let target_record = Self::find_record_in_tree(tree, folder_id);
        let Some(folder_rec) = target_record else {
            return;
        };

        let is_root = root_setting
            .as_ref()
            .map(|r| Path::new(&folder_rec.path) == r)
            .unwrap_or(false);

        let is_favorite = ctx.favorites.read().has_path(Path::new(&folder_rec.path));

        let menu = FolderContextMenu::build_menu_from_cached_tree(FolderMenuBuildParams {
            target_folder: &folder_rec,
            cached_tree: tree,
            is_root,
            show_hidden_folders,
            support_dot_hidden_files,
            dot_hidden_cache,
            is_busy,
            is_favorite,
        });

        drop(st);
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
            tracing::warn!(tab_id, "[ContextMenu] Tab not found for context menu");
            return;
        };

        let tab = &tabs[tab_idx];
        let is_busy = BridgeMedia::is_system_busy(window, thumbnails, state);

        let associated_apps = if tab.file_path.exists() {
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

    fn find_record_in_tree(nodes: &[FolderTreeNode], target_id: FolderId) -> Option<FolderRecord> {
        for node in nodes {
            if node.record.id == target_id {
                return Some(node.record.clone());
            }
            if let Some(rec) = Self::find_record_in_tree(&node.children, target_id) {
                return Some(rec);
            }
        }
        None
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
        let scale = window.window().scale_factor();
        let win_w = window.window().size().width as f32 / scale;
        let win_h = window.window().size().height as f32 / scale;

        let (w, h) = calculate_menu_dimensions(&menu.items);
        let l0_x = x.max(8.0).min(win_w - w - 8.0);
        let l0_y = y.max(8.0).min(win_h - h - 8.0);

        let cutouts = make_rounded_cutout_rectangles(l0_x, l0_y, w, h, scale);
        tab_manager.set_menu_cutouts(cutouts);

        let mut st = state.write();
        CtxMenuPresenter::open(window, &mut st.active_context_menu, menu, x, y);
    }
}

/// Computes cutout rectangles for the base menu and all currently active submenu levels (Depths 1 through 5).
pub fn calculate_all_active_menu_cutouts(
    window: &MainWindow,
    menu_opt: Option<&CtxMenu>,
    sub_map: &HashMap<i32, usize>,
) -> Vec<XRectangle> {
    let scale = window.window().scale_factor();
    let win_w = window.window().size().width as f32 / scale;
    let win_h = window.window().size().height as f32 / scale;

    let Some(menu) = menu_opt else {
        return Vec::new();
    };

    let (w0, h0) = calculate_menu_dimensions(&menu.items);
    let l0_x = window.get_context_menu_x().max(8.0).min(win_w - w0 - 8.0);
    let l0_y = window.get_context_menu_y().max(8.0).min(win_h - h0 - 8.0);

    let mut all_cutouts = make_rounded_cutout_rectangles(l0_x, l0_y, w0, h0, scale);
    let mut level_coords: HashMap<i32, (f32, f32, f32, f32)> = HashMap::new();
    level_coords.insert(0, (l0_x, l0_y, w0, h0));

    for depth in 1..=5 {
        if let Some(&menu_idx) = sub_map.get(&depth)
            && let Some(children) = menu.find_children_by_menu_index(menu_idx)
            && let Some(&(prev_x, prev_y, prev_w, _prev_h)) = level_coords.get(&(depth - 1))
        {
            let (w_sub, h_sub) = calculate_menu_dimensions(children);
            let x_sub = if prev_x + prev_w + w_sub - 2.0 + 8.0 > win_w {
                (prev_x - w_sub + 2.0).max(8.0)
            } else {
                prev_x + prev_w - 2.0
            };

            let y_offset = calculate_item_y_offset(&menu.items, menu_idx);
            let y_sub = (prev_y + y_offset - 4.0).max(8.0).min(win_h - h_sub - 8.0);

            let sub_cutouts = make_rounded_cutout_rectangles(x_sub, y_sub, w_sub, h_sub, scale);
            all_cutouts.extend(sub_cutouts);
            level_coords.insert(depth, (x_sub, y_sub, w_sub, h_sub));
        }
    }

    all_cutouts
}

/// Dynamically calculates the exact pixel dimensions of a menu from its items to eliminate black overhangs.
pub fn calculate_menu_dimensions(items: &[CtxMenuItem]) -> (f32, f32) {
    let width = 230.0;
    let mut height = 8.0;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            height += 1.0;
        }
        height += match item.kind {
            CtxMenuItemKind::Separator => 7.0,
            CtxMenuItemKind::Header => 22.0,
            _ => 28.0,
        };
    }
    (width, height)
}

/// Calculates the exact vertical offset of an item with `menu_index` within its parent menu box.
pub fn calculate_item_y_offset(items: &[CtxMenuItem], target_menu_idx: usize) -> f32 {
    let mut offset_y = 4.0;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            offset_y += 1.0;
        }
        if item.menu_index == target_menu_idx {
            return offset_y;
        }
        offset_y += match item.kind {
            CtxMenuItemKind::Separator => 7.0,
            CtxMenuItemKind::Header => 22.0,
            _ => 28.0,
        };
    }
    0.0
}

/// Constructs 4 precise overlapping horizontal bands matching Slint's 6px rounded rectangle border radius.
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
