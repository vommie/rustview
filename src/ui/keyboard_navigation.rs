use super::bridge_file_selection::BridgeFileSelection;
use super::multiview::MultiviewHandle;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::{BridgeMedia, MediaReloadOptions};
use super::bridge_state::{LastActivePanel, UiState};
use super::browse_folders::BrowseItem;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::MainWindow;
use super::multiview::handle::{self as multiview_handle, Pace};
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

/// Central orchestrator handling keyboard navigation events from Slint UI. The keys move over the whole list of the view, folder rows
/// ("..." included) and media alike; with Shift the selection grows from the anchor over everything in between.
pub fn handle_keyboard_navigate(params: KeyboardNavigateParams<'_>) {
    let t0 = Instant::now();

    let Some(direction) = NavigationDirection::from_i32(params.direction_code) else {
        return;
    };

    let (view_len, current_position) = {
        let st = params.state.read();
        let total = st.view_len();
        if total == 0 {
            return;
        }
        (total, st.browse_cursor().and_then(|item| st.browse_position_of(item)))
    };

    let is_table = params.window.get_is_table_view();
    let columns = if is_table {
        1
    } else {
        (params.window.get_grid_columns() as usize).max(1)
    };

    let target_idx = match current_position {
        Some(current) => match calculate_target_index(current, direction, view_len, columns, params.page_rows.max(1) as usize) {
            Some(target) => target.min(view_len - 1),
            None => return,
        },
        None => {
            if direction == NavigationDirection::End {
                view_len - 1
            } else {
                0
            }
        }
    };
    if Some(target_idx) == current_position {
        return;
    }

    // Update selection state and mark last active panel as Media
    let (delta, target_item) = {
        let mut st = params.state.write();
        let Some(item) = st.browse_item_at(target_idx) else {
            return;
        };
        st.last_active_panel = LastActivePanel::Media;
        (st.browse_click(item, false, params.is_shift), item)
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
    } else {
        BridgeFileSelection::sync_selection_delta(params.window, params.state, &delta);
    }

    params.window.invoke_scroll_media_item_into_view(target_idx as i32);

    BridgeInspector::update(params.window, params.state);
    BridgeTags::sync_tag_selection(params.window, params.library, params.state);
    StatusBarBridge::update_metrics(params.window, params.state);
    if !delta.media.is_empty() || matches!(target_item, BrowseItem::Media(_)) {
        multiview_handle::on_user_selection_changed(Pace::Immediate);
    }

    tracing::debug!(
        direction = ?direction,
        is_shift = params.is_shift,
        target_visual_idx = target_idx,
        target = ?target_item,
        columns,
        page_rows = params.page_rows,
        is_table,
        elapsed_us = t0.elapsed().as_micros(),
        "[KeyboardNavigation: Move] Browse item navigated successfully via keyboard"
    );
}
