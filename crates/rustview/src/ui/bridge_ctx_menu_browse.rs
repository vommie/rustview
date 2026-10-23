//! Context menus of the Browse view that belong to no single file or folder: the free space between the tiles and a selection
//! that mixes folders and files. Every entry is an action that exists elsewhere (same ids, same handlers).

use super::bridge::{DeleteActionParams, UiBridge};
use super::bridge_ctx_menu::BridgeContextMenu;
use super::bridge_ctx_menu_folder::FolderContextMenu;
use super::bridge_file_dialog_delete::DeleteFileDialogController;
use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_folder_dialogs::FolderDialogController;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_status_bar::StatusBarBridge;
use super::browse_folders::BrowseFolders;
use super::favorites::FavoriteManager;
use super::multiview::MultiviewHandle;
use super::open_confirmation::{OpenConfirmation, OpenKind};
use super::MainWindow;
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder};
use crate::icons::Icon;
use rustview_core::library::Library;
use rustview_core::settings::SettingsManager;
use crate::system::SystemFileHandler;
use rustview_core::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::Arc;

/// What opening one of these menus needs.
pub struct BrowseMenuContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub state: &'a Arc<RwLock<UiState>>,
}

/// What the actions of these menus need.
pub struct BrowseActionContext<'a> {
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub favorites: &'a Arc<RwLock<FavoriteManager>>,
    pub delete_file_dialog: &'a DeleteFileDialogController,
    pub remove_orphaned_file_dialog: &'a RemoveOrphanedFileDialogController,
    pub folder_dialogs: &'a FolderDialogController,
    pub fullscreen: &'a MultiviewHandle,
}

/// What the menu of the free space shows.
struct BackgroundMenuInfo {
    /// The open folder when the view is a plain folder (not a query, not the recursive view).
    folder: Option<(String, PathBuf, bool)>,
    is_busy: bool,
    show_folders: bool,
    has_items: bool,
}

pub struct BrowseContextMenus;

impl BrowseContextMenus {
    /// True for a selection that holds a folder and at least two items in all: it gets the reduced menu of this module.
    pub fn is_mixed_selection(st: &UiState) -> bool {
        let (folders, files) = st.browse_selection_counts();
        folders > 0 && folders + files > 1
    }

    fn plural(count: usize, one: &str, many: &str) -> String {
        format!("{} {}", count, if count == 1 { one } else { many })
    }

    /// The menu of a selection of several items with at least one folder: only what makes sense for both kinds. Copying and moving
    /// folders is not offered yet.
    pub fn build_selection_menu(folders: usize, files: usize, is_busy: bool) -> CtxMenu {
        let title = match (folders, files) {
            (folders, 0) => Self::plural(folders, "Folder", "Folders"),
            (0, files) => Self::plural(files, "File", "Files"),
            (folders, files) => format!("{}, {}", Self::plural(folders, "Folder", "Folders"), Self::plural(files, "File", "Files")),
        };
        CtxMenuBuilder::new()
            .header(format!("Selection: {title}"))
            .action(
                CTX_BROWSE_SELECTION_OPEN_IN_FILE_MANAGER,
                "Open in File Manager",
                None,
                Some(Icon::FolderOpen),
                true,
            )
            .separator()
            .action_with_tooltip(
                CTX_BROWSE_SELECTION_DELETE,
                "Delete...",
                Some("Del"),
                Some(Icon::Delete),
                !is_busy,
                if is_busy {
                    "Delete the selected folders and files\nDisabled: Another scan or refresh operation is currently in progress"
                } else {
                    "Delete the selected folders and files (a confirmation follows for each)"
                },
            )
            .build()
    }

    pub fn show_selection_menu(ctx: BrowseMenuContext<'_>, x: f32, y: f32) {
        let is_busy = BridgeMedia::is_system_busy(ctx.window, ctx.thumbnails, ctx.state);
        let (folders, files) = {
            let mut st = ctx.state.write();
            st.last_active_panel = LastActivePanel::Media;
            st.active_folder_dialog_target = None;
            st.active_tab_dialog_target = None;
            st.browse_selection_counts()
        };
        let menu = Self::build_selection_menu(folders, files, is_busy);
        BridgeContextMenu::open_custom_menu(ctx.window, ctx.state, ctx.tab_manager, menu, x, y);
    }

    fn build_background_menu(info: &BackgroundMenuInfo) -> CtxMenu {
        let mut builder = CtxMenuBuilder::new();
        let mut is_missing = false;

        builder = match &info.folder {
            Some((name, path, missing)) => {
                is_missing = *missing;
                let (title, _) = CtxMenuBuilder::format_header_title("Folder: ", name, 24);
                builder.header_with_tooltip(title, format!("Folder: {}", path.display()))
            }
            None => builder.header("Browse View"),
        };

        if info.folder.is_some() {
            builder = if is_missing {
                builder.action_with_tooltip(
                    CTX_FOLDER_OPEN_IN_FILE_MANAGER,
                    "Open in File Manager",
                    None,
                    Some(Icon::FolderOpen),
                    false,
                    "Disabled: Folder does not exist on physical disk",
                )
            } else {
                builder.action(CTX_FOLDER_OPEN_IN_FILE_MANAGER, "Open in File Manager", None, Some(Icon::FolderOpen), true)
            };
            builder = builder.separator();
            builder = if is_missing {
                builder.action_with_tooltip(
                    CTX_FOLDER_NEW,
                    "New Folder...",
                    None,
                    Some(Icon::FolderPlus),
                    false,
                    "Disabled: Cannot create a folder in a folder that does not exist on physical disk",
                )
            } else if info.is_busy {
                builder.action_with_tooltip(
                    CTX_FOLDER_NEW,
                    "New Folder...",
                    None,
                    Some(Icon::FolderPlus),
                    false,
                    "Create a new folder (or a path of folders) in this folder\nDisabled: Another scan or refresh operation is currently in progress",
                )
            } else {
                builder.action_with_tooltip(
                    CTX_FOLDER_NEW,
                    "New Folder...",
                    Some("Ctrl+Shift+N"),
                    Some(Icon::FolderPlus),
                    true,
                    "Create a new folder (or a path of folders) in this folder",
                )
            };
            builder = builder.separator();
        }

        builder = builder
            .action(CTX_ACTION_SELECT_ALL, "Select All", Some("Ctrl+A"), Some(Icon::SelectAll), info.has_items)
            .checkable(CTX_BROWSE_TOGGLE_FOLDERS, "Show Folders", info.show_folders, None, Some(Icon::Folders), true);

        if info.folder.is_some() {
            builder = builder.separator();
            builder = FolderContextMenu::append_refresh_submenu(builder, is_missing, info.is_busy);
        }
        builder.build()
    }

    /// The menu of the free space: the open folder's own actions (the ids of the folder menu), the selection and the folder rows.
    pub fn show_background_menu(ctx: BrowseMenuContext<'_>, x: f32, y: f32) {
        // The menu is about the folder, not about the selection: a click on the free space deselects, as a left click does.
        BridgeFileSelection::clear_selection(ctx.window, ctx.library, ctx.state);

        let is_busy = BridgeMedia::is_system_busy(ctx.window, ctx.thumbnails, ctx.state);
        let info = {
            let mut st = ctx.state.write();
            st.active_tab_dialog_target = None;
            let folder = st
                .current_folder_id
                .filter(|_| st.is_plain_folder_view())
                .and_then(|id| st.folder_hierarchy.as_ref().and_then(|snapshot| snapshot.get_node(id)).map(|node| (id, node)))
                .map(|(id, node)| (id, node.name.clone(), node.path.clone(), node.is_deleted));
            st.active_folder_dialog_target = folder.as_ref().map(|(id, ..)| *id);
            BackgroundMenuInfo {
                folder: folder.map(|(_, name, path, missing)| (name, path, missing)),
                is_busy,
                show_folders: st.show_folders,
                has_items: st.view_len() > 0,
            }
        };
        let menu = Self::build_background_menu(&info);
        BridgeContextMenu::open_custom_menu(ctx.window, ctx.state, ctx.tab_manager, menu, x, y);
    }

    /// The actions that only these menus have: the selection's Open in File Manager and Delete, and the Show Folders toggle.
    pub fn handle_action(action_id: CtxMenuActionId, window: &MainWindow, ctx: BrowseActionContext<'_>) {
        match action_id {
            CTX_BROWSE_SELECTION_OPEN_IN_FILE_MANAGER => Self::open_selection_in_file_manager(window, &ctx),
            CTX_BROWSE_SELECTION_DELETE => UiBridge::trigger_delete_action(DeleteActionParams {
                window,
                library: ctx.library,
                thumbnails: ctx.thumbnails,
                tab_manager: ctx.tab_manager,
                settings: ctx.settings,
                state: ctx.state,
                favorites: ctx.favorites,
                delete_file_dialog: ctx.delete_file_dialog,
                remove_orphaned_file_dialog: ctx.remove_orphaned_file_dialog,
                folder_dialogs: ctx.folder_dialogs,
                fullscreen: ctx.fullscreen,
            }),
            CTX_BROWSE_TOGGLE_FOLDERS => {
                let on = !ctx.state.read().show_folders;
                window.set_show_folders(on);
                BrowseFolders::set_show_folders(on, window, ctx.library, ctx.thumbnails, ctx.settings, ctx.state);
            }
            _ => {}
        }
    }

    /// One File Manager window with every selected folder and file highlighted (they all lie in the open folder).
    fn open_selection_in_file_manager(window: &MainWindow, ctx: &BrowseActionContext<'_>) {
        let paths: Vec<PathBuf> = {
            let st = ctx.state.read();
            let mut paths: Vec<PathBuf> = match st.folder_hierarchy.as_ref() {
                Some(snapshot) => st
                    .folder_rows
                    .selected_folders_ordered()
                    .into_iter()
                    .filter_map(|id| snapshot.get_node(id))
                    .filter(|node| !node.is_deleted)
                    .map(|node| node.path.clone())
                    .collect(),
                None => Vec::new(),
            };
            paths.extend(st.get_selected_paths_ordered());
            paths
        };
        if paths.is_empty() {
            StatusBarBridge::warn(window, "Nothing to show: the selected items do not exist on disk.");
            return;
        }
        let win = window.clone_strong();
        OpenConfirmation::run_or_ask(window, ctx.settings, OpenKind::FileManager, paths.len(), move || {
            Self::reveal_in_file_manager(&win, &paths);
        });
    }

    fn reveal_in_file_manager(window: &MainWindow, paths: &[PathBuf]) {
        match SystemFileHandler::reveal_in_file_manager(paths) {
            Ok(()) => StatusBarBridge::success(window, format!("Opened {} item(s) in File Manager.", paths.len())),
            Err(e) => {
                tracing::error!(error = %e, "[BrowseContextMenu] Failed to open the selection in the File Manager");
                StatusBarBridge::error(window, format!("Failed to open File Manager: {}", e));
            }
        }
    }
}
