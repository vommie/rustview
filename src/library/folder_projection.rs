//! Pure in-memory projections of the folder hierarchy snapshot into UI structures
//! (Slint tree rows and "Move to..." / "Copy to..." context menus). Zero disk I/O.

use super::models::{CachedFolderNode, FolderHierarchySnapshot, FolderId};
use crate::ctx_menu::{CtxMenuItem, CtxMenuItemKind};
use crate::icons::Icon;
use std::collections::HashSet;
use std::time::Instant;

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
    /// - Top level: Root action item (`Root (<name>)` with a folder icon), separated by a divider line, followed by all children of Root.
    /// - Tooltips are strictly omitted on normal enabled folders and only generated when an item is disabled or requires explanation.
    /// - Submenu depth and children count are completely unconstrained (arbitrary hierarchy depth supported).
    pub fn project_target_folder_menu(
        &self,
        base_action_id: u64,
        exclude_subtree_id: Option<FolderId>,
        current_parent_id: Option<FolderId>,
        show_hidden: bool,
    ) -> Vec<CtxMenuItem> {
        let t0 = Instant::now();
        let mut menu_items = Vec::new();

        for &root_id in &self.root_ids {
            let Some(root_node) = self.get_node(root_id) else {
                continue;
            };

            // 1. Root action item as the very first entry of the menu (e.g. "Root (vommie)")
            let is_root_current = current_parent_id == Some(root_id);
            let root_tooltip = if is_root_current {
                Some(format!(
                    "Disabled: Item is already located in the Root directory (current location).\nPath: {}",
                    root_node.path.display()
                ))
            } else {
                None // Tooltip completely omitted on normal enabled folder
            };

            let root_action = CtxMenuItem {
                id: base_action_id + root_id,
                text: format!("Root ({})", root_node.name),
                shortcut: None,
                icon: Some(Icon::Folder),
                icon_path: None,
                kind: CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: !is_root_current,
                children: Vec::new(),
                menu_index: 0,
                tooltip: root_tooltip,
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

            // Divider separator under Root entry on the first level if subfolders exist
            if !eligible_children.is_empty() {
                menu_items.push(CtxMenuItem::new_separator());
            }

            for &child_id in &eligible_children {
                if let Some(item) = self.build_target_folder_item(
                    child_id,
                    base_action_id,
                    exclude_subtree_id,
                    current_parent_id,
                    show_hidden,
                ) {
                    menu_items.push(item);
                }
            }
        }

        tracing::debug!(
            items_count = menu_items.len(),
            snapshot_nodes = self.nodes.len(),
            elapsed_us = t0.elapsed().as_micros() as u64,
            "[FolderTree: TargetMenu] Projected target folder menu with Root divider and clean tooltips"
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

    /// Recursively builds a folder node item without depth limits.
    /// - If it has no eligible subfolders: returns an Action item (`<name>` with a folder icon).
    /// - If it has subfolders: returns a Submenu item (`<name>` with a folder icon) containing a top action (`Move directly into '<name>'`),
    ///   followed by a separator and all its subfolders.
    /// - Normal enabled folders have NO tooltip; disabled entries receive explicit explanations.
    fn build_target_folder_item(
        &self,
        folder_id: FolderId,
        base_action_id: u64,
        exclude_subtree_id: Option<FolderId>,
        current_parent_id: Option<FolderId>,
        show_hidden: bool,
    ) -> Option<CtxMenuItem> {
        let node = self.get_node(folder_id)?;
        let is_current = current_parent_id == Some(folder_id);

        let (verb, verb_icon) = if base_action_id == crate::ui::bridge_state::CTX_ACTION_COPY_TO_BASE {
            ("Copy", Icon::Copy)
        } else {
            ("Move", Icon::MoveTo)
        };

        let children = self.get_children(Some(folder_id));
        let mut eligible_children = Vec::with_capacity(children.len());

        for &child_id in children {
            if self.is_target_eligible(child_id, exclude_subtree_id, show_hidden) {
                eligible_children.push(child_id);
            }
        }

        // If no eligible subfolders: Leaf Action item
        if eligible_children.is_empty() {
            let tooltip = if is_current {
                Some(format!(
                    "Disabled: Item is already located in '{}' (current location).\nPath: {}",
                    node.name,
                    node.path.display()
                ))
            } else {
                None // Tooltip completely omitted on normal enabled folder
            };

            return Some(CtxMenuItem {
                id: base_action_id + node.id,
                text: node.name.clone(),
                shortcut: None,
                icon: Some(Icon::Folder),
                icon_path: None,
                kind: CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: !is_current,
                children: Vec::new(),
                menu_index: 0,
                tooltip,
            });
        }

        // Has subfolders: Submenu with "Move directly into '<name>'" / "Copy directly into '<name>'"
        let direct_action_text = format!("{} directly into '{}'", verb, node.name);
        let direct_tooltip = if is_current {
            Some(format!(
                "Disabled: Item is already located in '{}' (current location).\nPath: {}",
                node.name,
                node.path.display()
            ))
        } else {
            None // Tooltip completely omitted on normal direct action
        };

        let direct_action = CtxMenuItem {
            id: base_action_id + node.id,
            text: direct_action_text,
            shortcut: None,
            icon: Some(verb_icon),
            icon_path: None,
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled: !is_current,
            children: Vec::new(),
            menu_index: 0,
            tooltip: direct_tooltip,
        };

        let mut sub_elements = Vec::with_capacity(eligible_children.len() + 2);
        sub_elements.push(direct_action);
        sub_elements.push(CtxMenuItem::new_separator());

        for &child_id in &eligible_children {
            if let Some(child_item) = self.build_target_folder_item(
                child_id,
                base_action_id,
                exclude_subtree_id,
                current_parent_id,
                show_hidden,
            ) {
                sub_elements.push(child_item);
            }
        }

        Some(CtxMenuItem {
            id: 0,
            text: node.name.clone(),
            shortcut: None,
            icon: Some(Icon::Folder),
            icon_path: None,
            kind: CtxMenuItemKind::Submenu,
            is_checked: false,
            is_enabled: true,
            children: sub_elements,
            menu_index: 0,
            tooltip: None,
        })
    }
}
