use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuItem};
use crate::icons::Icon;
use crate::ui::MainWindow;
use crate::ui::state::*;
use crate::ui::tags::{BridgeTags, TagActionDialogs};
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::{TagId, TagRecord};
use rustview_core::settings::{LogObscurer, SettingsManager};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Domain context menu builder and action handler for hierarchical tag tree nodes.
pub struct TagContextMenu;

impl TagContextMenu {
    /// Builds root tag tree context menu.
    pub fn build_root_menu() -> CtxMenu {
        CtxMenuBuilder::new()
            .header("Tags Root")
            .action(CTX_TAG_ADD_ROOT, "Add Root Tag...", None, Some(Icon::Plus), true)
            .separator()
            .action(CTX_TAG_EXPAND_ALL, "Expand All", None, Some(Icon::ExpandAll), true)
            .action(
                CTX_TAG_COLLAPSE_ALL,
                "Collapse All",
                None,
                Some(Icon::CollapseAll),
                true,
            )
            .separator()
            .action(CTX_TAG_REFRESH, "Refresh Tag Tree", None, Some(Icon::Refresh), true)
            .build()
    }

    /// Builds tag item context menu with dynamic reparenting submenu.
    /// Formats the header using sticky prefix and left-side ellipsis to eliminate menu text overflow while attaching the full path as tooltip.
    pub fn build_tag_menu(tag: &TagRecord, all_tags: &[TagRecord], descendants: &HashSet<TagId>) -> CtxMenu {
        let (header_title, tooltip) = CtxMenuBuilder::format_header_title("Tag: ", &tag.name, 26);
        let move_submenu = Self::build_reparent_submenu(tag.id, all_tags, descendants);

        CtxMenuBuilder::new()
            .header_with_tooltip(header_title, tooltip)
            .action(CTX_TAG_ADD_CHILD, "Add Child Tag...", None, Some(Icon::Plus), true)
            .action(CTX_TAG_RENAME, "Rename Tag...", Some("F2"), Some(Icon::Rename), true)
            .item(move_submenu)
            .separator()
            .action(CTX_TAG_DELETE, "Delete Tag...", None, Some(Icon::Delete), true)
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
        sub_items.push(CtxMenuItem::new_action(CTX_TAG_MOVE_TO_ROOT, "Root (Top-level)").with_icon(Icon::TagRoot));
        sub_items.push(CtxMenuItem::new_separator());

        Self::populate_move_nodes(None, &children_map, &mut sub_items);

        CtxMenuItem::new_submenu("Move to...", Some(Icon::MoveTo), true, sub_items)
    }

    fn populate_move_nodes(
        parent_id: Option<TagId>,
        children_map: &HashMap<Option<TagId>, Vec<TagRecord>>,
        output: &mut Vec<CtxMenuItem>,
    ) {
        if let Some(children) = children_map.get(&parent_id) {
            for child in children {
                let mut child_sub_items = Vec::new();
                child_sub_items.push(
                    CtxMenuItem::new_action(
                        CTX_TAG_MOVE_TARGET_BASE + (child.id as CtxMenuActionId),
                        format!("Move under '{}'", child.name),
                    )
                    .with_icon(Icon::MoveTo),
                );

                let has_grandchildren = children_map.get(&Some(child.id)).is_some_and(|g| !g.is_empty());
                if has_grandchildren {
                    child_sub_items.push(CtxMenuItem::new_separator());
                    Self::populate_move_nodes(Some(child.id), children_map, &mut child_sub_items);
                }

                output.push(CtxMenuItem::new_submenu(
                    child.name.clone(),
                    Some(Icon::Tag),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: TagId, parent_id: Option<TagId>, name: &str) -> TagRecord {
        TagRecord {
            id,
            parent_id,
            name: name.to_string(),
        }
    }

    #[test]
    fn every_tag_menu_entry_has_an_icon_and_no_emoji_labels() {
        let all = vec![
            tag(1, None, "Places"),
            tag(2, Some(1), "Cities"),
            tag(3, None, "People"),
        ];
        let menus = [
            TagContextMenu::build_root_menu(),
            TagContextMenu::build_tag_menu(&all[2], &all, &HashSet::new()),
        ];
        for menu in menus {
            assert!(
                menu.entries_without_icon().is_empty(),
                "{:?}",
                menu.entries_without_icon()
            );
        }
    }

    #[test]
    fn move_targets_keep_the_icon_out_of_the_label() {
        let all = vec![
            tag(1, None, "Places"),
            tag(2, Some(1), "Cities"),
            tag(3, None, "People"),
        ];
        let menu = TagContextMenu::build_tag_menu(&all[2], &all, &HashSet::new());
        let move_to = menu.items.iter().find(|i| i.text == "Move to...").expect("submenu");
        let places = move_to
            .children
            .iter()
            .find(|i| i.text == "Places")
            .expect("tag target");
        assert_eq!(places.icon, Some(Icon::Tag));
        let root = move_to
            .children
            .iter()
            .find(|i| i.text == "Root (Top-level)")
            .expect("root target");
        assert_eq!(root.icon, Some(Icon::TagRoot));
    }
}
