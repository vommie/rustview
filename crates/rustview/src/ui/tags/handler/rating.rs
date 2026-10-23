//! Rating the selected media.

use super::TagActionContext;
use super::TagActionHandler;
use super::TagTreeSync;
use crate::ui::browse_window::update_tile;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use rustview_core::library::models::MediaId;
use std::collections::HashSet;

impl TagActionHandler {
    pub fn apply_rating(rating: Option<u8>, ctx: &TagActionContext<'_>) {
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
        let rating_val = rating.unwrap_or(0);

        if is_assign {
            if selected_ids.is_empty() {
                StatusBarBridge::info(ctx.window, "Select one or more files to apply rating.");
                return;
            }
            Self::apply_rating_to_ids(&selected_ids, rating_val, ctx);
        } else {
            let (prev_rating, is_query) = {
                let s = ctx.state.read();
                (s.selected_rating, s.is_query_active)
            };

            if prev_rating == rating_val && !is_query && rating_val == 0 {
                return;
            }

            {
                let mut st = ctx.state.write();
                st.selected_rating = rating_val;
                st.filter.rating = if rating_val > 0 { Some(rating_val) } else { None };
            }

            Self::execute_query_flow(ctx);
        }
    }

    /// Stores `rating` for `ids` and brings every place that shows it up to date. Returns the media that were rated (a medium
    /// the library does not know is skipped).
    pub fn apply_rating_to_ids(ids: &HashSet<MediaId>, rating: u8, ctx: &TagActionContext<'_>) -> HashSet<MediaId> {
        let mut rated = HashSet::with_capacity(ids.len());
        for &id in ids {
            match ctx.library.media().set_rating(id, rating) {
                Ok(()) => {
                    rated.insert(id);
                }
                Err(e) => tracing::warn!(media_id = id, error = %e, "[Rating] The rating could not be stored"),
            }
            if let Ok(Some(updated)) = ctx.library.media().get(id) {
                ctx.tab_manager.notify_media_refreshed(id, &updated);
            }
        }

        {
            let mut st = ctx.state.write();
            for &media_id in &rated {
                if let Some(item) = st.all_folder_media.find_mut(media_id) {
                    item.record.rating = rating;
                }
            }
            for &media_id in &rated {
                update_tile(ctx.window, &st, media_id, |view| {
                    view.rating = i32::from(rating);
                    view.rating_str = rating.to_string().into();
                });
            }
        }

        TagTreeSync::sync_in_place(ctx.window, ctx.library, ctx.state);
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);

        StatusBarBridge::success(
            ctx.window,
            if rating == 0 {
                format!("Cleared rating for {} file(s)", rated.len())
            } else {
                format!("Set rating to {} for {} file(s)", rating, rated.len())
            },
        );
        rated
    }
}
