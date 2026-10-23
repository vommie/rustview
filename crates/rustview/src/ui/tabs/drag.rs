//! Dragging tabs in the tab bar.

use super::{BridgeViewerTabs, TabDragEndParams};
use crate::ui::MainWindow;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::dnd::{TabDndAutoScroller, TabDndGeometry, TabDndSession};
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tabs::icon::tab_icon;
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::RwLock;
use rustview_core::settings::LogObscurer;
use slint::{ComponentHandle, Model};
use std::sync::Arc;

impl BridgeViewerTabs {
    /// Handles tab drag initiation: computes tab widths, creates ghost item preview, and starts DND session.
    pub fn handle_tab_drag_started(
        tab_id: TabId,
        source_index: usize,
        mouse_x: f32,
        tab_width: f32,
        window: &MainWindow,
        tab_manager: &ViewerTabManager,
        state: &Arc<RwLock<UiState>>,
    ) {
        let tabs = tab_manager.list_tabs();
        let total_tabs = tabs.len();
        if total_tabs <= 1 {
            return;
        }

        let Some(source_tab) = tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };

        let tab_ids: Vec<TabId> = tabs.iter().map(|t| t.id).collect();

        let tab_models = window.get_tabs();
        let mut widths = Vec::with_capacity(total_tabs);
        for i in 0..total_tabs {
            let w = if i == source_index && tab_width > 20.0 {
                tab_width
            } else if let Some(t_session) = tabs.get(i) {
                let has_rating = window.get_viewer_show_tab_rating_badge() && t_session.record.rating > 0;
                TabDndGeometry::estimate_tab_width(&t_session.title, has_rating)
            } else {
                120.0
            };
            widths.push(w);
        }

        let slots = TabDndGeometry::compute_tab_slots(&tab_ids, &widths, 4.0, 2.0);
        let grab_offset = if let Some(s) = slots.get(source_index) {
            (mouse_x - s.start_x).clamp(0.0, s.width)
        } else {
            tab_width / 2.0
        };

        let session = TabDndSession::new(tab_id, source_index, grab_offset, mouse_x, slots);

        window.set_is_dragging_tab(true);
        window.set_drag_ghost_x(session.ghost_left_x());
        window.set_drag_ghost_title(source_tab.title.clone().into());
        window.set_drag_ghost_format(source_tab.format.default_extension().into());
        window.set_drag_ghost_icon(tab_icon(source_tab));
        window.set_drag_ghost_rating(source_tab.record.rating as i32);
        window.set_drag_ghost_is_active(Some(tab_id) == tab_manager.active_tab_id());

        for i in 0..tab_models.row_count() {
            if let Some(mut m) = tab_models.row_data(i) {
                let is_ph = m.id == tab_id as i32;
                if m.is_placeholder != is_ph {
                    m.is_placeholder = is_ph;
                    tab_models.set_row_data(i, m);
                }
            }
        }

        state.write().active_tab_dnd_session = Some(session);

        tracing::info!(
            tab_id,
            source_index,
            title = %LogObscurer::filename(&source_tab.title),
            mouse_x,
            "[TabDnd: Start] Tab drag gesture initiated with ghost preview"
        );
    }

    /// Handles pointer movement during tab drag: calculates target slot and applies smooth displacement to neighbor tabs.
    pub fn handle_tab_drag_moved(
        mouse_x: f32,
        view_width: f32,
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        autoscroller: &TabDndAutoScroller,
    ) {
        let (drag_tab_id, source_idx, cur_target_idx, ghost_x, shifts) = {
            let mut st = state.write();
            let Some(ref mut session) = st.active_tab_dnd_session else {
                return;
            };

            session.current_cursor_x = mouse_x;
            let center_x = session.dragged_center_x();
            let new_target_idx = TabDndGeometry::calculate_target_index(
                &session.slots,
                session.source_index,
                center_x,
                session.target_index,
            );

            session.target_index = new_target_idx;
            let shifts =
                TabDndGeometry::compute_shift_offsets(&session.slots, session.source_index, new_target_idx, 4.0);

            (
                session.dragged_tab_id,
                session.source_index,
                new_target_idx,
                session.ghost_left_x(),
                shifts,
            )
        };

        window.set_drag_ghost_x(ghost_x);

        let tab_models = window.get_tabs();
        let total_rows = tab_models.row_count();

        for i in 0..total_rows {
            if let Some(mut item) = tab_models.row_data(i) {
                let target_shift = shifts.get(i).copied().unwrap_or(0.0);
                let is_ph = item.id == drag_tab_id as i32;

                if (item.shift_x - target_shift).abs() > 0.1 || item.is_placeholder != is_ph {
                    item.shift_x = target_shift;
                    item.is_placeholder = is_ph;
                    tab_models.set_row_data(i, item);
                }
            }
        }

        let win_weak = window.as_weak();
        autoscroller.update_cursor(mouse_x, view_width, move |dx| {
            if let Some(w) = win_weak.upgrade() {
                w.invoke_scroll_tab_bar_by(dx);
            }
        });

        tracing::trace!(
            tab_id = drag_tab_id,
            source_index = source_idx,
            target_index = cur_target_idx,
            ghost_x = ghost_x,
            "[TabDnd: Move] Drag moved and neighbor shift offsets updated"
        );
    }

    /// Handles drop release: executes tab reordering in `ViewerTabManager` if target index changed and resyncs UI.
    pub fn handle_tab_drag_ended(params: TabDragEndParams<'_>) {
        params.autoscroller.stop();

        let session_opt = params.state.write().active_tab_dnd_session.take();
        params.window.set_is_dragging_tab(false);

        let Some(session) = session_opt else {
            Self::sync_ui(
                params.window,
                params.tab_manager,
                params.library,
                params.settings,
                params.state,
            );
            return;
        };

        let t_elapsed = session.start_time.elapsed().as_millis();
        let source_idx = session.source_index;
        let target_idx = session.target_index;

        if source_idx != target_idx {
            tracing::info!(
                tab_id = params.tab_id,
                source_index = source_idx,
                target_index = target_idx,
                reorder_time_ms = t_elapsed,
                "[TabDnd: Commit] Committed drag & drop tab reorder"
            );

            params.tab_manager.move_tab(params.tab_id, target_idx);

            let all_tabs = params.tab_manager.list_tabs();
            let title = ViewerTabManager::compute_tab_title(&all_tabs, params.tab_id);
            StatusBarBridge::success(
                params.window,
                format!("Reordered tab '{}' to position {}", title, target_idx + 1),
            );
        } else {
            tracing::debug!(
                tab_id = params.tab_id,
                source_index = source_idx,
                "[TabDnd: NoOp] Tab dropped in original slot"
            );
        }

        Self::sync_ui(
            params.window,
            params.tab_manager,
            params.library,
            params.settings,
            params.state,
        );
        Self::sync_video_embedding(params.window, params.tab_manager, params.fullscreen, params.settings);

        if params.window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(
                params.window,
                params.tab_manager,
                params.library,
                params.settings,
                params.state,
            );
        }
    }
}
