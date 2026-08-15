use super::bridge_context_menu::BridgeContextMenu;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::{MainWindow, TagNodeView};
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuItem};
use crate::library::Library;
use crate::library::models::{MediaId, TagId, TagRecord};
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

// =============================================================================
// Section 1: Domain & View Data Models
// =============================================================================

#[derive(Debug, Clone)]
pub struct TagTreeNode {
    pub record: TagRecord,
    pub children: Vec<TagTreeNode>,
}

/// Tag frequency and assignment evaluator across active media selections.
#[derive(Debug, Clone, Default)]
pub struct TagSelectionStats {
    pub selected_files_count: usize,
    pub tag_counts: HashMap<TagId, usize>,
}

impl TagSelectionStats {
    pub fn compute(selected_ids: &HashSet<MediaId>, library: &Library) -> Self {
        let count = selected_ids.len();
        if count == 0 {
            return Self::default();
        }

        let media_slice: Vec<MediaId> = selected_ids.iter().copied().collect();
        let tag_counts = library
            .tags()
            .get_tag_frequencies(&media_slice)
            .unwrap_or_default();

        Self {
            selected_files_count: count,
            tag_counts,
        }
    }

    #[inline]
    pub fn is_fully_assigned(&self, tag_id: TagId) -> bool {
        self.selected_files_count > 0 && self.assigned_count(tag_id) == self.selected_files_count
    }

    #[inline]
    pub fn is_partially_assigned(&self, tag_id: TagId) -> bool {
        let count = self.assigned_count(tag_id);
        self.selected_files_count > 1 && count > 0 && count < self.selected_files_count
    }

    #[inline]
    pub fn assigned_count(&self, tag_id: TagId) -> usize {
        self.tag_counts.get(&tag_id).copied().unwrap_or(0)
    }

    pub fn format_display_name(&self, raw_name: &str, tag_id: TagId) -> String {
        let count = self.assigned_count(tag_id);
        if self.selected_files_count > 1 && count > 0 {
            format!("{} ({})", raw_name, count)
        } else {
            raw_name.to_string()
        }
    }
}

/// Encapsulated closure bundle for tag dialog triggers to maintain clean parameter interfaces.
pub struct TagActionDialogs<F1, F2, F3>
where
    F1: Fn(Option<TagId>, &str),
    F2: Fn(TagId, &str),
    F3: Fn(TagId, &str, Vec<String>),
{
    pub show_add: F1,
    pub show_rename: F2,
    pub show_delete: F3,
}

// =============================================================================
// Section 2: TagTreeBuilder (Hierarchy Construction & Visual Flattening)
// =============================================================================

pub struct TagTreeBuilder;

impl TagTreeBuilder {
    /// Builds an alphabetically sorted hierarchical tree from a flat tag slice.
    pub fn build_hierarchy(tags: &[TagRecord]) -> Vec<TagTreeNode> {
        let mut children_map: HashMap<Option<TagId>, Vec<TagRecord>> = HashMap::new();
        for t in tags {
            children_map.entry(t.parent_id).or_default().push(t.clone());
        }

        for list in children_map.values_mut() {
            list.sort_by_key(|a| a.name.to_lowercase());
        }

        Self::build_branch(None, &children_map)
    }

    fn build_branch(
        parent_id: Option<TagId>,
        children_map: &HashMap<Option<TagId>, Vec<TagRecord>>,
    ) -> Vec<TagTreeNode> {
        let mut nodes = Vec::new();
        if let Some(children) = children_map.get(&parent_id) {
            for child in children {
                let sub_nodes = Self::build_branch(Some(child.id), children_map);
                nodes.push(TagTreeNode {
                    record: child.clone(),
                    children: sub_nodes,
                });
            }
        }
        nodes
    }

    /// Flattens the hierarchical tree into a sequence of TagNodeViews for Slint rendering.
    pub fn flatten(
        nodes: &[TagTreeNode],
        depth: i32,
        expanded_set: &HashSet<TagId>,
        selected_query_tags: &HashSet<TagId>,
        filter_text: &str,
        stats: &TagSelectionStats,
        output: &mut Vec<TagNodeView>,
    ) -> bool {
        let mut any_match_in_branch = false;

        for node in nodes {
            let has_children = !node.children.is_empty();
            let is_expanded = expanded_set.contains(&node.record.id);
            let is_selected = selected_query_tags.contains(&node.record.id);
            let name_match =
                !filter_text.is_empty() && node.record.name.to_lowercase().contains(filter_text);

            let is_assigned = stats.is_fully_assigned(node.record.id);
            let is_partial = stats.is_partially_assigned(node.record.id);
            let formatted_name = stats.format_display_name(&node.record.name, node.record.id);

            let start_idx = output.len();

            // Insert tentative node
            output.push(TagNodeView {
                id: node.record.id as i32,
                name: formatted_name.into(),
                depth,
                has_children,
                is_expanded,
                is_selected,
                is_assigned,
                is_partial,
                is_dimmed: false,
            });

            let mut child_matches = false;
            if has_children && (is_expanded || !filter_text.is_empty()) {
                child_matches = Self::flatten(
                    &node.children,
                    depth + 1,
                    expanded_set,
                    selected_query_tags,
                    filter_text,
                    stats,
                    output,
                );
            }

            if !filter_text.is_empty() && !name_match && !child_matches {
                output.truncate(start_idx);
            } else {
                // Dim parent nodes shown solely as hierarchical context for matching descendants
                let is_dimmed = !filter_text.is_empty() && !name_match && child_matches;
                output[start_idx].is_dimmed = is_dimmed;
                any_match_in_branch = true;
            }
        }

        any_match_in_branch
    }
}

// =============================================================================
// Section 3: TagTreeSync (Surgical In-Place Slint Model Synchronization)
// =============================================================================

pub struct TagTreeSync;

impl TagTreeSync {
    /// Strips trailing " (N)" count suffix from display names without allocation.
    #[inline]
    pub fn strip_count_suffix(name: &str) -> &str {
        if let Some(pos) = name.rfind(" (")
            && name.ends_with(')')
            && name[pos + 2..name.len() - 1]
                .chars()
                .all(|c| c.is_ascii_digit())
        {
            &name[..pos]
        } else {
            name
        }
    }

    /// Performs fast in-place mutation on existing rows to eliminate layout reflow and preserve scroll.
    pub fn sync_in_place(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let current_model = window.get_tags();
        let row_count = current_model.row_count();

        if row_count == 0 {
            BridgeTags::reload_tags(window, library, state);
            return;
        }

        let (selected_query_tags, selected_file_ids) = {
            let s = state.read();
            (
                s.filter.tag_ids.clone(),
                s.file_selection.selected_ids().clone(),
            )
        };

        let stats = TagSelectionStats::compute(&selected_file_ids, library);
        let mut modified_rows = 0;

        for idx in 0..row_count {
            if let Some(mut row) = current_model.row_data(idx) {
                let tag_id = row.id as TagId;
                let is_assigned = stats.is_fully_assigned(tag_id);
                let is_partial = stats.is_partially_assigned(tag_id);
                let is_selected = selected_query_tags.contains(&tag_id);

                let raw_name = Self::strip_count_suffix(row.name.as_str());
                let formatted_name = stats.format_display_name(raw_name, tag_id);
                let formatted_slint = formatted_name.into();

                let needs_update = row.name != formatted_slint
                    || row.is_assigned != is_assigned
                    || row.is_partial != is_partial
                    || row.is_selected != is_selected;

                if needs_update {
                    row.name = formatted_slint;
                    row.is_assigned = is_assigned;
                    row.is_partial = is_partial;
                    row.is_selected = is_selected;
                    current_model.set_row_data(idx, row);
                    modified_rows += 1;
                }
            }
        }

        tracing::debug!(
            total_rows = row_count,
            modified_rows = modified_rows,
            selected_files = stats.selected_files_count,
            elapsed_us = t0.elapsed().as_micros(),
            "[TagTree: InPlace Selection Sync] Surgical tag visual update completed"
        );
    }
}

// =============================================================================
// Section 4: TagContextMenu (Context Menu Construction & Reparenting Submenu)
// =============================================================================

pub struct TagContextMenu;

impl TagContextMenu {
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

    pub fn handle_action<F1, F2, F3>(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        dialogs: TagActionDialogs<F1, F2, F3>,
    ) where
        F1: Fn(Option<TagId>, &str),
        F2: Fn(TagId, &str),
        F3: Fn(TagId, &str, Vec<String>),
    {
        window.set_is_context_menu_open(false);
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
                    tracing::info!(parent_id = pid, parent_name = %parent.name, "[TagAction] Triggering Add Child Tag dialog");
                    (dialogs.show_add)(Some(pid), &format!("Parent: {}", parent.name));
                }
            }
            CTX_TAG_RENAME => {
                if let Some(tid) = target_id
                    && let Ok(Some(tag)) = library.tags().get_tag(tid)
                {
                    tracing::info!(tag_id = tid, current_name = %tag.name, "[TagAction] Triggering Rename Tag dialog");
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
                    tracing::info!(tag_id = tid, tag_name = %tag.name, "[TagAction] Triggering Delete Tag dialog");
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
                        tag_id = tid,
                        new_parent_id = dest_parent,
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

// =============================================================================
// Section 5: TagActionHandler (Assignment, Unassignment, Mode & Rating Logic)
// =============================================================================

pub struct TagActionHandler;

impl TagActionHandler {
    /// Toggles a tag in Query Filter mode or executes smart Assign/Unassign on selected files.
    pub fn toggle_tag(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (selected_ids, is_assign) = {
            let s = state.read();
            (s.file_selection.selected_ids().clone(), s.is_assign_mode)
        };

        let tag_name = library
            .tags()
            .get_tag(tag_id)
            .ok()
            .flatten()
            .map(|t| t.name)
            .unwrap_or_else(|| format!("Tag #{}", tag_id));

        if is_assign {
            Self::handle_assign_mode_toggle(
                tag_id,
                &tag_name,
                &selected_ids,
                window,
                library,
                state,
            );
        } else {
            Self::handle_query_mode_toggle(tag_id, window, library, thumbnails, state);
        }
    }

    fn handle_assign_mode_toggle(
        tag_id: TagId,
        tag_name: &str,
        selected_ids: &HashSet<MediaId>,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if selected_ids.is_empty() {
            tracing::warn!(
                tag_id = tag_id,
                "[TagAction: Assign] No media files selected to assign/unassign tag"
            );
            window.set_status_text("Select one or more files to assign/unassign tags.".into());
            return;
        }

        let stats = TagSelectionStats::compute(selected_ids, library);
        let total_selected = selected_ids.len();

        if stats.is_fully_assigned(tag_id) {
            tracing::info!(
                tag_id = tag_id,
                tag_name = %tag_name,
                affected_files = total_selected,
                "[TagAction: Unassign] Removing tag from all selected files"
            );
            for &id in selected_ids {
                let _ = library.tags().remove_tag_from_media(id, tag_id);
            }
            window.set_status_text(
                format!("Removed tag '{}' from {} file(s)", tag_name, total_selected).into(),
            );
        } else {
            tracing::info!(
                tag_id = tag_id,
                tag_name = %tag_name,
                affected_files = total_selected,
                previously_had_it = stats.assigned_count(tag_id),
                "[TagAction: Assign] Assigning tag to all selected files"
            );
            for &id in selected_ids {
                let _ = library.tags().assign_tag(id, tag_id);
            }
            window.set_status_text(
                format!("Assigned tag '{}' to {} file(s)", tag_name, total_selected).into(),
            );
        }

        Self::update_grid_tag_badges(selected_ids, window, library, state);
        TagTreeSync::sync_in_place(window, library, state);
        BridgeInspector::update(window, state);
    }

    fn handle_query_mode_toggle(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let is_active = {
            let mut st = state.write();
            if st.filter.tag_ids.contains(&tag_id) {
                st.filter.tag_ids.remove(&tag_id);
                false
            } else {
                st.filter.tag_ids.insert(tag_id);
                true
            }
        };

        tracing::info!(
            tag_id = tag_id,
            is_selected_in_filter = is_active,
            active_filter_tags = ?state.read().filter.tag_ids,
            "[TagAction: Query] Toggled tag in search filter"
        );

        TagTreeSync::sync_in_place(window, library, state);
        BridgeInspector::update(window, state);
        BridgeMedia::reload(window, library, thumbnails, state, false, false);
    }

    fn update_grid_tag_badges(
        selected_ids: &HashSet<MediaId>,
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
    ) {
        let media_model = window.get_media_items();
        let mut media_updates = Vec::new();

        {
            let mut st = state.write();
            let UiState {
                ref mut all_folder_media,
                ref media_row_map,
                ..
            } = *st;

            for item in all_folder_media.iter_mut() {
                if selected_ids.contains(&item.record.id) {
                    let has_tags = !library
                        .tags()
                        .get_tags_for_media(item.record.id)
                        .unwrap_or_default()
                        .is_empty();
                    item.has_tags = has_tags;

                    if let Some(&row_idx) = media_row_map.get(&item.record.id) {
                        media_updates.push((row_idx, has_tags));
                    }
                }
            }
        }

        for (row_idx, has_tags) in media_updates {
            if let Some(mut view) = media_model.row_data(row_idx) {
                view.has_tags = has_tags;
                media_model.set_row_data(row_idx, view);
            }
        }
    }

    pub fn apply_rating(
        rating: Option<u8>,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (selected_ids, is_assign) = {
            let s = state.read();
            (s.file_selection.selected_ids().clone(), s.is_assign_mode)
        };

        if is_assign {
            if let Some(r) = rating {
                Self::apply_rating_to_ids(&selected_ids, r, window, library, thumbnails, state);
            }
        } else {
            state.write().filter.rating = rating;
            BridgeInspector::update(window, state);
            BridgeMedia::reload(window, library, thumbnails, state, false, false);
        }
    }

    pub fn apply_rating_to_ids(
        ids: &HashSet<MediaId>,
        rating: u8,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        for &id in ids {
            let _ = library.media().set_rating(id, rating);
        }

        let current_folder = state.read().current_folder_id;
        let current_path = state.read().current_folder_path.clone();
        if let (Some(f_id), Some(path)) = (current_folder, current_path) {
            let raw = library.media().get_by_folder(f_id).unwrap_or_default();
            state.write().all_folder_media = BridgeMedia::preindex(library, raw, &path);
        }

        BridgeInspector::update(window, state);
        BridgeMedia::reload(window, library, thumbnails, state, false, false);
    }
}

// =============================================================================
// Section 6: BridgeTags (Unified Facade)
// =============================================================================

pub struct BridgeTags;

impl BridgeTags {
    /// Registers tag-related UI interaction callbacks onto MainWindow.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let st = state.clone();
        window.on_rating_applied(move |rating| {
            Self::apply_rating(
                if rating < 0 { None } else { Some(rating as u8) },
                &win,
                &lib,
                &thumb,
                &st,
            );
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails;
        let st = state.clone();
        window.on_tag_toggled(move |tag_id| {
            Self::toggle_tag(tag_id as u32, &win, &lib, &thumb, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings;
        let st = state.clone();
        window.on_tag_toggle_expand(move |tag_id| {
            Self::toggle_expand(tag_id as u32, &win, &lib, &set, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let st = state.clone();
        window.on_tag_context_menu(move |tag_id, x, y| {
            Self::show_tag_context_menu(tag_id as u32, &win, &lib, &st, x, y);
        });

        let win = window.clone_strong();
        let st = state.clone();
        window.on_tag_root_context_menu(move |x, y| {
            Self::show_root_context_menu(&win, &st, x, y);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let st = state.clone();
        window.on_tag_mode_toggled(move |is_assign| {
            st.write().is_assign_mode = is_assign;
            Self::reload_tags(&win, &lib, &st);
        });

        let win = window.clone_strong();
        let lib = library;
        let st = state;
        window.on_tag_filter_changed(move |query| {
            Self::filter_tags(&win, &lib, &st, &query);
        });
    }

    /// Reloads the entire hierarchical tag tree model into Slint.
    pub fn reload_tags(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let tags = library.tags().list_all_tags().unwrap_or_default();

        let valid_ids: HashSet<TagId> = tags.iter().map(|t| t.id).collect();
        {
            let mut st = state.write();
            st.expanded_tag_ids.retain(|id| valid_ids.contains(id));
        }

        let (expanded_set, selected_query_tags, filter_text, selected_file_ids) = {
            let s = state.read();
            (
                s.expanded_tag_ids.clone(),
                s.filter.tag_ids.clone(),
                s.tag_filter_query.to_lowercase(),
                s.file_selection.selected_ids().clone(),
            )
        };

        let stats = TagSelectionStats::compute(&selected_file_ids, library);
        let tree = TagTreeBuilder::build_hierarchy(&tags);

        let mut visual_nodes = Vec::new();
        TagTreeBuilder::flatten(
            &tree,
            0,
            &expanded_set,
            &selected_query_tags,
            &filter_text,
            &stats,
            &mut visual_nodes,
        );

        tracing::debug!(
            total_tags_in_db = tags.len(),
            visible_rows = visual_nodes.len(),
            expanded_parents = expanded_set.len(),
            selected_files = stats.selected_files_count,
            filter = %filter_text,
            elapsed_us = t0.elapsed().as_micros(),
            "[TagTree: Visual Model Sync] Synchronized tag tree to Slint UI"
        );

        window.set_tags(ModelRc::new(VecModel::from(visual_nodes)));
    }

    /// Surgically updates the visible tag nodes in-place without triggering layout reconstruction.
    #[inline]
    pub fn sync_tag_selection(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        TagTreeSync::sync_in_place(window, library, state);
    }

    /// Persists the active set of expanded tag IDs to configuration storage.
    pub fn persist_expanded_tags(settings: &SettingsManager, expanded_set: &HashSet<TagId>) {
        let mut cur_settings = settings.get();
        let mut ids: Vec<u32> = expanded_set.iter().copied().collect();
        ids.sort();
        if cur_settings.tags.expanded_tag_ids != ids {
            cur_settings.tags.expanded_tag_ids = ids;
            settings.save_quiet(&cur_settings);
            tracing::debug!(
                expanded_count = cur_settings.tags.expanded_tag_ids.len(),
                "[TagTree: Persistence] Saved expanded tag IDs to persistent settings"
            );
        }
    }

    /// Toggles expansion of a single parent tag node.
    pub fn toggle_expand(
        tag_id: TagId,
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let is_now_expanded = {
            let mut st = state.write();
            let expanded = if st.expanded_tag_ids.contains(&tag_id) {
                st.expanded_tag_ids.remove(&tag_id);
                false
            } else {
                st.expanded_tag_ids.insert(tag_id);
                true
            };
            Self::persist_expanded_tags(settings, &st.expanded_tag_ids);
            expanded
        };

        tracing::info!(
            tag_id = tag_id,
            is_expanded = is_now_expanded,
            "[TagTree: NodeToggle] Toggled parent tag expansion and persisted state"
        );

        Self::reload_tags(window, library, state);
    }

    pub fn expand_all(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let all_tags = library.tags().list_all_tags().unwrap_or_default();
        {
            let mut st = state.write();
            for t in all_tags {
                st.expanded_tag_ids.insert(t.id);
            }
            Self::persist_expanded_tags(settings, &st.expanded_tag_ids);
        }
        tracing::info!("[TagTree: ExpandAll] Expanded all parent nodes and persisted state");
        Self::reload_tags(window, library, state);
    }

    pub fn collapse_all(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        {
            let mut st = state.write();
            st.expanded_tag_ids.clear();
            Self::persist_expanded_tags(settings, &st.expanded_tag_ids);
        }
        tracing::info!("[TagTree: CollapseAll] Collapsed all parent nodes and persisted state");
        Self::reload_tags(window, library, state);
    }

    pub fn filter_tags(
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
        filter_text: &str,
    ) {
        state.write().tag_filter_query = filter_text.trim().to_string();
        Self::reload_tags(window, library, state);
    }

    pub fn show_root_context_menu(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        x: f32,
        y: f32,
    ) {
        tracing::info!(
            pos_x = x,
            pos_y = y,
            "[TagTree: ContextMenu] Opening Root Context Menu"
        );
        let menu = TagContextMenu::build_root_menu();
        BridgeContextMenu::open_custom_menu(window, state, menu, x, y);
    }

    pub fn show_tag_context_menu(
        tag_id: TagId,
        window: &MainWindow,
        library: &Library,
        state: &Arc<RwLock<UiState>>,
        x: f32,
        y: f32,
    ) {
        let Ok(Some(record)) = library.tags().get_tag(tag_id) else {
            tracing::warn!(tag_id = tag_id, "Tag not found for context menu");
            return;
        };

        tracing::info!(
            tag_id = tag_id,
            name = %record.name,
            parent_id = ?record.parent_id,
            pos_x = x,
            pos_y = y,
            "[TagTree: ContextMenu] Opening Tag Context Menu"
        );

        state.write().active_tag_dialog_target = Some(tag_id);

        let all_tags = library.tags().list_all_tags().unwrap_or_default();
        let descendants: HashSet<TagId> = library
            .tags()
            .get_descendants(tag_id)
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.id)
            .collect();

        let menu = TagContextMenu::build_tag_menu(&record, &all_tags, &descendants);
        BridgeContextMenu::open_custom_menu(window, state, menu, x, y);
    }

    #[inline]
    pub fn handle_tag_action<F1, F2, F3>(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        dialogs: TagActionDialogs<F1, F2, F3>,
    ) where
        F1: Fn(Option<TagId>, &str),
        F2: Fn(TagId, &str),
        F3: Fn(TagId, &str, Vec<String>),
    {
        TagContextMenu::handle_action(action_id, window, library, settings, state, dialogs);
    }

    #[inline]
    pub fn toggle_tag(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::toggle_tag(tag_id, window, library, thumbnails, state);
    }

    #[inline]
    pub fn apply_rating(
        rating: Option<u8>,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::apply_rating(rating, window, library, thumbnails, state);
    }

    #[inline]
    pub fn apply_rating_to_ids(
        ids: &HashSet<MediaId>,
        rating: u8,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::apply_rating_to_ids(ids, rating, window, library, thumbnails, state);
    }
}
