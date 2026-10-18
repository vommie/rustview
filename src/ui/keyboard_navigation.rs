use super::bridge_file_selection::BridgeFileSelection;
use super::multiview::MultiviewHandle;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::{BridgeMedia, MediaReloadOptions};
use super::bridge_state::{LastActivePanel, UiState};
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::file_selection::FileSelectionModel;
use super::MainWindow;
use super::multiview::handle::{self as multiview_handle, Pace};
use crate::library::models::MediaId;
use crate::library::Library;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;

/// Supported keyboard navigation directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDirection {
    Up = 0,
    Down = 1,
    Left = 2,
    Right = 3,
    PageUp = 4,
    PageDown = 5,
    Home = 6,
    End = 7,
}

impl NavigationDirection {
    pub fn from_i32(val: i32) -> Option<Self> {
        match val {
            0 => Some(Self::Up),
            1 => Some(Self::Down),
            2 => Some(Self::Left),
            3 => Some(Self::Right),
            4 => Some(Self::PageUp),
            5 => Some(Self::PageDown),
            6 => Some(Self::Home),
            7 => Some(Self::End),
            _ => None,
        }
    }
}

/// Parameter struct grouping keyboard navigation dispatch dependencies.
pub struct KeyboardNavigateParams<'a> {
    pub direction_code: i32,
    pub is_shift: bool,
    pub page_rows: i32,
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub fullscreen: &'a MultiviewHandle,
}

/// Pure mathematical calculation engine for grid and table keyboard navigation.
/// Computes target visual index given current position, layout parameters, and direction.
pub fn calculate_target_index(
    current_idx: usize,
    direction: NavigationDirection,
    total_items: usize,
    columns: usize,
    page_rows: usize,
) -> Option<usize> {
    if total_items == 0 {
        return None;
    }

    let cols = columns.max(1);
    let rows_per_page = page_rows.max(1);
    let total_rows = total_items.div_ceil(cols);

    let current_row = current_idx / cols;
    let current_col = current_idx % cols;

    let target = match direction {
        NavigationDirection::Left => {
            if cols == 1 {
                return None;
            }
            if current_idx > 0 {
                current_idx - 1
            } else {
                return None;
            }
        }
        NavigationDirection::Right => {
            if cols == 1 {
                return None;
            }
            if current_idx + 1 < total_items {
                current_idx + 1
            } else {
                return None;
            }
        }
        NavigationDirection::Up => {
            if cols == 1 {
                if current_idx > 0 {
                    current_idx - 1
                } else {
                    return None;
                }
            } else if current_row > 0 {
                (current_row - 1) * cols + current_col
            } else {
                return None;
            }
        }
        NavigationDirection::Down => {
            if cols == 1 {
                if current_idx + 1 < total_items {
                    current_idx + 1
                } else {
                    return None;
                }
            } else if current_row + 1 < total_rows {
                let candidate = (current_row + 1) * cols + current_col;
                candidate.min(total_items - 1)
            } else {
                return None;
            }
        }
        NavigationDirection::PageUp => {
            if current_row == 0 && current_idx == 0 {
                return None;
            }
            let target_row = current_row.saturating_sub(rows_per_page);
            let candidate = target_row * cols + current_col;
            let res = candidate.min(total_items - 1);
            if res == current_idx && current_idx > 0 {
                0
            } else {
                res
            }
        }
        NavigationDirection::PageDown => {
            let last_idx = total_items - 1;
            if current_row >= total_rows.saturating_sub(1) && current_idx == last_idx {
                return None;
            }
            let target_row = (current_row + rows_per_page).min(total_rows.saturating_sub(1));
            let candidate = target_row * cols + current_col;
            let res = candidate.min(last_idx);
            if res == current_idx && current_idx < last_idx {
                last_idx
            } else {
                res
            }
        }
        NavigationDirection::Home => {
            if current_idx == 0 {
                return None;
            }
            0
        }
        NavigationDirection::End => {
            let last = total_items - 1;
            if current_idx == last {
                return None;
            }
            last
        }
    };

    Some(target)
}

/// Central orchestrator handling media item keyboard navigation events from Slint UI.
pub fn handle_keyboard_navigate(params: KeyboardNavigateParams<'_>) {
    let t0 = Instant::now();

    let Some(direction) = NavigationDirection::from_i32(params.direction_code) else {
        return;
    };

    // The folder rows in front of the media take list positions: the arithmetic of the grid runs on list positions, while the
    // selection only ever moves between media (a folder row is never the target of a key).
    let (total_items, front_len, current_visual_idx_opt) = {
        let st = params.state.read();
        let total = st.active_indices.len();
        if total == 0 {
            return;
        }
        let front = st.folder_front_len();

        let mut idx_opt = None;

        // 1. Prioritize last clicked/focused item
        if let Some(last_id) = st.file_selection.last_clicked_id()
            && let Some(&row_idx) = st.media_row_map.get(&last_id)
            && row_idx < total
        {
            idx_opt = Some(row_idx);
        }

        // 2. Fall back to anchor item
        if idx_opt.is_none()
            && let Some(anchor_id) = st.file_selection.anchor_id()
            && let Some(&row_idx) = st.media_row_map.get(&anchor_id)
            && row_idx < total
        {
            idx_opt = Some(row_idx);
        }

        // 3. Fall back to first selected item in visual order
        if idx_opt.is_none() {
            idx_opt = st.file_selection.selected_ids().iter().filter_map(|id| st.media_row_map.get(id).copied()).min();
        }

        (total, front, idx_opt.map(|row| row + front))
    };

    let is_table = params.window.get_is_table_view();
    let columns = if is_table {
        1
    } else {
        (params.window.get_grid_columns() as usize).max(1)
    };

    let target_idx = match current_visual_idx_opt {
        Some(cur_idx) => {
            match calculate_target_index(
                cur_idx,
                direction,
                front_len + total_items,
                columns,
                params.page_rows.max(1) as usize,
            ) {
                Some(t) => t.clamp(front_len, front_len + total_items - 1),
                None => return,
            }
        }
        None => {
            if direction == NavigationDirection::End {
                front_len + total_items.saturating_sub(1)
            } else {
                front_len
            }
        }
    };
    if Some(target_idx) == current_visual_idx_opt {
        return;
    }
    let target_row = target_idx - front_len;

    let target_media_id = {
        let st = params.state.read();
        let orig_idx = st.active_indices[target_row];
        st.all_folder_media[orig_idx].record.id
    };

    // Update selection state and mark last active panel as Media
    let (changed_ids, sel_count) = {
        let mut st = params.state.write();
        st.last_active_panel = LastActivePanel::Media;
        let prev_selected = st.file_selection.selected_ids().clone();

        if params.is_shift {
            let visible_ids: Vec<MediaId> = st
                .active_indices
                .iter()
                .filter_map(|&idx| st.all_folder_media.get(idx))
                .map(|item| item.record.id)
                .collect();

            st.file_selection
                .select_range(target_media_id, &visible_ids, false);
        } else {
            st.file_selection.select_single(target_media_id);
        }

        let newly_selected = st.file_selection.selected_ids();
        let changed = FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected);
        let count = st.file_selection.count();
        (changed, count)
    };

    let need_slice_reanchor = !params.state.read().ring.range().contains(&target_idx);

    if need_slice_reanchor {
        BridgeMedia::reload_with_options(
            params.window,
            params.library,
            params.thumbnails,
            params.settings,
            params.state,
            MediaReloadOptions::with_anchor(false, false, target_idx),
        );
    } else if !changed_ids.is_empty() {
        BridgeFileSelection::sync_selection_diff(params.window, params.state, &changed_ids);
    }

    params.window.invoke_scroll_media_item_into_view(target_idx as i32);

    BridgeInspector::update(params.window, params.state);
    BridgeTags::sync_tag_selection(params.window, params.library, params.state);
    StatusBarBridge::update_metrics(params.window, params.state);
    multiview_handle::on_user_selection_changed(Pace::Immediate);

    tracing::debug!(
        direction = ?direction,
        is_shift = params.is_shift,
        target_visual_idx = target_idx,
        target_media_id,
        selected_count = sel_count,
        columns,
        page_rows = params.page_rows,
        is_table,
        elapsed_us = t0.elapsed().as_micros(),
        "[KeyboardNavigation: Move] Media item navigated successfully via keyboard"
    );
}
