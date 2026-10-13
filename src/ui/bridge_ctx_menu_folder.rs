use super::MainWindow;
use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::bridge_folders::BridgeFolders;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_status_bar::StatusBarBridge;
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuItem};
use crate::icons::Icon;
use crate::library::models::{CachedFolderNode, FolderHierarchySnapshot, FolderId};
use crate::settings::LogObscurer;
use crate::system::SystemFileHandler;
use std::path::{Path, PathBuf};

/// Reason appended to the tooltip of refresh entries while another scan or refresh holds the busy lock.
const DISABLED_BUSY_REASON: &str = "Disabled: Another scan or refresh operation is currently in progress";

/// Domain context menu builder and action handler for folder tree nodes.
pub struct FolderContextMenu;

impl FolderContextMenu {
    /// Builds the declarative context menu for a folder node directly from the in-memory SPoT snapshot with zero disk I/O.
    /// Provides clear contextual tooltips on disabled options explaining why the action cannot be performed.
    pub fn build_menu(
        target_node: &CachedFolderNode,
        snapshot: &FolderHierarchySnapshot,
        is_root: bool,
        show_hidden_folders: bool,
        is_busy: bool,
        is_favorite: bool,
    ) -> CtxMenu {
        tracing::debug!(
            folder = %LogObscurer::folder(Some(target_node.id), &target_node.path),
            "[FolderContextMenu: Build] Building context menu from SPoT hierarchy"
        );

        let (prefix, max_chars) = if target_node.is_deleted {
            ("Missing Folder: ", 18usize)
        } else {
            ("Folder: ", 24usize)
        };

        let (header_title, _) = CtxMenuBuilder::format_header_title(prefix, &target_node.name, max_chars);
        let header_tooltip = format!(
            "{}{}",
            if target_node.is_deleted { "Missing Folder: " } else { "Folder: " },
            target_node.path.display()
        );

        let mut builder = CtxMenuBuilder::new().header_with_tooltip(header_title, header_tooltip);

        // 1. Open in File Manager (with disabled reason tooltip)
        builder = if target_node.is_deleted {
            builder.action_with_tooltip(
                CTX_FOLDER_OPEN_IN_FILE_MANAGER,
                "Open in File Manager",
                None,
                Some(Icon::FolderOpen),
                false,
                "Disabled: Folder does not exist on physical disk",
            )
        } else {
            builder.action(
                CTX_FOLDER_OPEN_IN_FILE_MANAGER,
                "Open in File Manager",
                None,
                Some(Icon::FolderOpen),
                true,
            )
        };
        builder = builder.separator();

        if !is_root {
            builder = if target_node.is_deleted {
                builder.action_with_tooltip(
                    CTX_FOLDER_RENAME,
                    "Rename Folder...",
                    Some("F2"),
                    Some(Icon::Rename),
                    false,
                    "Disabled: Cannot rename a folder that does not exist on physical disk",
                )
            } else {
                builder.action(
                    CTX_FOLDER_RENAME,
                    "Rename Folder...",
                    Some("F2"),
                    Some(Icon::Rename),
                    true,
                )
            };

            let move_submenu = Self::build_reparent_submenu(
                target_node.id,
                snapshot,
                show_hidden_folders,
            );
            builder = builder.item(move_submenu);
        }

        // 2. Refresh submenu (with disabled reason tooltips when busy or missing)
        let can_refresh = !target_node.is_deleted && !is_busy;
        builder = builder.separator();
        builder = builder.submenu("Refresh...", Some(Icon::Refresh), can_refresh, |sub| {
            // Every tooltip keeps the description of the function; a disabled entry appends the reason.
            let reason_tip = |description: &str| {
                if target_node.is_deleted {
                    format!("{}\nDisabled: Folder does not exist on physical disk", description)
                } else if is_busy {
                    format!("{}\n{}", description, DISABLED_BUSY_REASON)
                } else {
                    description.to_string()
                }
            };

            let refresh_file_tip = reason_tip("Scan and refresh all files in this folder (Full re-indexing)");
            let refresh_meta_tip = reason_tip("Refresh metadata and file sizes without re-generating thumbnails");
            let refresh_relative_tip = reason_tip("Rescan this folder and its subdirectories only");
            let refresh_tree_description = "Perform a complete scan of the entire directory tree on disk";
            let refresh_tree_tip = if is_busy {
                format!("{}\n{}", refresh_tree_description, DISABLED_BUSY_REASON)
            } else {
                refresh_tree_description.to_string()
            };

            sub.action_with_tooltip(
                CTX_FOLDER_REFRESH_FILES_FULL,
                "Refresh Files (Full)",
                None,
                Some(Icon::Refresh),
                can_refresh,
                refresh_file_tip,
            )
            .action_with_tooltip(
                CTX_FOLDER_REFRESH_FILES_META,
                "Refresh Files (Meta)",
                None,
                Some(Icon::Refresh),
                can_refresh,
                refresh_meta_tip,
            )
            .separator()
            .action_with_tooltip(
                CTX_FOLDER_REFRESH_TREE_FULL,
                "Refresh Tree (Full)",
                None,
                Some(Icon::Refresh),
                !is_busy,
                refresh_tree_tip,
            )
            .action_with_tooltip(
                CTX_FOLDER_REFRESH_TREE_RELATIVE,
                "Refresh Tree (Relative)",
                None,
                Some(Icon::Refresh),
                can_refresh,
                refresh_relative_tip,
            )
        });

        // 3. Add to Favorites (with disabled reason tooltip)
        builder = builder.separator();
        let can_favorite = !is_favorite && !target_node.is_deleted;
        let favorite_description = "Bookmark this folder in the Favorites sidebar";
        let favorite_tip = if is_favorite {
            format!("{}\nDisabled: Folder is already added to Favorites", favorite_description)
        } else if target_node.is_deleted {
            format!("{}\nDisabled: Cannot add a missing or deleted folder to Favorites", favorite_description)
        } else {
            favorite_description.to_string()
        };

        builder = builder.action_with_tooltip(
            CTX_FOLDER_ADD_TO_FAVORITES,
            "Add to Favorites",
            None,
            Some(Icon::FavoriteAdd),
            can_favorite,
            favorite_tip,
        );
        builder = builder.separator();

        if !is_root {
            if target_node.is_deleted {
                builder = builder.action(
                    CTX_FOLDER_PURGE_DB,
                    "Remove from Database...",
                    Some("Del"),
                    Some(Icon::DatabaseClean),
                    true,
                );
            } else {
                builder = builder.action(
                    CTX_FOLDER_DELETE,
                    "Delete Folder...",
                    Some("Del"),
                    Some(Icon::Delete),
                    true,
                );
            }
        }

        builder.build()
    }

    /// Recursively builds the "Move to..." reparenting submenu directly from the in-memory SPoT snapshot with zero disk I/O.
    fn build_reparent_submenu(
        target_folder_id: FolderId,
        snapshot: &FolderHierarchySnapshot,
        show_hidden: bool,
    ) -> CtxMenuItem {
        let sub_items = snapshot.project_target_folder_menu(
            CTX_FOLDER_MOVE_BASE,
            Some(target_folder_id),
            snapshot.get_node(target_folder_id).and_then(|n| n.parent_id),
            show_hidden,
        );

        let is_enabled = !sub_items.is_empty();
        CtxMenuItem::new_submenu("Move to...", Some(Icon::MoveTo), is_enabled, sub_items)
    }

    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        folder_dialogs: &FolderDialogController,
        ctx: FolderDialogContext,
        target_folder_id: Option<FolderId>,
    ) {
        if action_id >= CTX_FOLDER_MOVE_BASE {
            let target_parent_id = (action_id - CTX_FOLDER_MOVE_BASE) as FolderId;
            if let Some(src_id) = target_folder_id {
                folder_dialogs.show_move(window, src_id, target_parent_id, ctx);
            }
            return;
        }

        let is_refresh_action = action_id == CTX_FOLDER_REFRESH
            || action_id == CTX_FOLDER_REFRESH_FILES_FULL
            || action_id == CTX_FOLDER_REFRESH_METADATA
            || action_id == CTX_FOLDER_REFRESH_FILES_META
            || action_id == CTX_FOLDER_REFRESH_TREE_FULL
            || action_id == CTX_FOLDER_REFRESH_TREE_RELATIVE;

        if is_refresh_action && BridgeMedia::is_system_busy(window, &ctx.thumbnails, &ctx.state) {
            StatusBarBridge::warn(
                window,
                "Cannot start refresh: another operation is already in progress.",
            );
            return;
        }

        if action_id == CTX_FOLDER_REFRESH_TREE_FULL {
            BridgeFolders::refresh_folder_tree(
                window,
                &ctx.library,
                &ctx.thumbnails,
                &ctx.settings,
                &ctx.state,
            );
            return;
        }

        if let Some(fid) = target_folder_id
            && let Ok(Some(rec)) = ctx.library.folders().get_folder(fid)
        {
            let name = rec.name.clone();

            if action_id == CTX_FOLDER_OPEN_IN_FILE_MANAGER {
                tracing::info!(
                    folder = %LogObscurer::folder(Some(fid), Path::new(&rec.path)),
                    "[FolderContextMenu] Open in File Manager triggered"
                );
                if let Err(e) = SystemFileHandler::open_folder(&rec.path) {
                    tracing::error!(
                        error = %e,
                        folder = %LogObscurer::folder(Some(fid), Path::new(&rec.path)),
                        "[FolderContextMenu] Failed to open folder in File Manager"
                    );
                    StatusBarBridge::error(window, format!("Failed to open File Manager: {}", e));
                } else {
                    StatusBarBridge::success(
                        window,
                        format!("Opened folder '{}' in File Manager.", name),
                    );
                }
            } else if action_id == CTX_FOLDER_REFRESH || action_id == CTX_FOLDER_REFRESH_FILES_FULL {
                BridgeFolders::refresh_folder_explicit(
                    fid,
                    PathBuf::from(&rec.path),
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.settings,
                    &ctx.state,
                );
            } else if action_id == CTX_FOLDER_REFRESH_METADATA || action_id == CTX_FOLDER_REFRESH_FILES_META {
                BridgeFolders::refresh_folder_metadata_only(
                    fid,
                    PathBuf::from(&rec.path),
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.state,
                );
            } else if action_id == CTX_FOLDER_REFRESH_TREE_RELATIVE {
                BridgeFolders::refresh_folder_subtree(
                    fid,
                    PathBuf::from(&rec.path),
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.settings,
                    &ctx.state,
                );
            } else if action_id == CTX_FOLDER_RENAME {
                folder_dialogs.show_rename(window, fid, &name, ctx);
            } else if action_id == CTX_FOLDER_DELETE {
                folder_dialogs.show_delete(window, fid, &name, ctx);
            } else if action_id == CTX_FOLDER_PURGE_DB {
                folder_dialogs.show_remove_orphaned(window, fid, &name, ctx);
            }
        }
    }
}
