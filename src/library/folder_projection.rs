//! Pure in-memory projections of the folder hierarchy snapshot into UI structures
//! (Slint tree rows and "Move to..." / "Copy to..." context menus). Zero disk I/O.

use super::models::{CachedFolderNode, FolderHierarchySnapshot, FolderId};
use crate::ctx_menu::{CtxMenuItem, CtxMenuItemKind};
use std::collections::HashSet;
use std::time::Instant;

/// Maximum number of folder entries displayed per submenu level to prevent screen overflow.
pub const MAX_CHILDREN_PER_MENU_LEVEL: usize = 120;

/// Maximum submenu depth supported by Slint's ContextMenuPopup (level 0 to level 5).
pub const MAX_SUBMENU_DEPTH: usize = 4;

/// Parameters passed during recursive tree flattening to Slint model items.
pub struct FolderTreeFlattenFilter<'a> {
    pub expanded_ids: &'a HashSet<FolderId>,
    pub selected_id: Option<FolderId>,
    pub show_deleted: bool,
    pub show_metrics: bool,
    pub show_hidden: bool,
}

#[inline]
fn is_node_visible(node: &CachedFolderNode, show_deleted: bool, show_hidden: bool) -> bool {
    !(node.is_deleted && !show_deleted) && !(node.is_hidden && !show_hidden)
}

impl FolderHierarchySnapshot {
    /// Projects the hierarchical folder tree into Slint's `FolderNodeView` list. Only expanded branches are emitted.
    pub fn project_tree(
        &self,
        expanded_ids: &HashSet<FolderId>,
        selected_id: Option<FolderId>,
        show_deleted: bool,
        show_metrics: bool,
        show_hidden: bool,
    ) -> Vec<crate::ui::FolderNodeView> {
        let t0 = Instant::now();
        let mut output = Vec::with_capacity(self.nodes.len().min(128));

        let filter = FolderTreeFlattenFilter {
            expanded_ids,
            selected_id,
            show_deleted,
            show_metrics,
            show_hidden,
        };

        self.flatten_into(None, 0, &filter, &mut output);

        tracing::debug!(
            rows = output.len(),
            snapshot_nodes = self.nodes.len(),
            expanded = expanded_ids.len(),
            elapsed_us = t0.elapsed().as_micros() as u64,
            "[FolderTree: Project] Projected folder tree rows from SPoT snapshot"
        );

        output
    }

    fn flatten_into(
        &self,
        parent_id: Option<FolderId>,
        depth: i32,
        filter: &FolderTreeFlattenFilter<'_>,
        output: &mut Vec<crate::ui::FolderNodeView>,
    ) {
        for &child_id in self.get_children(parent_id) {
            let Some(node) = self.get_node(child_id) else {
                continue;
            };

            if !is_node_visible(node, filter.show_deleted, filter.show_hidden) {
                continue;
            }

            let has_children = node.children.iter().any(|cid| {
                self.get_node(*cid)
                    .map(|c| is_node_visible(c, filter.show_deleted, filter.show_hidden))
                    .unwrap_or(false)
            });
            let is_expanded = filter.expanded_ids.contains(&node.id);
            let is_selected = filter.selected_id == Some(node.id);

            let formatted_name = if filter.show_metrics {
                format!(
                    "{} ({}, {})",
                    node.name, node.subfolder_count, node.media_count
                )
            } else {
                node.name.clone()
            };

            output.push(crate::ui::FolderNodeView {
                id: node.id as i32,
                name: formatted_name.into(),
                depth,
                path: node.path.to_string_lossy().to_string().into(),
                has_children,
                is_expanded,
                is_selected,
                is_deleted: node.is_deleted,
                subfolder_count: node.subfolder_count as i32,
                file_count: node.media_count as i32,
            });

            if has_children && is_expanded {
                self.flatten_into(Some(node.id), depth + 1, filter, output);
            }
        }
    }

    /// Single Point of Truth (SPoT): Generates the hierarchical target folder menu for "Move to..." and "Copy to...".
    /// - Top level: Root action item (`📁 Root (<name>)`), separated by a clean divider line, followed by all children of Root.
    /// - Each child with subfolders is rendered as a submenu `📁 <name> ▶`.
    /// - Opening any subfolder presents a top action item `Move directly into '<name>'`, followed by a separator and all its subfolders.
    /// - Inactive/disabled entries receive explicit tooltips detailing the exact reason why moving is forbidden.
    /// - The folder being moved and all of its descendants are strictly excluded from appearing as destinations.
    pub fn project_target_folder_menu(
        &self,
        base_action_id: u64,
        exclude_subtree_id: Option<FolderId>,
        current_parent_id: Option<FolderId>,
        show_hidden: bool,
    ) -> Vec<CtxMenuItem> {
        let t0 = Instant::now();
        let mut menu_items = Vec::new();

        let verb = if base_action_id == crate::ui::bridge_state::CTX_ACTION_COPY_TO_BASE {
            "Copy"
        } else {
            "Move"
        };

        for &root_id in &self.root_ids {
            let Some(root_node) = self.get_node(root_id) else {
                continue;
            };

            // 1. Root action item as the very first entry of the menu (e.g. "📁 Root (vommie)")
            let is_root_current = current_parent_id == Some(root_id);
            let root_tooltip = if is_root_current {
                format!(
                    "Disabled: Item is already located in the Root directory (current location).\nPath: {}",
                    root_node.path.display()
                )
            } else {
                format!(
                    "{} directly into Root directory\nPath: {}",
                    verb,
                    root_node.path.display()
                )
            };

            let root_action = CtxMenuItem {
                id: base_action_id + root_id,
                text: format!("📁 Root ({})", root_node.name),
                shortcut: None,
                icon_name: Some("folder".to_string()),
                icon_path: None,
                kind: CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: !is_root_current,
                children: Vec::new(),
                menu_index: 0,
                tooltip: Some(root_tooltip),
            };
            menu_items.push(root_action);

            // 2. Direct child folders of Root
            let children = self.get_children(Some(root_id));
            let mut eligible_children = Vec::with_capacity(children.len());

            for &child_id in children {
                if self.is_target_eligible(child_id, exclude_subtree_id, show_hidden) {
                    eligible_children.push(child_id);
                }
            }

            // Divider separator under Root entry on the first level
            if !eligible_children.is_empty() {
                menu_items.push(CtxMenuItem::new_separator());
            }

            let count = eligible_children.len();
            let display_slice = &eligible_children[..count.min(MAX_CHILDREN_PER_MENU_LEVEL)];

            for &child_id in display_slice {
                if let Some(item) = self.build_target_folder_item(
                    child_id,
                    base_action_id,
                    exclude_subtree_id,
                    current_parent_id,
                    show_hidden,
                    1,
                ) {
                    menu_items.push(item);
                }
            }

            if count > MAX_CHILDREN_PER_MENU_LEVEL {
                let hidden_count = count - MAX_CHILDREN_PER_MENU_LEVEL;
                menu_items.push(CtxMenuItem {
                    id: 0,
                    text: format!("... ({} more subfolders)", hidden_count),
                    shortcut: None,
                    icon_name: None,
                    icon_path: None,
                    kind: CtxMenuItemKind::Action,
                    is_checked: false,
                    is_enabled: false,
                    children: Vec::new(),
                    menu_index: 0,
                    tooltip: Some(format!(
                        "Notice: {} subfolders in Root are hidden to keep the menu responsive.\nPlease use the Folder Tree in the sidebar to navigate into deeper directories.",
                        hidden_count
                    )),
                });
            }
        }

        tracing::debug!(
            items_count = menu_items.len(),
            snapshot_nodes = self.nodes.len(),
            elapsed_us = t0.elapsed().as_micros() as u64,
            "[FolderTree: TargetMenu] Projected target folder menu from SPoT snapshot with Root separator"
        );

        menu_items
    }

    /// Evaluates if a folder is an eligible move/copy target.
    /// Strictly rejects deleted folders, hidden folders (if disabled), the excluded folder,
    /// and ANY transitive descendant of the excluded folder.
    fn is_target_eligible(
        &self,
        folder_id: FolderId,
        exclude_subtree_id: Option<FolderId>,
        show_hidden: bool,
    ) -> bool {
        let Some(node) = self.get_node(folder_id) else {
            return false;
        };

        if node.is_deleted || (node.is_hidden && !show_hidden) {
            return false;
        }

        // Prevent moving a folder into itself or ANY of its own subdirectories!
        if let Some(excluded) = exclude_subtree_id
            && (folder_id == excluded || self.is_descendant_of(folder_id, excluded))
        {
            return false;
        }

        true
    }

    /// Recursively builds a folder node item:
    /// - If it has no eligible subfolders or has reached MAX_SUBMENU_DEPTH: returns an Action item (`📁 <name>`).
    /// - If it has subfolders: returns a Submenu item (`📁 <name> ▶`) containing a top action (`Move directly into '<name>'`),
    ///   followed by a separator and all its subfolders.
    /// - Any disabled action item receives an explicit reason in its tooltip explaining why it cannot be selected.
    fn build_target_folder_item(
        &self,
        folder_id: FolderId,
        base_action_id: u64,
        exclude_subtree_id: Option<FolderId>,
        current_parent_id: Option<FolderId>,
        show_hidden: bool,
        current_depth: usize,
    ) -> Option<CtxMenuItem> {
        let node = self.get_node(folder_id)?;
        let is_current = current_parent_id == Some(folder_id);

        let verb = if base_action_id == crate::ui::bridge_state::CTX_ACTION_COPY_TO_BASE {
            "Copy"
        } else {
            "Move"
        };

        let children = self.get_children(Some(folder_id));
        let mut eligible_children = Vec::with_capacity(children.len());

        for &child_id in children {
            if self.is_target_eligible(child_id, exclude_subtree_id, show_hidden) {
                eligible_children.push(child_id);
            }
        }

        // If no eligible subfolders or maximum submenu depth reached: Leaf Action item
        if eligible_children.is_empty() || current_depth >= MAX_SUBMENU_DEPTH {
            let tooltip = if is_current {
                format!(
                    "Disabled: Item is already located in '{}' (current location).\nPath: {}",
                    node.name,
                    node.path.display()
                )
            } else if current_depth >= MAX_SUBMENU_DEPTH && !eligible_children.is_empty() {
                format!(
                    "{} directly into '{}'\nPath: {}\n(Subfolders hidden due to maximum menu depth; you can still {} directly into this folder)",
                    verb,
                    node.name,
                    node.path.display(),
                    verb.to_lowercase()
                )
            } else {
                format!(
                    "{} directly into '{}'\nPath: {}",
                    verb,
                    node.name,
                    node.path.display()
                )
            };

            return Some(CtxMenuItem {
                id: base_action_id + node.id,
                text: format!("📁 {}", node.name),
                shortcut: None,
                icon_name: Some("folder".to_string()),
                icon_path: None,
                kind: CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: !is_current,
                children: Vec::new(),
                menu_index: 0,
                tooltip: Some(tooltip),
            });
        }

        // Has subfolders: Submenu with "Move directly into '<name>'" / "Copy directly into '<name>'"
        let direct_action_text = format!("{} directly into '{}'", verb, node.name);
        let direct_tooltip = if is_current {
            format!(
                "Disabled: Item is already located in '{}' (current location).\nPath: {}",
                node.name,
                node.path.display()
            )
        } else {
            format!(
                "{} directly into '{}'\nPath: {}",
                verb,
                node.name,
                node.path.display()
            )
        };

        let direct_action = CtxMenuItem {
            id: base_action_id + node.id,
            text: direct_action_text,
            shortcut: None,
            icon_name: Some("folder".to_string()),
            icon_path: None,
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled: !is_current,
            children: Vec::new(),
            menu_index: 0,
            tooltip: Some(direct_tooltip),
        };

        let count = eligible_children.len();
        let display_slice = &eligible_children[..count.min(MAX_CHILDREN_PER_MENU_LEVEL)];

        let mut sub_elements = Vec::with_capacity(display_slice.len() + 3);
        sub_elements.push(direct_action);
        sub_elements.push(CtxMenuItem::new_separator());

        for &child_id in display_slice {
            if let Some(child_item) = self.build_target_folder_item(
                child_id,
                base_action_id,
                exclude_subtree_id,
                current_parent_id,
                show_hidden,
                current_depth + 1,
            ) {
                sub_elements.push(child_item);
            }
        }

        if count > MAX_CHILDREN_PER_MENU_LEVEL {
            let hidden_count = count - MAX_CHILDREN_PER_MENU_LEVEL;
            sub_elements.push(CtxMenuItem {
                id: 0,
                text: format!("... ({} more subfolders)", hidden_count),
                shortcut: None,
                icon_name: None,
                icon_path: None,
                kind: CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: false,
                children: Vec::new(),
                menu_index: 0,
                tooltip: Some(format!(
                    "Notice: {} subfolders in '{}' are hidden to keep the menu responsive.\nPlease use the Folder Tree in the sidebar to navigate into deeper directories.",
                    hidden_count,
                    node.name
                )),
            });
        }

        Some(CtxMenuItem {
            id: 0,
            text: format!("📁 {}", node.name),
            shortcut: None,
            icon_name: Some("folder".to_string()),
            icon_path: None,
            kind: CtxMenuItemKind::Submenu,
            is_checked: false,
            is_enabled: true,
            children: sub_elements,
            menu_index: 0,
            tooltip: Some(node.path.to_string_lossy().to_string()),
        })
    }
}
