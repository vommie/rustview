//! The tag tree: building the hierarchy, flattening it into columns of tag items and syncing the Slint model in place.

use super::TagSelectionStats;
use super::{TAG_ITEM_HEIGHT, TAG_PADDING_V, TAG_ROW_SPACING};
use crate::ui::state::UiState;
use crate::ui::tags::history_bridge::BridgeTagHistory;
use crate::ui::{MainWindow, TagColumnView, TagNodeView};
use parking_lot::RwLock;
use regex::Regex;
use rustview_core::library::Library;
use rustview_core::library::models::{TagId, TagRecord};
use rustview_core::query::MatchMode;
use rustview_core::util::cast::{FloatToInt, WrappingAs};
use slint::{Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TagTreeNode {
    pub record: TagRecord,
    pub children: Vec<TagTreeNode>,
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

    pub(super) fn build_branch(
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
            let is_selected = !ctx.is_assign_mode && ctx.selected_query_tags.contains(&node.record.id);

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
                id: node.record.id.wrapping_as::<i32>(),
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

    pub fn chunk_into_columns(nodes: Vec<TagNodeView>, available_height: f32) -> (Vec<TagColumnView>, usize) {
        let total_items = nodes.len();
        if total_items == 0 {
            return (Vec::new(), 1);
        }

        let usable_h = (available_height - (TAG_PADDING_V * 2.0)).max(TAG_ITEM_HEIGHT);
        let stride_y = TAG_ITEM_HEIGHT + TAG_ROW_SPACING;
        let rows_per_col = ((usable_h + TAG_ROW_SPACING) / stride_y)
            .floor()
            .max(1.0)
            .to_int::<usize>();

        let mut columns = Vec::new();
        for chunk in nodes.chunks(rows_per_col) {
            columns.push(TagColumnView {
                tags: ModelRc::new(VecModel::from(chunk.to_vec())),
            });
        }

        (columns, rows_per_col)
    }
}

pub struct TagTreeSync;

impl TagTreeSync {
    #[inline]
    pub fn strip_count_suffix(name: &str) -> &str {
        if let Some(pos) = name.rfind(" (")
            && name.ends_with(')')
            && name[pos + 2..name.len() - 1].chars().all(|c| c.is_ascii_digit())
        {
            &name[..pos]
        } else {
            name
        }
    }

    pub fn sync_in_place(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let current_columns = window.get_tag_columns();
        let col_count = current_columns.row_count();
        let is_tab_mode = window.get_active_view_mode() == 1;

        let (
            is_assign_raw,
            selected_query_tags,
            selected_file_ids,
            query_rating,
            match_mode,
            scope_mode,
            is_resettable,
        ) = {
            let s = state.read();
            let file_ids = if is_tab_mode {
                s.active_tab_selection().unwrap_or_default()
            } else {
                s.file_selection.selected_ids().clone()
            };

            (
                s.is_assign_mode,
                s.filter.tag_ids.clone(),
                file_ids,
                s.selected_rating,
                s.filter.match_mode,
                s.filter.scope_mode,
                s.is_query_resettable(),
            )
        };

        let is_assign = is_assign_raw || is_tab_mode;
        let stats = TagSelectionStats::compute(&selected_file_ids, library);
        let has_selection = stats.selected_files_count > 0;

        window.set_tag_is_assign_mode(is_assign);
        window.set_tag_match_mode_idx(i32::from(match_mode != MatchMode::All));
        window.set_tag_scope_mode_idx(scope_mode.to_index());
        window.set_has_file_selection(if is_assign { has_selection } else { true });
        window.set_has_active_query(!is_tab_mode && is_resettable);

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
                window.set_selected_rating(i32::from(single_r));
            } else if !has_selection {
                window.set_selected_rating(0);
            } else {
                window.set_selected_rating(-1);
            }
        } else {
            window.set_partial_ratings(ModelRc::new(VecModel::from(vec![false; 6])));
            window.set_selected_rating(i32::from(query_rating));
        }

        if col_count > 0 {
            let mut modified_rows = 0;
            let mut total_rows = 0;

            for col_idx in 0..col_count {
                if let Some(col_view) = current_columns.row_data(col_idx) {
                    let col_model = col_view.tags;
                    let rows_in_col = col_model.row_count();
                    total_rows += rows_in_col;

                    for row_idx in 0..rows_in_col {
                        if let Some(mut row) = col_model.row_data(row_idx) {
                            let tag_id = row.id.wrapping_as::<TagId>();
                            let is_assigned = is_assign && stats.is_fully_assigned(tag_id);
                            let is_partial = is_assign && stats.is_partially_assigned(tag_id);
                            let is_selected = !is_assign && selected_query_tags.contains(&tag_id);

                            let raw_name = Self::strip_count_suffix(row.name.as_str());
                            let formatted_name = if is_assign {
                                stats.format_display_name(raw_name, tag_id)
                            } else {
                                raw_name.to_string()
                            };

                            let formatted_slint: slint::SharedString = formatted_name.into();
                            if row.name != formatted_slint
                                || row.is_assigned != is_assigned
                                || row.is_partial != is_partial
                                || row.is_selected != is_selected
                            {
                                row.name = formatted_slint;
                                row.is_assigned = is_assigned;
                                row.is_partial = is_partial;
                                row.is_selected = is_selected;
                                col_model.set_row_data(row_idx, row);
                                modified_rows += 1;
                            }
                        }
                    }
                }
            }

            tracing::debug!(
                mode = if is_assign { "ASSIGN" } else { "QUERY" },
                is_tab_mode = is_tab_mode,
                total_columns = col_count,
                total_rows = total_rows,
                modified_rows = modified_rows,
                selected_files = stats.selected_files_count,
                elapsed_us = t0.elapsed().as_micros(),
                "[TagTree: InPlace Selection Sync] Tag & Rating panel state synchronized in-place with native column layouts"
            );
        }

        // Keep Assigned Tags History panel synchronized in-place
        BridgeTagHistory::sync_ui(window, library, state);
    }
}
