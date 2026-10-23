//! Toggling a tag: assigning or removing it on the selection, or adding it to the query.

use super::TagActionContext;
use super::TagActionHandler;
use super::TagSelectionStats;
use super::TagTreeSync;
use crate::ui::MainWindow;
use crate::ui::browse_window::update_tile;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tags::history_bridge::BridgeTagHistory;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::{MediaId, TagId};
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::SaturatingAs;
use std::collections::HashSet;

impl TagActionHandler {
    pub fn toggle_tag(tag_id: TagId, ctx: &TagActionContext<'_>) {
        let is_tab_active = ctx.window.get_active_view_mode() == 1 || ctx.tab_manager.active_tab_id().is_some();

        let (selected_ids, is_assign_raw) = {
            let s = ctx.state.read();
            let ids = if is_tab_active {
                if let Some(ids) = s.active_tab_selection() {
                    ids
                } else if let Some(m_id) = ctx.tab_manager.get_active_media_id() {
                    HashSet::from([m_id])
                } else {
                    s.file_selection.selected_ids().clone()
                }
            } else {
                s.file_selection.selected_ids().clone()
            };
            (ids, s.is_assign_mode)
        };

        let is_assign = is_assign_raw || is_tab_active;

        let tag_name = ctx
            .library
            .tags()
            .get_tag(tag_id)
            .ok()
            .flatten()
            .map_or_else(|| format!("Tag #{tag_id}"), |t| t.name);

        if is_assign {
            Self::handle_assign_mode_toggle(tag_id, &tag_name, &selected_ids, ctx);
        } else {
            Self::handle_query_mode_toggle(tag_id, ctx);
        }
    }

    pub(in crate::ui::tags) fn handle_assign_mode_toggle(
        tag_id: TagId,
        tag_name: &str,
        selected_ids: &HashSet<MediaId>,
        ctx: &TagActionContext<'_>,
    ) {
        if selected_ids.is_empty() {
            tracing::warn!(
                tag_id = tag_id,
                "[TagAction: Assign] No media files selected to assign/unassign tag"
            );
            StatusBarBridge::info(ctx.window, "Select one or more files to assign/unassign tags.");
            return;
        }

        let stats = TagSelectionStats::compute(selected_ids, ctx.library);
        let total_selected = selected_ids.len();

        if stats.is_fully_assigned(tag_id) {
            tracing::info!(
                tag = %LogObscurer::tag(Some(tag_id), tag_name),
                affected_files = total_selected,
                "[TagAction: Unassign] Removing tag from all selected files"
            );
            for &id in selected_ids {
                let _ = ctx.library.tags().remove_tag_from_media(id, tag_id);
            }
            StatusBarBridge::success(
                ctx.window,
                format!("Removed tag '{tag_name}' from {total_selected} file(s)"),
            );
        } else {
            tracing::info!(
                tag = %LogObscurer::tag(Some(tag_id), tag_name),
                affected_files = total_selected,
                previously_had_it = stats.assigned_count(tag_id),
                "[TagAction: Assign] Assigning tag to all selected files"
            );
            for &id in selected_ids {
                let _ = ctx.library.tags().assign_tag(id, tag_id);
            }
            StatusBarBridge::success(
                ctx.window,
                format!("Assigned tag '{tag_name}' to {total_selected} file(s)"),
            );

            // Record tag assignment in history: automatically push to position 0
            BridgeTagHistory::record_tag_assigned(tag_id, ctx.settings, ctx.state);
        }

        Self::update_grid_tag_badges(selected_ids, ctx.window, ctx.library, ctx.state);
        TagTreeSync::sync_in_place(ctx.window, ctx.library, ctx.state);
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        BridgeTagHistory::sync_ui(ctx.window, ctx.library, ctx.state);
    }

    pub(in crate::ui::tags) fn handle_query_mode_toggle(tag_id: TagId, ctx: &TagActionContext<'_>) {
        {
            let mut st = ctx.state.write();
            if st.filter.tag_ids.contains(&tag_id) {
                st.filter.tag_ids.remove(&tag_id);
            } else {
                st.filter.tag_ids.insert(tag_id);
            }
        }

        Self::execute_query_flow(ctx);
    }

    pub fn update_grid_tag_badges(
        selected_ids: &HashSet<MediaId>,
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
    ) {
        let mut st = state.write();
        let mut counts = Vec::with_capacity(selected_ids.len());
        for &media_id in selected_ids {
            if let Some(item) = st.all_folder_media.find_mut(media_id) {
                let count = library.tags().get_tags_for_media(media_id).unwrap_or_default().len();
                item.set_tags_count(count);
                counts.push((media_id, count));
            }
        }

        for (media_id, count) in counts {
            update_tile(window, &st, media_id, |view| {
                view.has_tags = count > 0;
                view.tags_count = count.saturating_as::<i32>();
                view.tags_str = count.to_string().into();
            });
        }
    }
}
