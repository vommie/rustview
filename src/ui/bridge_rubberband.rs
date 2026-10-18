use super::bridge_file_selection::BridgeFileSelection;
use super::browse_folders::{BrowseItem, FolderRowRef, SelectionDelta};
use crate::library::models::{FolderId, MediaId};
use std::collections::HashSet;
use super::bridge_inspector::BridgeInspector;
use super::bridge_state::{LastActivePanel, UiState};
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::file_selection::FileSelectionModel;
use super::rubberband_selection::{
    GridLayoutConfig, RubberbandAutoScroller, RubberbandGeometry, RubberbandStartParams,
    TableLayoutConfig,
};
use super::MainWindow;
use super::multiview::handle::{self as multiview_handle, Pace};
use crate::library::Library;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::sync::Arc;
use std::time::Instant;

/// Parameter struct grouping rubberband drag initiation arguments.
pub struct RubberbandDragStartParams<'a> {
    pub start_content_x: f32,
    pub start_content_y: f32,
    pub view_x: f32,
    pub view_y: f32,
    pub is_ctrl: bool,
    pub is_shift: bool,
    pub window: &'a MainWindow,
    pub state: &'a Arc<RwLock<UiState>>,
    pub autoscroller: &'a RubberbandAutoScroller,
}

/// Parameter struct grouping rubberband move event arguments.
pub struct RubberbandDragMoveParams<'a> {
    pub content_x: f32,
    pub content_y: f32,
    pub view_x: f32,
    pub view_y: f32,
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub autoscroller: &'a RubberbandAutoScroller,
}

/// Centralized coordinator for Rubberband / Marquee selection across Grid and Table views.
pub struct BridgeRubberband;

impl BridgeRubberband {
    /// Handles the initiation of rubberband selection from background touch area.
    pub fn handle_started(params: RubberbandDragStartParams<'_>) {
        let t0 = Instant::now();
        params.autoscroller.stop();

        let baseline = {
            let mut st = params.state.write();
            st.last_active_panel = LastActivePanel::Media;
            let current_sel = st.file_selection.selected_ids().clone();
            let baseline_folders = st.folder_rows.selection_snapshot().0;
            st.rubberband_session.start(RubberbandStartParams {
                start_content_x: params.start_content_x,
                start_content_y: params.start_content_y,
                view_x: params.view_x,
                view_y: params.view_y,
                is_ctrl: params.is_ctrl,
                is_shift: params.is_shift,
                baseline: current_sel.clone(),
                baseline_folders,
            });
            current_sel
        };

        params.window.set_rubberband_active(true);
        params.window.set_rubberband_x(params.start_content_x);
        params.window.set_rubberband_y(params.start_content_y);
        params.window.set_rubberband_width(0.0);
        params.window.set_rubberband_height(0.0);

        tracing::info!(
            start_content_x = params.start_content_x,
            start_content_y = params.start_content_y,
            is_ctrl = params.is_ctrl,
            is_shift = params.is_shift,
            baseline_count = baseline.len(),
            elapsed_us = t0.elapsed().as_micros(),
            "[Rubberband: Start] Initiated rectangular area selection"
        );
    }

    /// Handles pointer movement and updates the selection rectangle and intersecting items.
    pub fn handle_moved(params: RubberbandDragMoveParams<'_>) {
        let is_active = params.state.read().rubberband_session.is_active;
        if !is_active {
            return;
        }

        // 1. Update session coordinates
        {
            let mut st = params.state.write();
            st.last_active_panel = LastActivePanel::Media;
            st.rubberband_session.update_content_point(
                params.content_x,
                params.content_y,
                params.view_x,
                params.view_y,
            );
        }

        // 2. Sync visual rectangle to Slint UI
        let rect = params.state.read().rubberband_session.content_rect();
        params.window.set_rubberband_x(rect.x);
        params.window.set_rubberband_y(rect.y);
        params.window.set_rubberband_width(rect.width);
        params.window.set_rubberband_height(rect.height);

        // 3. Compute intersecting items and update selection
        Self::evaluate_selection(params.window, params.library, params.state);

        // 4. Edge Auto-Scroller check
        let win_weak = params.window.as_weak();
        let lib_weak = params.library.clone();
        let st_weak = params.state.clone();

        params.autoscroller.update_cursor(params.window, params.view_y, move |dy| {
            if let Some(w) = win_weak.upgrade() {
                w.invoke_scroll_media_by(dy);
                let new_viewport_y = w.invoke_get_media_viewport_y();

                let (last_vx, last_vy) = {
                    let st_r = st_weak.read();
                    (st_r.rubberband_session.last_view_x, st_r.rubberband_session.last_view_y)
                };

                let new_content_y = last_vy - new_viewport_y;
                let new_content_x = last_vx;

                {
                    let mut st = st_weak.write();
                    st.last_active_panel = LastActivePanel::Media;
                    st.rubberband_session.update_content_point(new_content_x, new_content_y, last_vx, last_vy);
                }

                let new_rect = st_weak.read().rubberband_session.content_rect();
                w.set_rubberband_x(new_rect.x);
                w.set_rubberband_y(new_rect.y);
                w.set_rubberband_width(new_rect.width);
                w.set_rubberband_height(new_rect.height);

                Self::evaluate_selection(&w, &lib_weak, &st_weak);
            }
        });
    }

    /// Evaluates current rectangle intersection against dataset items and synchronizes selection.
    fn evaluate_selection(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let is_table = window.get_is_table_view();

        let (rect, is_ctrl, is_shift, baseline, baseline_folders) = {
            let st = state.read();
            (
                st.rubberband_session.content_rect(),
                st.rubberband_session.is_ctrl,
                st.rubberband_session.is_shift,
                st.rubberband_session.baseline_selection.clone(),
                st.rubberband_session.baseline_folders.clone(),
            )
        };

        let st = state.read();
        let total_items = st.view_len();

        let hits = if is_table {
            let col_w_sum = window.get_col_filename_width()
                + window.get_col_path_width()
                + window.get_col_modified_date_width()
                + window.get_col_created_date_width()
                + window.get_col_size_width()
                + window.get_col_rating_width()
                + window.get_col_tags_width()
                + window.get_col_width_width()
                + window.get_col_height_width()
                + window.get_col_ar_width()
                + window.get_col_duration_width()
                + window.get_col_format_width()
                + window.get_col_flags_width();

            let config = TableLayoutConfig { row_height: 28.0, content_width: col_w_sum };
            RubberbandGeometry::table_hit_positions(rect, total_items, config)
        } else {
            let thumb_size = window.get_thumb_size() as f32;
            let aspect_idx = window.get_thumb_aspect_idx();
            let padding_h = window.get_grid_item_padding_h() as f32;
            let padding_v = window.get_grid_item_padding_v() as f32;
            let spacing = window.get_grid_spacing() as f32;
            let padding = window.get_grid_padding() as f32;

            let thumb_h = match aspect_idx {
                0 => thumb_size,
                1 => (thumb_size * 3.0 / 4.0).round(),
                2 => (thumb_size * 4.0 / 5.0).round(),
                3 => (thumb_size * 9.0 / 16.0).round(),
                4 => (thumb_size * 4.0 / 3.0).round(),
                5 => (thumb_size * 5.0 / 4.0).round(),
                6 => (thumb_size * 16.0 / 9.0).round(),
                _ => (thumb_size * 3.0 / 4.0).round(),
            };

            let tile_w = thumb_size + (padding_h * 2.0);
            let tile_h = thumb_h + (padding_v * 2.0) + 22.0;

            let columns = window.get_grid_columns().max(1) as usize;

            let config = GridLayoutConfig {
                columns,
                item_tile_width: tile_w,
                item_tile_height: tile_h,
                grid_spacing: spacing,
                grid_padding: padding,
            };
            RubberbandGeometry::grid_hit_positions(rect, total_items, config)
        };

        // What lies under the rectangle: media and folder rows ("..." is never selected by a rubber band).
        let mut intersected: HashSet<MediaId> = HashSet::new();
        let mut intersected_folders: HashSet<FolderId> = HashSet::new();
        for position in hits {
            match st.browse_item_at(position) {
                Some(BrowseItem::Media(id)) => {
                    intersected.insert(id);
                }
                Some(BrowseItem::Folder(FolderRowRef::Folder(id))) => {
                    intersected_folders.insert(id);
                }
                _ => {}
            }
        }

        drop(st);

        let target_selection = RubberbandGeometry::evaluate_modifier_selection(&baseline, &intersected, is_ctrl, is_shift);
        let target_folders =
            RubberbandGeometry::evaluate_modifier_selection(&baseline_folders, &intersected_folders, is_ctrl, is_shift);

        let delta = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let before_media = st.file_selection.selected_ids().clone();
            let before_folders = st.folder_rows.selection_snapshot();
            st.file_selection.set_selection(target_selection);
            st.folder_rows.set_folders(target_folders);
            SelectionDelta {
                media: FileSelectionModel::compute_selection_diff(&before_media, st.file_selection.selected_ids()),
                folders_changed: before_folders != st.folder_rows.selection_snapshot(),
            }
        };

        if !delta.is_empty() {
            BridgeFileSelection::sync_selection_delta(window, state, &delta);
            BridgeInspector::update(window, state);
            BridgeTags::sync_tag_selection(window, library, state);
            StatusBarBridge::update_metrics(window, state);
            if !delta.media.is_empty() {
                multiview_handle::on_user_selection_changed(Pace::Settled);
            }
        }
    }

    /// Handles mouse release, concluding the rubberband selection session and committing state.
    pub fn handle_ended(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        autoscroller: &RubberbandAutoScroller,
    ) {
        autoscroller.stop();

        let final_count = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            st.rubberband_session.reset();
            st.file_selection.count()
        };

        window.set_rubberband_active(false);
        window.set_rubberband_width(0.0);
        window.set_rubberband_height(0.0);

        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        tracing::info!(
            selected_count = final_count,
            "[Rubberband: End] Finished rectangular area selection"
        );
    }
}
