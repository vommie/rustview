//! The two actions of the menu of several selected tabs: merge them into one Multiview tab, close them.

use super::BridgeViewerTabs;
use crate::ui::context_menu::tab::TabActionContext;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tabs::merge_limit::{MergeLimitDialog, texts};
use crate::viewer::{MergeError, MergeRefusal, TabId, ViewerTabManager};

impl BridgeViewerTabs {
    /// The selected tabs in tab order.
    fn selected_tab_ids(ctx: &TabActionContext<'_>) -> Vec<TabId> {
        let order: Vec<TabId> = ctx.tab_manager.list_tabs().iter().map(|t| t.id).collect();
        ctx.state.read().tab_selection.in_order(&order)
    }

    /// Brings every view in line after tabs came or went: tab bar, tab view, fullscreen binding, tab grid.
    fn sync_after_tab_change(ctx: &TabActionContext<'_>) {
        Self::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
        Self::sync_video_embedding(ctx.window, ctx.tab_manager, ctx.fullscreen, ctx.settings);
        ctx.fullscreen.sync_tab(ctx.tab_manager);
        if ctx.window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
        }
    }

    /// "Merge Selected Tabs": one new Multiview tab with the media of all selected tabs (their players keep running). The new tab is
    /// shown unless the tab grid is open. When the media do not fit, nothing changes and a dialog says how many have to leave first.
    pub fn merge_selected_tabs(ctx: &TabActionContext<'_>) {
        let ids = Self::selected_tab_ids(ctx);
        let in_grid = ctx.window.get_active_view_mode() == 2;
        let titles_before = ctx.tab_manager.list_tabs();
        match ctx.tab_manager.merge_tabs(&ids, !in_grid) {
            Ok(merged) => {
                ctx.state.write().tab_selection.clear();
                Self::sync_after_tab_change(ctx);
                let all_tabs = ctx.tab_manager.list_tabs();
                let title = ViewerTabManager::compute_tab_title(&all_tabs, merged.tab);
                tracing::info!(
                    tab_id = merged.tab,
                    merged = merged.closed.len(),
                    tiles = merged.tiles,
                    "[BridgeViewerTabs: Merge] The selected tabs were merged"
                );
                StatusBarBridge::success(
                    ctx.window,
                    format!(
                        "Merged {} tabs into '{title}' ({})",
                        merged.closed.len(),
                        texts::media(merged.tiles)
                    ),
                );
            }
            Err(MergeError::Refused(refusal @ MergeRefusal::TooManyMedia { .. })) => {
                let MergeRefusal::TooManyMedia {
                    total,
                    cap,
                    ref sources,
                } = refusal
                else {
                    return;
                };
                let rows: Vec<(String, usize)> = sources
                    .iter()
                    .map(|s| (ViewerTabManager::compute_tab_title(&titles_before, s.tab), s.tiles))
                    .collect();
                tracing::info!(
                    total,
                    cap,
                    excess = refusal.excess(),
                    tabs = rows.len(),
                    "[BridgeViewerTabs: Merge] Refused, the media do not fit into one Multiview"
                );
                let message = texts::message(total, cap, refusal.excess());
                if !MergeLimitDialog::show(ctx.window, &message, &texts::breakdown(&rows)) {
                    StatusBarBridge::warn(ctx.window, message.replace("\n\n", " "));
                }
            }
            Err(MergeError::Refused(MergeRefusal::TooFewTabs)) => {
                StatusBarBridge::warn(ctx.window, "Select at least two tabs to merge them.");
            }
            Err(MergeError::ShownInFullscreen) => {
                StatusBarBridge::warn(ctx.window, "A selected tab is shown in the fullscreen. Close it first.");
            }
            Err(MergeError::NotReady) => {
                StatusBarBridge::warn(ctx.window, "Tabs cannot be merged yet.");
            }
        }
    }

    /// "Close Selected Tabs".
    pub fn close_selected_tabs(ctx: &TabActionContext<'_>) {
        let ids = Self::selected_tab_ids(ctx);
        let (closed, _) = ctx.tab_manager.close_tabs(&ids);
        ctx.state.write().tab_selection.clear();
        tracing::info!(
            closed,
            "[BridgeViewerTabs: CloseSelected] The selected tabs were closed"
        );
        Self::sync_after_tab_change(ctx);
        if closed > 0 {
            StatusBarBridge::success(ctx.window, format!("Closed {closed} selected tab(s)"));
        }
    }
}
