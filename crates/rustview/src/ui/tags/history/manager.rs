use rustview_core::library::models::{TagId, TagRecord};
use std::collections::{HashMap, HashSet};

/// Dedicated manager for operations on the Tag History collection.
pub struct TagHistoryManager;

impl TagHistoryManager {
    /// Records a tag as used/assigned: moves to front (position 0), deduplicates, and truncates to `max_items`.
    pub fn record_tag_used(history: &mut Vec<TagId>, tag_id: TagId, max_items: usize) {
        history.retain(|&id| id != tag_id);
        history.insert(0, tag_id);
        history.truncate(max_items);
    }

    /// Prunes invalid tag IDs that do not exist in the database.
    pub fn prune_invalid(history: &mut Vec<TagId>, valid_tag_ids: &HashSet<TagId>) {
        history.retain(|id| valid_tag_ids.contains(id));
    }

    /// Prunes deleted tag IDs from history.
    pub fn prune_tags(history: &mut Vec<TagId>, tags_to_remove: &HashSet<TagId>) {
        history.retain(|id| !tags_to_remove.contains(id));
    }

    /// Generates a hierarchical tree tooltip for a given tag without any action text.
    /// Root tags return only their clean name. Child tags return an ASCII tree representation of their lineage.
    pub fn build_hierarchy_tooltip(tag_id: TagId, all_tags_map: &HashMap<TagId, TagRecord>) -> String {
        let Some(tag) = all_tags_map.get(&tag_id) else {
            return String::new();
        };

        let clean_name = tag.name.trim();

        let mut ancestors = Vec::new();
        let mut current_pid = tag.parent_id;

        while let Some(pid) = current_pid {
            if let Some(parent) = all_tags_map.get(&pid) {
                ancestors.push(parent.name.trim().to_string());
                current_pid = parent.parent_id;
            } else {
                break;
            }
        }

        if ancestors.is_empty() {
            clean_name.to_string()
        } else {
            ancestors.reverse();
            ancestors.push(clean_name.to_string());

            let mut tree_repr = String::new();

            for (idx, name) in ancestors.iter().enumerate() {
                if idx == 0 {
                    tree_repr.push_str(name);
                } else {
                    let indent = "   ".repeat(idx.saturating_sub(1));
                    tree_repr.push_str(&format!("\n{}`-- {}", indent, name));
                }
            }

            tree_repr
        }
    }
}
