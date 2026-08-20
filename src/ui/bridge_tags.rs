use super::bridge_ctx_menu::BridgeContextMenu;
use super::bridge_ctx_menu_tag::TagContextMenu;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::{MainWindow, TagNodeView};
use crate::ctx_menu::CtxMenuActionId;
use crate::library::models::{MediaId, MediaSearchParams, TagId, TagRecord};
use crate::library::Library;
use crate::query::{MatchMode, ScopeMode};
use crate::settings::{LogObscurer, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use regex::{Regex, RegexBuilder};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TagTreeNode {
    pub record: TagRecord,
    pub children: Vec<TagTreeNode>,
}

#[derive(Debug, Clone, Default)]
pub struct TagSelectionStats {
    pub selected_files_count: usize,
    pub tag_counts: HashMap<TagId, usize>,
    pub ratings_in_selection: HashSet<u8>,
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

        let mut ratings = HashSet::new();
        for &id in selected_ids {
            if let Ok(Some(m)) = library.media().get(id) {
                ratings.insert(m.rating);
            }
        }

        Self {
            selected_files_count: count,
            tag_counts,
            ratings_in_selection: ratings,
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

pub type ShowAddDialogFn = Box<dyn Fn(Option<TagId>, &str)>;
pub type ShowRenameDialogFn = Box<dyn Fn(TagId, &str)>;
pub type ShowDeleteDialogFn = Box<dyn Fn(TagId, &str, Vec<String>)>;

pub struct TagActionDialogs {
    pub show_add: ShowAddDialogFn,
    pub show_rename: ShowRenameDialogFn,
    pub show_delete: ShowDeleteDialogFn,
}

#[derive(Debug, Clone)]
pub struct TagFlattenContext<'a> {
    pub expanded_set: &'a HashSet<TagId>,
    pub selected_query_tags: &'a HashSet<TagId>,
    pub filter_text: &'a str,
    pub filter_regex: Option<&'a Regex>,
    pub is_regex: bool,
    pub stats: &'a TagSelectionStats,
    pub is_assign_mode: bool,
}

pub struct TagTreeBuilder;

impl TagTreeBuilder {
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

    pub fn flatten(
        nodes: &[TagTreeNode],
        depth: i32,
        ctx: &TagFlattenContext<'_>,
        output: &mut Vec<TagNodeView>,
    ) -> bool {
        let mut any_match_in_branch = false;
        let is_filtering = !ctx.filter_text.is_empty();

        for node in nodes {
            let has_children = !node.children.is_empty();
            let is_expanded = ctx.expanded_set.contains(&node.record.id);
            let is_selected =
                !ctx.is_assign_mode && ctx.selected_query_tags.contains(&node.record.id);

            let name_match = if !is_filtering {
                false
            } else if let Some(re) = ctx.filter_regex {
                re.is_match(&node.record.name)
            } else {
                node.record.name.to_lowercase().contains(ctx.filter_text)
            };

            let is_assigned = ctx.is_assign_mode && ctx.stats.is_fully_assigned(node.record.id);
            let is_partial = ctx.is_assign_mode && ctx.stats.is_partially_assigned(node.record.id);
            let formatted_name = if ctx.is_assign_mode {
                ctx.stats.format_display_name(&node.record.name, node.record.id)
            } else {
                node.record.name.clone()
            };

            let start_idx = output.len();

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
            if has_children && (is_expanded || is_filtering) {
                child_matches = Self::flatten(&node.children, depth + 1, ctx, output);
            }

            if is_filtering && !name_match && !child_matches {
                output.truncate(start_idx);
            } else {
                let is_dimmed = is_filtering && !name_match && child_matches;
                output[start_idx].is_dimmed = is_dimmed;
                any_match_in_branch = true;
            }
        }

        any_match_in_branch
    }
}

pub struct TagTreeSync;

impl TagTreeSync {
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

    pub fn sync_in_place(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let current_model = window.get_tags();
        let row_count = current_model.row_count();

        let (
            is_assign,
            selected_query_tags,
            selected_file_ids,
            query_rating,
            match_mode,
            scope_mode,
            is_resettable,
        ) = {
            let s = state.read();
            (
                s.is_assign_mode,
                s.filter.tag_ids.clone(),
                s.file_selection.selected_ids().clone(),
                s.selected_rating,
                s.filter.match_mode,
                s.filter.scope_mode,
                s.is_query_resettable(),
            )
        };

        let stats = TagSelectionStats::compute(&selected_file_ids, library);
        let has_selection = stats.selected_files_count > 0;

        window.set_tag_is_assign_mode(is_assign);
        window.set_tag_match_mode_idx(if match_mode == MatchMode::All { 0 } else { 1 });
        window.set_tag_scope_mode_idx(scope_mode.to_index());
        window.set_has_file_selection(if is_assign { has_selection } else { true });
        window.set_has_active_query(is_resettable);

        if is_assign {
            let mut partial_arr = [false; 6];
            if stats.selected_files_count > 1 && stats.ratings_in_selection.len() > 1 {
                for &r in &stats.ratings_in_selection {
                    if (r as usize) < 6 {
                        partial_arr[r as usize] = true;
                    }
                }
            }
            window.set_partial_ratings(ModelRc::new(VecModel::from(partial_arr.to_vec())));

            if has_selection && stats.ratings_in_selection.len() == 1 {
                let single_r = *stats.ratings_in_selection.iter().next().unwrap();
                window.set_selected_rating(single_r as i32);
            } else if !has_selection {
                window.set_selected_rating(0);
            } else {
                window.set_selected_rating(-1);
            }
        } else {
            window.set_partial_ratings(ModelRc::new(VecModel::from(vec![false; 6])));
            window.set_selected_rating(query_rating as i32);
        }

        if row_count == 0 {
            return;
        }

        let mut modified_rows = 0;

        for idx in 0..row_count {
            if let Some(mut row) = current_model.row_data(idx) {
                let tag_id = row.id as TagId;
                let is_assigned = is_assign && stats.is_fully_assigned(tag_id);
                let is_partial = is_assign && stats.is_partially_assigned(tag_id);
                let is_selected = !is_assign && selected_query_tags.contains(&tag_id);

                let raw_name = Self::strip_count_suffix(row.name.as_str());
                let formatted_name = if is_assign {
                    stats.format_display_name(raw_name, tag_id)
                } else {
                    raw_name.to_string()
                };
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
            mode = if is_assign { "ASSIGN" } else { "QUERY" },
            total_rows = row_count,
            modified_rows = modified_rows,
            selected_files = stats.selected_files_count,
            is_resettable = is_resettable,
            elapsed_us = t0.elapsed().as_micros(),
            "[TagTree: InPlace Selection Sync] Tag & Rating panel state synchronized in-place"
        );
    }
}

pub struct TagActionHandler;

impl TagActionHandler {
    pub fn toggle_tag(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
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
            Self::handle_query_mode_toggle(
                tag_id, window, library, thumbnails, settings, state,
            );
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
                tag = %LogObscurer::tag(Some(tag_id), tag_name),
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
                tag = %LogObscurer::tag(Some(tag_id), tag_name),
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

        // In-place updates without list re-sorting!
        Self::update_grid_tag_badges(selected_ids, window, library, state);
        TagTreeSync::sync_in_place(window, library, state);
        BridgeInspector::update(window, state);
    }

    fn handle_query_mode_toggle(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        {
            let mut st = state.write();
            if st.filter.tag_ids.contains(&tag_id) {
                st.filter.tag_ids.remove(&tag_id);
            } else {
                st.filter.tag_ids.insert(tag_id);
            }
        }

        Self::execute_query_flow(window, library, thumbnails, settings, state);
    }

    /// Surgically updates the tag indicators and tag count values in-place without re-sorting the active view.
    fn update_grid_tag_badges(
        selected_ids: &HashSet<MediaId>,
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
    ) {
        let media_model = window.get_media_items();
        let start_window = window.get_visible_start_index() as usize;
        let visible_count = media_model.row_count();
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
                    let assigned_tags = library
                        .tags()
                        .get_tags_for_media(item.record.id)
                        .unwrap_or_default();
                    let count = assigned_tags.len();
                    item.tags_count = count;
                    item.tags_count_str = count.to_string().into();
                    item.has_tags = count > 0;

                    if let Some(&global_row_idx) = media_row_map.get(&item.record.id)
                        && global_row_idx >= start_window
                        && global_row_idx < start_window + visible_count
                    {
                        media_updates.push((global_row_idx - start_window, count, item.has_tags));
                    }
                }
            }
        }

        for (local_idx, count, has_tags) in media_updates {
            if let Some(mut view) = media_model.row_data(local_idx) {
                view.has_tags = has_tags;
                view.tags_count = count as i32;
                view.tags_str = count.to_string().into();
                media_model.set_row_data(local_idx, view);
            }
        }
    }

    pub fn apply_rating(
        rating: Option<u8>,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (selected_ids, is_assign) = {
            let s = state.read();
            (s.file_selection.selected_ids().clone(), s.is_assign_mode)
        };

        let rating_val = rating.unwrap_or(0);

        if is_assign {
            if selected_ids.is_empty() {
                window.set_status_text("Select one or more files to apply rating.".into());
                return;
            }
            Self::apply_rating_to_ids(
                &selected_ids,
                rating_val,
                window,
                library,
                thumbnails,
                settings,
                state,
            );
        } else {
            let (prev_rating, is_query) = {
                let s = state.read();
                (s.selected_rating, s.is_query_active)
            };

            if prev_rating == rating_val && !is_query && rating_val == 0 {
                return;
            }

            {
                let mut st = state.write();
                st.selected_rating = rating_val;
                st.filter.rating = if rating_val > 0 { Some(rating_val) } else { None };
            }

            Self::execute_query_flow(window, library, thumbnails, settings, state);
        }
    }

    /// Surgically updates rating scores in-place on both data structures and visible UI rows without re-sorting the active view.
    pub fn apply_rating_to_ids(
        ids: &HashSet<MediaId>,
        rating: u8,
        window: &MainWindow,
        library: &Arc<Library>,
        _thumbnails: &Arc<ThumbnailManager>,
        _settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        for &id in ids {
            let _ = library.media().set_rating(id, rating);
        }

        let media_model = window.get_media_items();
        let start_window = window.get_visible_start_index() as usize;
        let visible_count = media_model.row_count();
        let mut row_updates = Vec::new();

        {
            let mut st = state.write();
            let UiState {
                ref mut all_folder_media,
                ref media_row_map,
                ..
            } = *st;

            for item in all_folder_media.iter_mut() {
                if ids.contains(&item.record.id) {
                    item.record.rating = rating;
                    item.rating_str = rating.to_string().into();
                    if let Some(&global_row_idx) = media_row_map.get(&item.record.id)
                        && global_row_idx >= start_window
                        && global_row_idx < start_window + visible_count
                    {
                        row_updates.push((global_row_idx - start_window, item.record.id));
                    }
                }
            }
        }

        for (local_idx, media_id) in row_updates {
            if let Some(mut view) = media_model.row_data(local_idx)
                && view.id == media_id as i32
            {
                view.rating = rating as i32;
                view.rating_str = rating.to_string().into();
                media_model.set_row_data(local_idx, view);
            }
        }

        TagTreeSync::sync_in_place(window, library, state);
        BridgeInspector::update(window, state);

        window.set_status_text(
            if rating == 0 {
                format!("Cleared rating for {} file(s)", ids.len()).into()
            } else {
                format!("Set rating to {} for {} file(s)", rating, ids.len()).into()
            },
        );
    }

    pub fn reset_query(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (current_f_id, current_f_path) = {
            let mut st = state.write();
            st.selected_rating = 0;
            st.filter.rating = None;
            st.filter.tag_ids.clear();
            st.filter.search_query.clear();
            st.last_executed_query.clear();
            st.is_query_active = false;
            (st.current_folder_id, st.current_folder_path.clone())
        };

        window.set_selected_rating(0);
        window.set_search_query("".into());
        window.invoke_scroll_media_to_top();

        if let (Some(f_id), Some(path)) = (current_f_id, current_f_path.as_ref()) {
            let raw_media = library.media().get_by_folder(f_id).unwrap_or_default();
            let indexed = BridgeMedia::preindex(library, raw_media, path);
            let valid_ids: HashSet<MediaId> = indexed.iter().map(|m| m.record.id).collect();

            {
                let mut st = state.write();
                st.all_folder_media = indexed;
                st.file_selection.prune_missing(&valid_ids);
            }
        }

        TagTreeSync::sync_in_place(window, library, state);
        BridgeMedia::reload(window, library, thumbnails, settings, state, true, false);
        BridgeInspector::update(window, state);
        window.set_status_text("Query reset to default folder view.".into());
    }

    pub fn execute_query_flow(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t_flow_start = Instant::now();

        window.set_search_query("".into());
        window.invoke_scroll_media_to_top();

        let (params, current_folder_p) = {
            let mut st = state.write();
            st.filter.search_query.clear();
            st.last_executed_query.clear();

            let is_rating_active = st.selected_rating > 0;
            st.filter.rating = if is_rating_active {
                Some(st.selected_rating)
            } else {
                None
            };

            let is_query = is_rating_active || !st.filter.tag_ids.is_empty();
            st.is_query_active = is_query;

            let params = MediaSearchParams {
                rating: st.filter.rating,
                tag_ids: st.filter.tag_ids.clone(),
                match_mode: st.filter.match_mode,
                scope_mode: st.filter.scope_mode,
                folder_id: st.current_folder_id,
            };

            (params, st.current_folder_path.clone())
        };

        let is_query_active =
            params.rating.map(|r| r > 0).unwrap_or(false) || !params.tag_ids.is_empty();

        if !is_query_active {
            let t_restore = Instant::now();
            {
                let mut st = state.write();
                st.is_query_active = false;
                if let (Some(f_id), Some(path)) = (st.current_folder_id, current_folder_p.as_ref()) {
                    let raw_media = library.media().get_by_folder(f_id).unwrap_or_default();
                    st.all_folder_media = BridgeMedia::preindex(library, raw_media, path);
                    let valid_ids: HashSet<MediaId> =
                        st.all_folder_media.iter().map(|m| m.record.id).collect();
                    st.file_selection.prune_missing(&valid_ids);
                }
            }

            TagTreeSync::sync_in_place(window, library, state);
            BridgeMedia::reload(window, library, thumbnails, settings, state, true, false);
            BridgeInspector::update(window, state);
            window.set_status_text("Query cleared. Regular folder view active.".into());

            tracing::info!(
                restore_us = t_restore.elapsed().as_micros(),
                total_flow_us = t_flow_start.elapsed().as_micros(),
                "[QueryFlow: Inactive] Restored default folder dataset"
            );
            return;
        }

        let t_db = Instant::now();
        let search_results = library.media().search(&params).unwrap_or_default();
        let db_search_us = t_db.elapsed().as_micros();
        let result_count = search_results.len();

        let t_preindex = Instant::now();
        let indexed_results = BridgeMedia::preindex_records(library, search_results);
        let preindex_us = t_preindex.elapsed().as_micros();

        let valid_ids: HashSet<MediaId> = indexed_results.iter().map(|m| m.record.id).collect();

        {
            let mut st = state.write();
            st.all_folder_media = indexed_results;
            st.file_selection.prune_missing(&valid_ids);
        }

        let t_reload = Instant::now();
        TagTreeSync::sync_in_place(window, library, state);
        BridgeMedia::reload(window, library, thumbnails, settings, state, true, false);
        BridgeInspector::update(window, state);
        let reload_us = t_reload.elapsed().as_micros();

        let total_flow_us = t_flow_start.elapsed().as_micros();

        let scope_str = params.scope_mode.as_str();
        if result_count == 0 {
            window.set_status_text(format!("Query ({}) returned no results.", scope_str).into());
        } else {
            window.set_status_text(
                format!(
                    "Query ({}) returned {} result{}.",
                    scope_str,
                    result_count,
                    if result_count == 1 { "" } else { "s" }
                )
                .into(),
            );
        }

        tracing::info!(
            rating = ?params.rating,
            tags_count = params.tag_ids.len(),
            match_mode = ?params.match_mode,
            scope_mode = ?params.scope_mode,
            matched_count = result_count,
            db_search_us = db_search_us,
            preindex_us = preindex_us,
            view_reload_us = reload_us,
            total_flow_us = total_flow_us,
            "[Benchmark: QueryFlow] Complete query evaluation pipeline finished"
        );
    }
}

pub struct BridgeTags;

impl BridgeTags {
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
        let set = settings.clone();
        let st = state.clone();
        window.on_rating_applied(move |rating| {
            Self::apply_rating(
                if rating < 0 { None } else { Some(rating as u8) },
                &win,
                &lib,
                &thumb,
                &set,
                &st,
            );
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_tag_toggled(move |tag_id| {
            Self::toggle_tag(tag_id as u32, &win, &lib, &thumb, &set, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings.clone();
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
            TagTreeSync::sync_in_place(&win, &lib, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_match_mode_changed(move |idx| {
            let t0 = Instant::now();
            let mode = if idx == 0 {
                MatchMode::All
            } else {
                MatchMode::Any
            };

            let should_execute = {
                let mut s = st.write();
                s.filter.match_mode = mode;
                s.is_query_active || s.selected_rating > 0 || !s.filter.tag_ids.is_empty()
            };

            let elapsed_us = t0.elapsed().as_micros();
            tracing::info!(
                ?mode,
                should_execute,
                elapsed_us = elapsed_us,
                "[TagRatePanel: MatchMode] Match mode toggled (Fast-Path active if inactive)"
            );

            if should_execute {
                Self::execute_query_flow(&win, &lib, &thumb, &set, &st);
            } else {
                TagTreeSync::sync_in_place(&win, &lib, &st);
            }
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_scope_mode_changed(move |idx| {
            let t0 = Instant::now();
            let scope = ScopeMode::from_index(idx);

            let should_execute = {
                let mut s = st.write();
                s.filter.scope_mode = scope;
                s.is_query_active || s.selected_rating > 0 || !s.filter.tag_ids.is_empty()
            };

            let elapsed_us = t0.elapsed().as_micros();
            tracing::info!(
                ?scope,
                should_execute,
                elapsed_us = elapsed_us,
                "[TagRatePanel: ScopeMode] Scope mode toggled (Fast-Path active if inactive)"
            );

            if should_execute {
                Self::execute_query_flow(&win, &lib, &thumb, &set, &st);
            } else {
                TagTreeSync::sync_in_place(&win, &lib, &st);
            }
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let st = state.clone();
        window.on_tag_filter_changed(move |query| {
            Self::filter_tags(&win, &lib, &st, &query);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let st = state.clone();
        window.on_tag_regex_toggled(move |enabled| {
            st.write().tag_filter_is_regex = enabled;
            Self::reload_tags(&win, &lib, &st);
        });

        let win = window.clone_strong();
        let lib = library;
        let thumb = thumbnails;
        let set = settings;
        let st = state;
        window.on_tag_reset_clicked(move || {
            Self::reset_query(&win, &lib, &thumb, &set, &st);
        });
    }

    pub fn reload_tags(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let tags = library.tags().list_all_tags().unwrap_or_default();

        let valid_ids: HashSet<TagId> = tags.iter().map(|t| t.id).collect();
        {
            let mut st = state.write();
            st.expanded_tag_ids.retain(|id| valid_ids.contains(id));
        }

        let (is_assign, expanded_set, selected_query_tags, filter_text, is_regex, selected_file_ids) = {
            let s = state.read();
            (
                s.is_assign_mode,
                s.expanded_tag_ids.clone(),
                s.filter.tag_ids.clone(),
                s.tag_filter_query.clone(),
                s.tag_filter_is_regex,
                s.file_selection.selected_ids().clone(),
            )
        };

        let filter_trimmed = filter_text.trim().to_string();
        let filter_lower = filter_trimmed.to_lowercase();

        let regex_matcher = if is_regex && !filter_trimmed.is_empty() {
            match RegexBuilder::new(&filter_trimmed).case_insensitive(true).build() {
                Ok(re) => Some(re),
                Err(e) => {
                    tracing::debug!(
                        query = %LogObscurer::query(&filter_trimmed),
                        error = %e,
                        "[TagFilter: Regex Warning] Invalid regex syntax, falling back to substring matching"
                    );
                    None
                }
            }
        } else {
            None
        };

        let stats = TagSelectionStats::compute(&selected_file_ids, library);
        let tree = TagTreeBuilder::build_hierarchy(&tags);

        let ctx = TagFlattenContext {
            expanded_set: &expanded_set,
            selected_query_tags: &selected_query_tags,
            filter_text: &filter_lower,
            filter_regex: regex_matcher.as_ref(),
            is_regex,
            stats: &stats,
            is_assign_mode: is_assign,
        };

        let mut visual_nodes = Vec::new();
        TagTreeBuilder::flatten(&tree, 0, &ctx, &mut visual_nodes);

        tracing::debug!(
            total_tags_in_db = tags.len(),
            visible_rows = visual_nodes.len(),
            expanded_parents = expanded_set.len(),
            selected_files = stats.selected_files_count,
            filter = %LogObscurer::query(&filter_text),
            is_regex = is_regex,
            elapsed_us = t0.elapsed().as_micros(),
            "[TagTree: Visual Model Sync] Synchronized tag tree to Slint UI"
        );

        window.set_tags(ModelRc::new(VecModel::from(visual_nodes)));
        TagTreeSync::sync_in_place(window, library, state);
    }

    #[inline]
    pub fn sync_tag_selection(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        TagTreeSync::sync_in_place(window, library, state);
    }

    pub fn persist_expanded_tags(settings: &SettingsManager, expanded_set: &HashSet<TagId>) {
        let mut cur_settings = settings.get();
        let mut ids: Vec<u32> = expanded_set.iter().copied().collect();
        ids.sort();
        if cur_settings.tags.expanded_tag_ids != ids {
            cur_settings.tags.expanded_tag_ids = ids;
            settings.save_quiet(&cur_settings);
        }
    }

    pub fn toggle_expand(
        tag_id: TagId,
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        {
            let mut st = state.write();
            if st.expanded_tag_ids.contains(&tag_id) {
                st.expanded_tag_ids.remove(&tag_id);
            } else {
                st.expanded_tag_ids.insert(tag_id);
            }
            Self::persist_expanded_tags(settings, &st.expanded_tag_ids);
        }

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
        state.write().active_tag_dialog_target = None;
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
    pub fn handle_tag_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        dialogs: TagActionDialogs,
    ) {
        TagContextMenu::handle_action(action_id, window, library, settings, state, dialogs);
    }

    #[inline]
    pub fn toggle_tag(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::toggle_tag(tag_id, window, library, thumbnails, settings, state);
    }

    #[inline]
    pub fn apply_rating(
        rating: Option<u8>,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::apply_rating(rating, window, library, thumbnails, settings, state);
    }

    #[inline]
    pub fn apply_rating_to_ids(
        ids: &HashSet<MediaId>,
        rating: u8,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::apply_rating_to_ids(
            ids, rating, window, library, thumbnails, settings, state,
        );
    }

    #[inline]
    pub fn reset_query(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::reset_query(window, library, thumbnails, settings, state);
    }

    #[inline]
    pub fn execute_query_flow(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        TagActionHandler::execute_query_flow(window, library, thumbnails, settings, state);
    }
}
