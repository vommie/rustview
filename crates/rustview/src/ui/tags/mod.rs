pub mod dialogs;
pub mod history;
pub mod history_bridge;

use crate::ctx_menu::CtxMenuActionId;
use crate::ui::MainWindow;
use crate::ui::context_menu::BridgeContextMenu;
use crate::ui::context_menu::tag::TagContextMenu;
use crate::ui::state::*;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use regex::RegexBuilder;
use rustview_core::library::Library;
use rustview_core::library::models::{MediaId, TagId};
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{Model, ModelRc, VecModel};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

pub mod handler;
pub mod selection_stats;
pub mod tree;
pub use handler::TagActionHandler;
pub use selection_stats::TagSelectionStats;
pub use tree::{TagFlattenContext, TagTreeBuilder, TagTreeNode, TagTreeSync};

mod callbacks;

pub const MAX_TAG_ITEM_WIDTH: f32 = 180.0;
pub const MIN_TAG_ITEM_WIDTH: f32 = 60.0;
pub const TAG_ITEM_HEIGHT: f32 = 22.0;
pub const TAG_ROW_SPACING: f32 = 2.0;
pub const TAG_COLUMN_SPACING: f32 = 6.0;
pub const TAG_PADDING_H: f32 = 4.0;
pub const TAG_PADDING_V: f32 = 4.0;

/// Grouped dependencies bundle for tag and rating actions.
pub struct TagActionContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
}

pub type ShowAddDialogFn = Box<dyn Fn(Option<TagId>, &str)>;
pub type ShowRenameDialogFn = Box<dyn Fn(TagId, &str)>;
pub type ShowDeleteDialogFn = Box<dyn Fn(TagId, &str, Vec<String>)>;

pub struct TagActionDialogs {
    pub show_add: ShowAddDialogFn,
    pub show_rename: ShowRenameDialogFn,
    pub show_delete: ShowDeleteDialogFn,
}

pub struct BridgeTags;

impl BridgeTags {
    pub fn reload_tags(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let tags = library.tags().list_all_tags().unwrap_or_default();

        let valid_ids: HashSet<TagId> = tags.iter().map(|t| t.id).collect();
        {
            let mut st = state.write();
            st.expanded_tag_ids.retain(|id| valid_ids.contains(id));
        }

        let is_tab_mode = window.get_active_view_mode() == 1;

        let (is_assign_raw, expanded_set, selected_query_tags, filter_text, is_regex, selected_file_ids, available_h) = {
            let s = state.read();
            let file_ids = if is_tab_mode {
                s.active_tab_selection().unwrap_or_default()
            } else {
                s.file_selection.selected_ids().clone()
            };

            (
                s.is_assign_mode,
                s.expanded_tag_ids.clone(),
                s.filter.tag_ids.clone(),
                s.tag_filter_query.clone(),
                s.tag_filter_is_regex,
                file_ids,
                s.tag_tree_available_height,
            )
        };

        let is_assign = is_assign_raw || is_tab_mode;
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

        let (tag_columns, rows_per_col) = TagTreeBuilder::chunk_into_columns(visual_nodes, available_h);
        {
            let mut st = state.write();
            st.tag_tree_rows_per_column = rows_per_col;
        }

        window.set_tag_columns(ModelRc::new(VecModel::from(tag_columns)));
        TagTreeSync::sync_in_place(window, library, state);

        tracing::debug!(
            total_tags_in_db = tags.len(),
            total_columns = window.get_tag_columns().row_count(),
            rows_per_column = rows_per_col,
            expanded_parents = expanded_set.len(),
            selected_files = stats.selected_files_count,
            filter = %LogObscurer::query(&filter_text),
            is_regex = is_regex,
            elapsed_us = t0.elapsed().as_micros(),
            "[TagTree: Visual Model Sync] Synchronized tag tree to Slint UI with native dynamic column layouts"
        );
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

    pub fn expand_all(window: &MainWindow, library: &Library, settings: &SettingsManager, state: &RwLock<UiState>) {
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

    pub fn collapse_all(window: &MainWindow, library: &Library, settings: &SettingsManager, state: &RwLock<UiState>) {
        {
            let mut st = state.write();
            st.expanded_tag_ids.clear();
            Self::persist_expanded_tags(settings, &st.expanded_tag_ids);
        }
        Self::reload_tags(window, library, state);
    }

    pub fn filter_tags(window: &MainWindow, library: &Library, state: &RwLock<UiState>, filter_text: &str) {
        state.write().tag_filter_query = filter_text.trim().to_string();
        Self::reload_tags(window, library, state);
    }

    pub fn show_root_context_menu(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        tab_manager: &ViewerTabManager,
        x: f32,
        y: f32,
    ) {
        state.write().active_tag_dialog_target = None;
        let menu = TagContextMenu::build_root_menu();
        BridgeContextMenu::open_custom_menu(window, state, tab_manager, menu, x, y);
    }

    pub fn show_tag_context_menu(
        tag_id: TagId,
        window: &MainWindow,
        library: &Library,
        state: &Arc<RwLock<UiState>>,
        tab_manager: &ViewerTabManager,
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
        BridgeContextMenu::open_custom_menu(window, state, tab_manager, menu, x, y);
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
    pub fn toggle_tag(tag_id: TagId, ctx: &TagActionContext<'_>) {
        TagActionHandler::toggle_tag(tag_id, ctx);
    }

    #[inline]
    pub fn apply_rating(rating: Option<u8>, ctx: &TagActionContext<'_>) {
        TagActionHandler::apply_rating(rating, ctx);
    }

    #[inline]
    pub fn apply_rating_to_ids(ids: &HashSet<MediaId>, rating: u8, ctx: &TagActionContext<'_>) {
        TagActionHandler::apply_rating_to_ids(ids, rating, ctx);
    }

    #[inline]
    pub fn reset_query(ctx: &TagActionContext<'_>) {
        TagActionHandler::reset_query(ctx);
    }

    #[inline]
    pub fn execute_query_flow(ctx: &TagActionContext<'_>) {
        TagActionHandler::execute_query_flow(ctx);
    }
}
