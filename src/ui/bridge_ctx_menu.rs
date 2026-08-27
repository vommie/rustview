pub use super::bridge_ctx_menu_folder::FolderContextMenu;
pub use super::bridge_ctx_menu_media::{MediaActionContext, MediaContextMenu};
pub use super::bridge_ctx_menu_presenter::CtxMenuPresenter;
pub use super::bridge_ctx_menu_tab::{TabActionContext, TabContextMenu};
pub use super::bridge_ctx_menu_tag::TagContextMenu;
pub use super::bridge_file_dialog_delete::DeleteFileDialogController;
pub use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
pub use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
pub use super::bridge_file_dialogs::{FileDialogContext, FileRenameContext, FileRenameDialogController};

pub use super::bridge_ctx_menu_folder::FolderContextMenu as FolderContextMenuBuilder;
pub use super::bridge_ctx_menu_media::MediaContextMenu as MediaContextMenuBuilder;
pub use super::bridge_ctx_menu_presenter::CtxMenuPresenter as SlintContextMenuPresenter;
pub use super::bridge_file_dialogs::FileRenameDialogController as FileRenameController;

use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::bridge_folders::{FolderTreeBuilder, FolderTreeNode};
use super::bridge_image_fullscreen::ImageFullscreenDialogController;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_tag_dialogs::TagDialogController;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{CtxMenu, CtxMenuItem, CtxMenuItemKind, CtxMenuDispatcher};
use crate::library::models::{FolderId, FolderRecord, MediaId};
use crate::library::Library;
use crate::settings::SettingsManager;
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
    pub settings: &'a SettingsManager,
    pub tab_manager: &'a ViewerTabManager,
    pub state: &'a Arc<RwLock<UiState>>,
}

pub struct ContextMenuRegistration {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub tab_manager: Arc<ViewerTabManager>,
    pub state: Arc<RwLock<UiState>>,
    pub dispatcher: Arc<CtxMenuDispatcher>,
    pub rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    pub missing_file_dialog: MissingFileDialogController,
    pub delete_file_dialog: DeleteFileDialogController,
    pub remove_orphaned_dialog: RemoveOrphanedFileDialogController,
    pub tag_dialogs: TagDialogController,
    pub folder_dialogs: FolderDialogController,
    pub fullscreen_controller: ImageFullscreenDialogController,
    pub settings: Arc<SettingsManager>,
}

pub struct BridgeContextMenu;

impl BridgeContextMenu {
    pub fn register_callbacks(window: &MainWindow, reg: ContextMenuRegistration) {
        Self::register_media_menu(window, &reg.library, &reg.thumbnails, &reg.tab_manager, &reg.state);
        Self::register_folder_menu(window, &reg.library, &reg.settings, &reg.tab_manager, &reg.state);
        Self::register_tab_menu(window, &reg.tab_manager, &reg.state);
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
        settings: &Arc<SettingsManager>,
        tab_manager: &Arc<ViewerTabManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings.clone();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_folder_context_menu(move |id, x, y| {
            Self::show_folder_menu(
                FolderMenuContext {
                    window: &win,
                    library: &lib,
                    settings: &set,
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
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_tab_context_menu(move |id, x, y| {
            Self::show_tab_menu(&win, &tm, &st, id as u64, x, y);
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

            // Update active submenus map for multi-level cutout tracking
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

            if let Some(ref menu) = active {
                let scale = win.window().scale_factor();
                let win_w = win.window().size().width as f32 / scale;
                let win_h = win.window().size().height as f32 / scale;

                let (w0, h0) = calculate_menu_dimensions(&menu.items);
                let l0_x = win.get_context_menu_x().max(8.0).min(win_w - w0 - 8.0);
                let l0_y = win.get_context_menu_y().max(8.0).min(win_h - h0 - 8.0);

                let mut cutouts = make_rounded_cutout_rectangles(l0_x, l0_y, w0, h0, scale);

                let sub_map = active_submenus_event.read().clone();
                for (&sub_depth, &sub_menu_idx) in &sub_map {
                    if sub_depth == 1 && let Some(children) = menu.find_children_by_menu_index(sub_menu_idx) {
                        let (w1, h1) = calculate_menu_dimensions(children);
                        let l1_x = if l0_x + w0 + w1 - 2.0 + 8.0 > win_w {
                            (l0_x - w1 + 2.0).max(8.0)
                        } else {
                            l0_x + w0 - 2.0
                        };
                        let sub1_y = calculate_item_y_offset(&menu.items, sub_menu_idx);
                        let l1_y = (l0_y + sub1_y - 4.0).max(8.0).min(win_h - h1 - 8.0);

                        let sub_cutouts = make_rounded_cutout_rectangles(l1_x, l1_y, w1, h1, scale);
                        cutouts.extend(sub_cutouts);
                    }
                }

                tm_sub.clip_video_subwindow(&cutouts);
            }
        });

        let win = window.clone_strong();
        let tm = tab_manager.clone();
        let active_submenus_dismiss = active_submenus;
        window.on_context_menu_dismissed(move || {
            active_submenus_dismiss.write().clear();
            CtxMenuPresenter::dismiss(&win);
            tm.reset_video_subwindow_clip();
        });
    }

    fn register_action_dispatcher(window: &MainWindow, reg: ContextMenuRegistration) {
        let win = window.clone_strong();
        let lib = reg.library;
        let thumb = reg.thumbnails;
        let tab_mgr = reg.tab_manager;
        let st = reg.state;
        let set = reg.settings;
        let disp = reg.dispatcher;
        let ren_dlg = reg.rename_dialog;
        let missing_file_dlg = reg.missing_file_dialog;
        let delete_file_dlg = reg.delete_file_dialog;
        let remove_orphaned_dlg = reg.remove_orphaned_dialog;
        let tag_dlg = reg.tag_dialogs;
        let fld_dlg = reg.folder_dialogs;
        let fs_ctrl = reg.fullscreen_controller;

        window.on_context_menu_item_clicked(move |action_id| {
            CtxMenuPresenter::dismiss(&win);
            tab_mgr.reset_video_subwindow_clip();
            let action_u64 = action_id as u64;

            // 1. Folder Actions
            if action_u64 >= CTX_FOLDER_MOVE_BASE || (3000..4000).contains(&action_u64) {
                let target_folder_id = st.read().active_folder_dialog_target;
                let ctx = FolderDialogContext {
                    library: lib.clone(),
                    thumbnails: thumb.clone(),
                    settings: set.clone(),
                    state: st.clone(),
                };
                FolderContextMenu::handle_action(action_u64, &win, &fld_dlg, ctx, target_folder_id);
                return;
            }

            // 2. Tag Actions
            if action_u64 >= CTX_TAG_MOVE_TARGET_BASE || (2000..3000).contains(&action_u64) {
                let bundle = tag_dlg.create_action_bundle(&win, &lib, &thumb, &set, &st);
                TagContextMenu::handle_action(action_u64, &win, &lib, &set, &st, bundle);
                return;
            }

            // 3. Tab Actions
            if (4000..5000).contains(&action_u64) {
                let target_tab_id = st.read().active_tab_dialog_target;
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
                    target_tab_id,
                );
                return;
            }

            // 4. Media File Actions
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
        let (all_selected_deleted, selected_count, is_single_image, associated_apps) =
            MediaContextMenu::prepare_selection(ctx.window, ctx.library, ctx.state, media_id);
        let is_busy = BridgeMedia::is_system_busy(ctx.window, ctx.thumbnails, ctx.state);
        let menu = MediaContextMenu::build_menu(
            selected_count,
            is_single_image,
            all_selected_deleted,
            is_busy,
            &associated_apps,
        );
        Self::open_custom_menu(ctx.window, ctx.state, ctx.tab_manager, menu, x, y);
    }

    pub fn show_folder_menu(
        ctx: FolderMenuContext<'_>,
        folder_id: FolderId,
        x: f32,
        y: f32,
    ) {
        let app_settings = ctx.settings.get();
        let root_setting = app_settings.general.root_folder;
        let show_hidden_folders = app_settings.folders.show_hidden_folders;
        let support_dot_hidden_files = app_settings.folders.support_dot_hidden_files;

        let mut st = ctx.state.write();
        st.active_folder_dialog_target = Some(folder_id);

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

        let menu = FolderContextMenu::build_menu_from_cached_tree(
            &folder_rec,
            tree,
            is_root,
            show_hidden_folders,
            support_dot_hidden_files,
            dot_hidden_cache,
        );

        drop(st);
        Self::open_custom_menu(ctx.window, ctx.state, ctx.tab_manager, menu, x, y);
    }

    pub fn show_tab_menu(
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
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
        state.write().active_tab_dialog_target = Some(tab_id);

        let menu = TabContextMenu::build_menu(tab, tab_idx, total_tabs, &tabs);
        // Position directly below tab bar
        Self::open_custom_menu(window, state, tab_manager, menu, x, y + 10.0);
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
        tab_manager.clip_video_subwindow(&cutouts);

        let mut st = state.write();
        CtxMenuPresenter::open(window, &mut st.active_context_menu, menu, x, y);
    }
}

/// Dynamically calculates the exact pixel dimensions of a menu from its items to eliminate black overhangs.
pub fn calculate_menu_dimensions(items: &[CtxMenuItem]) -> (f32, f32) {
    let width = 230.0;
    let mut height = 8.0; // 4px top + 4px bottom padding
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            height += 1.0; // 1px item spacing
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
    let mut offset_y = 4.0; // 4px top padding
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            offset_y += 1.0; // 1px item spacing
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
        // Central wide band
        XRectangle {
            x: px,
            y: py + pr,
            width: pw,
            height: ph.saturating_sub((pr * 2) as u16).max(1),
        },
        // Intermediate horizontal band for smooth corner step
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
        // Top & bottom horizontal inset band
        XRectangle {
            x: px + pr,
            y: py,
            width: pw.saturating_sub((pr * 2) as u16).max(1),
            height: ph,
        },
    ]
}
