use super::MainWindow;
use super::bridge_state::*;
use super::bridge_tags::{BridgeTags, TagActionDialogs};
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuItem};
use crate::library::models::{TagId, TagRecord};
use crate::library::Library;
use crate::settings::{LogObscurer, SettingsManager};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Domain context menu builder and action handler for hierarchical tag tree nodes.
pub struct TagContextMenu;

impl TagContextMenu {
    /// Builds root tag tree context menu.
    pub fn build_root_menu() -> CtxMenu {
        CtxMenuBuilder::new()
            .header("Tags Root")
            .action(
                CTX_TAG_ADD_ROOT,
                "Add Root Tag...",
                None,
                Some("plus"),
                true,
            )
            .separator()
            .action(CTX_TAG_EXPAND_ALL, "Expand All", None, None, true)
            .action(CTX_TAG_COLLAPSE_ALL, "Collapse All", None, None, true)
            .separator()
            .action(
                CTX_TAG_REFRESH,
                "Refresh Tag Tree",
                None,
                Some("refresh"),
                true,
            )
            .build()
    }

    /// Builds tag item context menu with dynamic reparenting submenu.
    pub fn build_tag_menu(
        tag: &TagRecord,
        all_tags: &[TagRecord],
        descendants: &HashSet<TagId>,
    ) -> CtxMenu {
        let move_submenu = Self::build_reparent_submenu(tag.id, all_tags, descendants);

        CtxMenuBuilder::new()
            .header(format!("Tag: {}", tag.name))
            .action(
                CTX_TAG_ADD_CHILD,
                "Add Child Tag...",
                None,
                Some("plus"),
                true,
            )
            .action(
                CTX_TAG_RENAME,
                "Rename Tag...",
                Some("F2"),
                Some("edit"),
                true,
            )
            .item(move_submenu)
            .separator()
            .action(
                CTX_TAG_DELETE,
                "Delete Tag...",
                Some("Del"),
                Some("trash"),
                true,
            )
            .build()
    }

    fn build_reparent_submenu(
        current_tag_id: TagId,
        all_tags: &[TagRecord],
        descendants: &HashSet<TagId>,
    ) -> CtxMenuItem {
        let mut children_map: HashMap<Option<TagId>, Vec<TagRecord>> = HashMap::new();
        for t in all_tags {
            if t.id != current_tag_id && !descendants.contains(&t.id) {
                children_map.entry(t.parent_id).or_default().push(t.clone());
            }
        }

        for list in children_map.values_mut() {
            list.sort_by_key(|a| a.name.to_lowercase());
        }

        let mut sub_items = Vec::new();
        sub_items.push(CtxMenuItem::new_action(
            CTX_TAG_MOVE_TO_ROOT,
            "📁 Root (Top-level)",
        ));
        sub_items.push(CtxMenuItem::new_separator());

        Self::populate_move_nodes(None, &children_map, &mut sub_items);

        CtxMenuItem::new_submenu("Move to...", Some("move"), true, sub_items)
    }

    fn populate_move_nodes(
        parent_id: Option<TagId>,
        children_map: &HashMap<Option<TagId>, Vec<TagRecord>>,
        output: &mut Vec<CtxMenuItem>,
    ) {
        if let Some(children) = children_map.get(&parent_id) {
            for child in children {
                let mut child_sub_items = Vec::new();
                child_sub_items.push(CtxMenuItem::new_action(
                    CTX_TAG_MOVE_TARGET_BASE + (child.id as CtxMenuActionId),
                    format!("Move under '{}'", child.name),
                ));

                let has_grandchildren = children_map
                    .get(&Some(child.id))
                    .is_some_and(|g| !g.is_empty());
                if has_grandchildren {
                    child_sub_items.push(CtxMenuItem::new_separator());
                    Self::populate_move_nodes(Some(child.id), children_map, &mut child_sub_items);
                }

                output.push(CtxMenuItem::new_submenu(
                    format!("📁 {}", child.name),
                    None,
                    true,
                    child_sub_items,
                ));
            }
        }
    }

    /// Handles tag context menu action routing.
    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        dialogs: TagActionDialogs,
    ) {
        let target_id = state.read().active_tag_dialog_target;

        match action_id {
            CTX_TAG_ADD_ROOT => {
                tracing::info!("[TagAction] Triggering Add Root Tag dialog");
                (dialogs.show_add)(None, "Creating as Root Tag");
            }
            CTX_TAG_ADD_CHILD => {
                if let Some(pid) = target_id
                    && let Ok(Some(parent)) = library.tags().get_tag(pid)
                {
                    tracing::info!(
                        parent = %LogObscurer::tag(Some(pid), &parent.name),
                        "[TagAction] Triggering Add Child Tag dialog"
                    );
                    (dialogs.show_add)(Some(pid), &format!("Parent: {}", parent.name));
                }
            }
            CTX_TAG_RENAME => {
                if let Some(tid) = target_id
                    && let Ok(Some(tag)) = library.tags().get_tag(tid)
                {
                    tracing::info!(
                        tag = %LogObscurer::tag(Some(tid), &tag.name),
                        "[TagAction] Triggering Rename Tag dialog"
                    );
                    (dialogs.show_rename)(tid, &tag.name);
                }
            }
            CTX_TAG_DELETE => {
                if let Some(tid) = target_id
                    && let Ok(Some(tag)) = library.tags().get_tag(tid)
                {
                    let children = library
                        .tags()
                        .get_descendants(tid)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|t| t.name)
                        .collect();
                    tracing::info!(
                        tag = %LogObscurer::tag(Some(tid), &tag.name),
                        "[TagAction] Triggering Delete Tag dialog"
                    );
                    (dialogs.show_delete)(tid, &tag.name, children);
                }
            }
            CTX_TAG_MOVE_TO_ROOT => {
                if let Some(tid) = target_id {
                    tracing::info!(tag_id = tid, "[TagAction] Moving tag to Root");
                    let _ = library.tags().move_tag(tid, None);
                    BridgeTags::reload_tags(window, library, state);
                }
            }
            CTX_TAG_EXPAND_ALL => BridgeTags::expand_all(window, library, settings, state),
            CTX_TAG_COLLAPSE_ALL => BridgeTags::collapse_all(window, library, settings, state),
            CTX_TAG_REFRESH => BridgeTags::reload_tags(window, library, state),
            id if id >= CTX_TAG_MOVE_TARGET_BASE => {
                let dest_parent = (id - CTX_TAG_MOVE_TARGET_BASE) as TagId;
                if let Some(tid) = target_id {
                    tracing::info!(
                        tag = %LogObscurer::tag_id(tid),
                        new_parent = %LogObscurer::tag_id(dest_parent),
                        "[TagAction] Moving tag under parent"
                    );
                    let _ = library.tags().move_tag(tid, Some(dest_parent));
                    {
                        let mut st = state.write();
                        st.expanded_tag_ids.insert(dest_parent);
                        BridgeTags::persist_expanded_tags(settings, &st.expanded_tag_ids);
                    }
                    BridgeTags::reload_tags(window, library, state);
                }
            }
            _ => {}
        }
    }
}
