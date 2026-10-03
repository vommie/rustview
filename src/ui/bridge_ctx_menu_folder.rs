use super::MainWindow;
use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::bridge_folders::{BridgeFolders, FolderTreeNode};
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_status_bar::StatusBarBridge;
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuItem};
use crate::library::dot_hidden::DotHiddenCache;
use crate::library::models::{FolderId, FolderRecord};
use crate::settings::LogObscurer;
use crate::system::SystemFileHandler;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Parameter context bundle for traversing cached tree nodes without exceeding argument limits.
struct MoveMenuTraverseContext<'a> {
    target_folder_id: FolderId,
    show_hidden_folders: bool,
    support_dot_hidden_files: bool,
    dot_hidden_cache: &'a mut DotHiddenCache,
    valid_destinations: usize,
    hidden_excluded: usize,
}

/// Domain context menu builder and action handler for folder tree nodes.
pub struct FolderContextMenu;

impl FolderContextMenu {
    /// Builds the declarative context menu for a folder item directly from the in-memory cached FolderTree hierarchy with zero disk I/O.
    /// Formats the header using sticky prefix and left-side ellipsis to eliminate menu text overflow while attaching the full path as tooltip.
    pub fn build_menu_from_cached_tree(
        target_folder: &FolderRecord,
        cached_tree: &[FolderTreeNode],
        is_root: bool,
        show_hidden_folders: bool,
        support_dot_hidden_files: bool,
        dot_hidden_cache: &mut DotHiddenCache,
        is_busy: bool,
    ) -> CtxMenu {
        let t0 = Instant::now();
        let target_path = Path::new(&target_folder.path);
        let folder_name = target_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| target_folder.path.clone());

        let (prefix, max_chars) = if target_folder.is_deleted {
            ("Missing Folder: ", 18usize)
        } else {
            ("Folder: ", 24usize)
        };

        let (header_title, _) = CtxMenuBuilder::format_header_title(prefix, &folder_name, max_chars);
        let header_tooltip = format!(
            "{}{}",
            if target_folder.is_deleted { "Missing Folder: " } else { "Folder: " },
            target_folder.path
        );

        let mut builder = CtxMenuBuilder::new().header_with_tooltip(header_title, header_tooltip);

        // Open in File Manager right at the top
        builder = builder.action(
            CTX_FOLDER_OPEN_IN_FILE_MANAGER,
            "Open in File Manager",
            None,
            Some("folder"),
            !target_folder.is_deleted,
        );
        builder = builder.separator();

        if !is_root {
            builder = builder.action(
                CTX_FOLDER_RENAME,
                "Rename Folder...",
                Some("F2"),
                Some("edit"),
                !target_folder.is_deleted,
            );

            let move_submenu = Self::build_reparent_submenu_from_tree(
                target_folder,
                cached_tree,
                show_hidden_folders,
                support_dot_hidden_files,
                dot_hidden_cache,
                t0,
            );
            builder = builder.item(move_submenu);
        }

        // Modular "Refresh..." submenu containing files and tree refreshes (no F5 shortcuts!)
        let can_refresh = !target_folder.is_deleted && !is_busy;
        builder = builder.separator();
        builder = builder.submenu("Refresh...", Some("refresh"), can_refresh, |sub| {
            sub.action(
                CTX_FOLDER_REFRESH_FILES_FULL,
                "Refresh Files (Full)",
                None,
                Some("refresh"),
                can_refresh,
            )
            .action(
                CTX_FOLDER_REFRESH_FILES_META,
                "Refresh Files (Meta)",
                None,
                Some("refresh"),
                can_refresh,
            )
            .separator()
            .action(
                CTX_FOLDER_REFRESH_TREE_FULL,
                "Refresh Tree (Full)",
                None,
                Some("refresh"),
                !is_busy,
            )
            .action(
                CTX_FOLDER_REFRESH_TREE_RELATIVE,
                "Refresh Tree (Relative)",
                None,
                Some("refresh"),
                can_refresh,
            )
        });
        builder = builder.separator();

        if !is_root {
            if target_folder.is_deleted {
                builder = builder.action(
                    CTX_FOLDER_PURGE_DB,
                    "Remove from Database...",
                    Some("Del"),
                    Some("trash"),
                    true,
                );
            } else {
                builder = builder.action(
                    CTX_FOLDER_DELETE,
                    "Delete Folder...",
                    Some("Del"),
                    Some("trash"),
                    true,
                );
            }
        }

        builder.build()
    }

    /// Backward-compatible wrapper calling `build_menu_from_cached_tree`.
    pub fn build_menu(
        target_folder: &FolderRecord,
        cached_tree: &[FolderTreeNode],
        is_root: bool,
        show_hidden_folders: bool,
        support_dot_hidden_files: bool,
        dot_hidden_cache: &mut DotHiddenCache,
        is_busy: bool,
    ) -> CtxMenu {
        Self::build_menu_from_cached_tree(
            target_folder,
            cached_tree,
            is_root,
            show_hidden_folders,
            support_dot_hidden_files,
            dot_hidden_cache,
            is_busy,
        )
    }

    /// Traverses the cached in-memory tree to build the "Move to..." submenu with zero disk I/O and zero redundant allocations.
    fn build_reparent_submenu_from_tree(
        current_folder: &FolderRecord,
        cached_tree: &[FolderTreeNode],
        show_hidden_folders: bool,
        support_dot_hidden_files: bool,
        dot_hidden_cache: &mut DotHiddenCache,
        t_start: Instant,
    ) -> CtxMenuItem {
        if current_folder.is_deleted {
            return CtxMenuItem::new_submenu("Move to...", Some("move"), false, Vec::new());
        }

        let Some(root_node) = cached_tree.first() else {
            return CtxMenuItem::new_submenu("Move to...", Some("move"), false, Vec::new());
        };

        let is_root_itself = current_folder.id == root_node.record.id;
        let is_direct_child_of_root = root_node
            .children
            .iter()
            .any(|child| child.record.id == current_folder.id);

        let mut sub_items = Vec::new();

        // 1. Root destination (only if not root itself and not already directly in root)
        if !is_root_itself && !is_direct_child_of_root {
            let root_display_name = Path::new(&root_node.record.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Root".to_string());

            sub_items.push(CtxMenuItem::new_action(
                CTX_FOLDER_MOVE_BASE + (root_node.record.id as CtxMenuActionId),
                format!("📁 Root ({})", root_display_name),
            ));
        }

        // 2. Recursively traverse children of root in-memory
        let mut child_items = Vec::new();
        let mut ctx = MoveMenuTraverseContext {
            target_folder_id: current_folder.id,
            show_hidden_folders,
            support_dot_hidden_files,
            dot_hidden_cache,
            valid_destinations: 0,
            hidden_excluded: 0,
        };

        Self::traverse_tree_for_move_menu(&root_node.children, &mut ctx, &mut child_items);

        if !is_root_itself && !is_direct_child_of_root {
            ctx.valid_destinations += 1;
        }

        if !child_items.is_empty() {
            if !sub_items.is_empty() {
                sub_items.push(CtxMenuItem::new_separator());
            }
            sub_items.extend(child_items);
        }

        let build_time_us = t_start.elapsed().as_micros() as u64;

        tracing::info!(
            folder_id = current_folder.id,
            target_folder = %LogObscurer::folder(Some(current_folder.id), &current_folder.path),
            valid_destinations = ctx.valid_destinations,
            hidden_excluded = ctx.hidden_excluded,
            build_time_us = build_time_us,
            build_time_ms = format!("{:.3} ms", build_time_us as f64 / 1000.0),
            "[Benchmark: FolderContextMenu] Folder context menu built directly from in-memory cache"
        );

        let is_enabled = !sub_items.is_empty();
        CtxMenuItem::new_submenu("Move to...", Some("move"), is_enabled, sub_items)
    }

    /// Fast recursive traversal of `FolderTreeNode` in memory with pruning of target folder descendants and hidden branches.
    fn traverse_tree_for_move_menu(
        nodes: &[FolderTreeNode],
        ctx: &mut MoveMenuTraverseContext<'_>,
        output: &mut Vec<CtxMenuItem>,
    ) {
        for node in nodes {
            if node.record.id == ctx.target_folder_id {
                continue;
            }

            if node.record.is_deleted {
                continue;
            }

            let path_obj = Path::new(&node.record.path);
            let raw_name = path_obj
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| node.record.path.clone());

            if !ctx.show_hidden_folders
                && ctx
                    .dot_hidden_cache
                    .is_hidden(path_obj, &raw_name, ctx.support_dot_hidden_files)
            {
                ctx.hidden_excluded += 1;
                continue;
            }

            ctx.valid_destinations += 1;

            let mut child_items = Vec::new();
            if !node.children.is_empty() {
                Self::traverse_tree_for_move_menu(&node.children, ctx, &mut child_items);
            }

            if !child_items.is_empty() {
                let mut menu_items = Vec::with_capacity(child_items.len() + 2);
                menu_items.push(CtxMenuItem::new_action(
                    CTX_FOLDER_MOVE_BASE + (node.record.id as CtxMenuActionId),
                    format!("Move directly into '{}'", raw_name),
                ));
                menu_items.push(CtxMenuItem::new_separator());
                menu_items.extend(child_items);

                output.push(CtxMenuItem::new_submenu(
                    format!("📁 {}", raw_name),
                    None,
                    true,
                    menu_items,
                ));
            } else {
                output.push(CtxMenuItem::new_action(
                    CTX_FOLDER_MOVE_BASE + (node.record.id as CtxMenuActionId),
                    format!("📁 {}", raw_name),
                ));
            }
        }
    }

    /// Handles folder context menu action routing with full single-refreshment guards.
    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        folder_dialogs: &FolderDialogController,
        ctx: FolderDialogContext,
        target_folder_id: Option<FolderId>,
    ) {
        // 1. Move folder under selected target parent
        if action_id >= CTX_FOLDER_MOVE_BASE {
            let target_parent_id = (action_id - CTX_FOLDER_MOVE_BASE) as FolderId;
            if let Some(src_id) = target_folder_id {
                folder_dialogs.show_move(window, src_id, target_parent_id, ctx);
            }
            return;
        }

        // 2. Refreshment Actions: Check single-operation busy guard
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

        // 3. Full Tree Refresh
        if action_id == CTX_FOLDER_REFRESH_TREE_FULL {
            tracing::info!(
                "[FolderContextMenu: Refresh Tree Full] Full tree scan triggered from folder context menu"
            );
            BridgeFolders::refresh_folder_tree(
                window,
                &ctx.library,
                &ctx.thumbnails,
                &ctx.settings,
                &ctx.state,
            );
            return;
        }

        // 4. Folder-Specific Actions
        if let Some(fid) = target_folder_id
            && let Ok(Some(rec)) = ctx.library.folders().get_folder(fid)
        {
            let name = Path::new(&rec.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| rec.path.clone());

            if action_id == CTX_FOLDER_OPEN_IN_FILE_MANAGER {
                tracing::info!(
                    folder = %LogObscurer::folder(Some(fid), &rec.path),
                    "[FolderContextMenu] Open in File Manager triggered"
                );
                if let Err(e) = SystemFileHandler::open_folder(&rec.path) {
                    tracing::error!(
                        error = %e,
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
                tracing::info!(
                    folder = %LogObscurer::folder(Some(fid), &rec.path),
                    "[FolderContextMenu: Refresh Files Full] Explicit Refresh Files (Full) triggered via context menu"
                );
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
                tracing::info!(
                    folder = %LogObscurer::folder(Some(fid), &rec.path),
                    "[FolderContextMenu: Refresh Files Meta] Explicit Refresh Files (Meta) triggered via context menu"
                );
                BridgeFolders::refresh_folder_metadata_only(
                    fid,
                    PathBuf::from(&rec.path),
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.state,
                );
            } else if action_id == CTX_FOLDER_REFRESH_TREE_RELATIVE {
                tracing::info!(
                    folder = %LogObscurer::folder(Some(fid), &rec.path),
                    "[FolderContextMenu: Refresh Tree Relative] Targeted relative tree scan triggered via context menu"
                );
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
