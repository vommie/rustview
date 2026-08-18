use super::MainWindow;
use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::bridge_folders::BridgeFolders;
use super::bridge_state::*;
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuItem};
use crate::library::models::{FolderId, FolderRecord};
use crate::settings::LogObscurer;
use crate::system::SystemFileHandler;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Domain context menu builder and action handler for folder tree nodes.
pub struct FolderContextMenu;

impl FolderContextMenu {
    /// Builds the declarative context menu for a folder item.
    pub fn build_menu(
        target_folder: &FolderRecord,
        all_folders: &[FolderRecord],
        root_dir: Option<&Path>,
        is_root: bool,
    ) -> CtxMenu {
        let target_path = Path::new(&target_folder.path);
        let folder_name = target_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| target_folder.path.clone());

        let title_header = if target_folder.is_deleted {
            format!("Missing Folder: {}", folder_name)
        } else {
            format!("Folder: {}", folder_name)
        };

        let mut builder = CtxMenuBuilder::new().header(title_header);

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

            let move_submenu = Self::build_reparent_submenu(target_folder, all_folders, root_dir);
            builder = builder.item(move_submenu);
        }

        // Separator above and below Refresh Folder
        builder = builder.separator();
        builder = builder.action(
            CTX_FOLDER_REFRESH,
            "Refresh Folder",
            Some("F5"),
            Some("refresh"),
            !target_folder.is_deleted,
        );
        builder = builder.separator();

        if !is_root {
            if target_folder.is_deleted {
                // Orphaned folder: Show "Remove from Database..."
                builder = builder.action(
                    CTX_FOLDER_PURGE_DB,
                    "Remove from Database...",
                    Some("Del"),
                    Some("trash"),
                    true,
                );
            } else {
                // Active folder: Show standard "Delete Folder..."
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

    /// Builds the hierarchical "Move to..." submenu tree excluding illegal targets and descendants.
    fn build_reparent_submenu(
        current_folder: &FolderRecord,
        all_folders: &[FolderRecord],
        root_dir_opt: Option<&Path>,
    ) -> CtxMenuItem {
        if current_folder.is_deleted {
            return CtxMenuItem::new_submenu("Move to...", Some("move"), false, Vec::new());
        }

        let current_path = Path::new(&current_folder.path);

        let root_record = match root_dir_opt {
            Some(root_dir) => {
                let canonical_root = match root_dir.canonicalize() {
                    Ok(p) => p,
                    Err(_) => root_dir.to_path_buf(),
                };
                all_folders.iter().find(|f| {
                    let p = Path::new(&f.path);
                    p == canonical_root || p == root_dir
                })
            }
            None => all_folders.iter().min_by_key(|f| f.path.len()),
        };

        let Some(root_rec) = root_record else {
            return CtxMenuItem::new_submenu("Move to...", Some("move"), false, Vec::new());
        };

        let root_path = Path::new(&root_rec.path);

        let mut valid_destinations = Vec::new();
        for f in all_folders {
            let f_path = Path::new(&f.path);
            if !f.is_deleted && f.id != current_folder.id && !f_path.starts_with(current_path) {
                valid_destinations.push(f.clone());
            }
        }

        if valid_destinations.is_empty() {
            return CtxMenuItem::new_submenu("Move to...", Some("move"), false, Vec::new());
        }

        let mut subfolder_map: HashMap<Option<String>, Vec<FolderRecord>> = HashMap::new();
        for f in &valid_destinations {
            let p = Path::new(&f.path);
            let parent_str = p
                .parent()
                .map(|parent| parent.to_string_lossy().to_string());
            subfolder_map.entry(parent_str).or_default().push(f.clone());
        }

        for list in subfolder_map.values_mut() {
            list.sort_by_key(|a| a.path.to_lowercase());
        }

        let mut sub_items = Vec::new();

        let is_direct_child_of_root = current_path.parent() == Some(root_path);
        let is_root_itself = current_folder.id == root_rec.id;

        if !is_root_itself && !is_direct_child_of_root {
            let root_display_name = root_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Root".to_string());

            sub_items.push(CtxMenuItem::new_action(
                CTX_FOLDER_MOVE_BASE + (root_rec.id as CtxMenuActionId),
                format!("📁 Root ({})", root_display_name),
            ));
        }

        let root_path_str = Some(root_rec.path.clone());
        let mut child_items = Vec::new();
        Self::populate_move_nodes(&root_path_str, &subfolder_map, &mut child_items);

        if !child_items.is_empty() {
            if !sub_items.is_empty() {
                sub_items.push(CtxMenuItem::new_separator());
            }
            sub_items.extend(child_items);
        }

        let is_enabled = !sub_items.is_empty();
        CtxMenuItem::new_submenu("Move to...", Some("move"), is_enabled, sub_items)
    }

    fn populate_move_nodes(
        parent_path: &Option<String>,
        subfolder_map: &HashMap<Option<String>, Vec<FolderRecord>>,
        output: &mut Vec<CtxMenuItem>,
    ) {
        if let Some(children) = subfolder_map.get(parent_path) {
            for child in children {
                let child_path_str = Some(child.path.clone());
                let has_grandchildren = subfolder_map
                    .get(&child_path_str)
                    .is_some_and(|subs| !subs.is_empty());

                let display_name = Path::new(&child.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| child.path.clone());

                if has_grandchildren {
                    let mut child_sub_items = Vec::new();
                    child_sub_items.push(CtxMenuItem::new_action(
                        CTX_FOLDER_MOVE_BASE + (child.id as CtxMenuActionId),
                        format!("Move directly into '{}'", display_name),
                    ));
                    child_sub_items.push(CtxMenuItem::new_separator());

                    Self::populate_move_nodes(&child_path_str, subfolder_map, &mut child_sub_items);

                    output.push(CtxMenuItem::new_submenu(
                        format!("📁 {}", display_name),
                        None,
                        true,
                        child_sub_items,
                    ));
                } else {
                    output.push(CtxMenuItem::new_action(
                        CTX_FOLDER_MOVE_BASE + (child.id as CtxMenuActionId),
                        format!("📁 {}", display_name),
                    ));
                }
            }
        }
    }

    /// Handles folder context menu action routing.
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

        // 2. Standard Folder Actions (Open in File Manager / Rename / Delete / Purge Orphaned / Refresh)
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
                    window.set_status_text(format!("Failed to open File Manager: {}", e).into());
                } else {
                    window.set_status_text(
                        format!("Opened folder '{}' in File Manager.", name).into(),
                    );
                }
            } else if action_id == CTX_FOLDER_REFRESH {
                tracing::info!(
                    folder = %LogObscurer::folder(Some(fid), &rec.path),
                    "[FolderContextMenu] Refresh Folder action triggered via context menu"
                );
                BridgeFolders::select_folder(
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
