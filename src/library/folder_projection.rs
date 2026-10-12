//! Pure in-memory projections of the folder hierarchy snapshot into UI structures
//! (Slint tree rows and "Move to..." / "Copy to..." context menus). Zero disk I/O.

use super::models::{CachedFolderNode, FolderHierarchySnapshot, FolderId};
use crate::ctx_menu::{CtxMenuItem, CtxMenuItemKind};
use std::collections::{HashSet, VecDeque};
use std::time::Instant;

/// Maximum number of folder entries emitted into a single "Move to..." / "Copy to..." menu tree.
/// Folders are included breadth-first (shallowest first), so deep node_modules-style trees cannot freeze the UI.
pub const MAX_TARGET_MENU_NODES: usize = 2500;

/// Parameters passed during recursive tree flattening to Slint model items.
pub struct FolderTreeFlattenFilter<'a> {
    pub expanded_ids: &'a HashSet<FolderId>,
    pub selected_id: Option<FolderId>,
    pub show_deleted: bool,
    pub show_metrics: bool,
    pub show_hidden: bool,
}

/// Result of the breadth-first menu node selection.
struct MenuSelection {
    ids: HashSet<FolderId>,
    truncated: bool,
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

    /// Single Point of Truth (SPoT): Universal context menu generator for "Move to..." and "Copy to..." folder selection trees.
    /// Operates purely in-memory with zero disk I/O, respecting hidden-folder settings, excluding invalid drop targets
    /// and bounding the menu size to `MAX_TARGET_MENU_NODES` entries (breadth-first).
    pub fn project_target_folder_menu(
        &self,
        base_action_id: u64,
        exclude_subtree_id: Option<FolderId>,
        current_parent_id: Option<FolderId>,
        show_hidden: bool,
    ) -> Vec<CtxMenuItem> {
        let t0 = Instant::now();
        let selection =
            self.select_menu_node_ids(exclude_subtree_id, show_hidden, MAX_TARGET_MENU_NODES);

        let mut menu_items = Vec::new();
        for &root_id in &self.root_ids {
            if let Some(item) = self.build_menu_item_recursive(
                root_id,
                base_action_id,
                current_parent_id,
                &selection.ids,
                true,
            ) {
                menu_items.push(item);
            }
        }

        if selection.truncated {
            menu_items.push(CtxMenuItem {
                id: 0,
                text: "... more folders not shown (menu limit reached)".to_string(),
                shortcut: None,
                icon_name: None,
                icon_path: None,
                kind: CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: false,
                children: Vec::new(),
                menu_index: 0,
                tooltip: Some("Use the folder tree to reach deeper folders".to_string()),
            });
        }

        tracing::debug!(
            included_nodes = selection.ids.len(),
            truncated = selection.truncated,
            snapshot_nodes = self.nodes.len(),
            elapsed_us = t0.elapsed().as_micros() as u64,
            "[FolderTree: TargetMenu] Projected target folder menu from SPoT snapshot"
        );

        menu_items
    }

    /// Breadth-first selection of eligible menu nodes (shallowest folders first) up to `budget` entries.
    fn select_menu_node_ids(
        &self,
        exclude_subtree_id: Option<FolderId>,
        show_hidden: bool,
        budget: usize,
    ) -> MenuSelection {
        let mut ids: HashSet<FolderId> = HashSet::new();
        let mut queue: VecDeque<FolderId> = VecDeque::new();
        let mut truncated = false;

        for &root_id in &self.root_ids {
            if self.is_menu_eligible(root_id, exclude_subtree_id, show_hidden) {
                queue.push_back(root_id);
            }
        }

        while let Some(id) = queue.pop_front() {
            if ids.len() >= budget {
                truncated = true;
                break;
            }
            ids.insert(id);

            for &child_id in self.get_children(Some(id)) {
                if self.is_menu_eligible(child_id, exclude_subtree_id, show_hidden) {
                    queue.push_back(child_id);
                }
            }
        }

        MenuSelection { ids, truncated }
    }

    /// A folder is a valid menu entry if it exists, is not deleted, is visible and is not the excluded subtree root
    /// (which also prevents moving a folder into itself or its descendants).
    fn is_menu_eligible(
        &self,
        node_id: FolderId,
        exclude_subtree_id: Option<FolderId>,
        show_hidden: bool,
    ) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        if node.is_deleted || (node.is_hidden && !show_hidden) {
            return false;
        }
        exclude_subtree_id != Some(node_id)
    }

    fn build_menu_item_recursive(
        &self,
        node_id: FolderId,
        base_action_id: u64,
        current_parent_id: Option<FolderId>,
        included: &HashSet<FolderId>,
        is_root: bool,
    ) -> Option<CtxMenuItem> {
        if !included.contains(&node_id) {
            return None;
        }
        let node = self.get_node(node_id)?;

        // Do not allow moving or copying to the current folder where files already reside
        let is_current_location = current_parent_id == Some(node.id);

        let mut valid_child_items = Vec::new();
        for &child_id in self.get_children(Some(node.id)) {
            if let Some(child_item) = self.build_menu_item_recursive(
                child_id,
                base_action_id,
                current_parent_id,
                included,
                false,
            ) {
                valid_child_items.push(child_item);
            }
        }

        let display_title = if is_root {
            format!("📁 Root ({})", node.name)
        } else {
            format!("📁 {}", node.name)
        };

        if valid_child_items.is_empty() {
            return Some(Self::build_action_item(
                base_action_id,
                node,
                display_title,
                is_current_location,
            ));
        }

        let mut sub_elements = Vec::with_capacity(valid_child_items.len() + 2);
        sub_elements.push(Self::build_action_item(
            base_action_id,
            node,
            format!("Drop directly into '{}'", node.name),
            is_current_location,
        ));
        sub_elements.push(CtxMenuItem::new_separator());
        sub_elements.extend(valid_child_items);

        Some(CtxMenuItem {
            id: 0,
            text: display_title,
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

    fn build_action_item(
        base_action_id: u64,
        node: &CachedFolderNode,
        text: String,
        is_current_location: bool,
    ) -> CtxMenuItem {
        CtxMenuItem {
            id: base_action_id + node.id,
            text,
            shortcut: None,
            icon_name: Some("folder".to_string()),
            icon_path: None,
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled: !is_current_location,
            children: Vec::new(),
            menu_index: 0,
            tooltip: if is_current_location {
                Some("Current location".to_string())
            } else {
                Some(node.path.to_string_lossy().to_string())
            },
        }
    }
}
