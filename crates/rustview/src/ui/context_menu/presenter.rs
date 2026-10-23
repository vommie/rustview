use crate::ctx_menu::{CtxMenu, CtxMenuDispatcher, CtxMenuItem, CtxMenuItemKind};
use crate::ui::{ContextMenuLevelModel, MainWindow};
use parking_lot::Mutex;
use rustview_core::util::cast::{Az, SaturatingAs};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::{Arc, LazyLock};

pub const MENU_WIDTH: f32 = 230.0;
pub const MENU_WINDOW_MARGIN: f32 = 8.0;

#[derive(Debug, Clone)]
struct OpenLevelInfo {
    invoking_menu_idx: usize,
    x: f32,
    y: f32,
    width: f32,
}

#[derive(Default)]
struct PresenterState {
    open_levels: Vec<OpenLevelInfo>,
    active_menu: Option<CtxMenu>,
}

static ACTIVE_PRESENTER_STATE: LazyLock<Arc<Mutex<PresenterState>>> =
    LazyLock::new(|| Arc::new(Mutex::new(PresenterState::default())));

/// Dedicated presenter responsible for rendering and managing multi-level context menu popups in Slint.
/// Supports arbitrary submenu depth, dynamic vertical scrollbars (clamped to max 70% window height),
/// accurate Y-level alignment relative to the invoking parent row, and active state highlights for open submenus.
pub struct CtxMenuPresenter;

impl CtxMenuPresenter {
    /// Opens the context menu at the specified screen coordinates and displays Level 0.
    pub fn open(window: &MainWindow, active_menu_slot: &mut Option<CtxMenu>, mut menu: CtxMenu, x: f32, y: f32) {
        menu.index_items();

        let scale = window.window().scale_factor();
        let win_size = window.window().size();
        let win_w = (win_size.width.az::<f32>() / scale).max(200.0);
        let win_h = (win_size.height.az::<f32>() / scale).max(200.0);

        // Max height capped at 70% of window height (desktop UX best practice)
        let max_h = (win_h * 0.70).max(180.0).min(win_h - (MENU_WINDOW_MARGIN * 2.0));
        let content_h = Self::calculate_items_content_height(&menu.items);
        let effective_h = content_h.min(max_h);

        let clamped_x = if x + MENU_WIDTH > win_w - MENU_WINDOW_MARGIN {
            (win_w - MENU_WIDTH - MENU_WINDOW_MARGIN).max(MENU_WINDOW_MARGIN)
        } else {
            x.max(MENU_WINDOW_MARGIN)
        };

        let clamped_y = if y + effective_h > win_h - MENU_WINDOW_MARGIN {
            (win_h - effective_h - MENU_WINDOW_MARGIN).max(MENU_WINDOW_MARGIN)
        } else {
            y.max(MENU_WINDOW_MARGIN)
        };

        let level0 = ContextMenuLevelModel {
            depth: 0,
            active_menu_index: 0,
            x: clamped_x,
            y: clamped_y,
            width: MENU_WIDTH,
            max_height: max_h,
            height: effective_h,
            items: CtxMenuDispatcher::items_to_slint_model(&menu.items, 0),
        };

        {
            let mut state = ACTIVE_PRESENTER_STATE.lock();
            state.active_menu = Some(menu.clone());
            state.open_levels = vec![OpenLevelInfo {
                invoking_menu_idx: 0,
                x: clamped_x,
                y: clamped_y,
                width: MENU_WIDTH,
            }];
        }

        *active_menu_slot = Some(menu);

        window.set_context_menu_levels(ModelRc::new(VecModel::from(vec![level0])));
        window.set_is_context_menu_open(true);
    }

    /// Handles submenu requests at arbitrary depth:
    /// - Aligns submenu top with the invoking item's Y-position when space permits.
    /// - Restricts menu height to 70% of window height with vertical scrolling when needed.
    /// - Flips to the left if overflowing window width on the right.
    /// - Avoids destroying rows when a submenu is already open to preserve hover states.
    pub fn handle_submenu_request(
        window: &MainWindow,
        _active_menu: Option<&CtxMenu>,
        depth: usize,
        visible_row_y: f32,
        menu_idx: usize,
    ) {
        let mut state = ACTIVE_PRESENTER_STATE.lock();
        let Some(menu) = state.active_menu.clone() else {
            return;
        };

        // 1. If an item without a submenu was hovered (menu_idx == 0): close deeper submenus only
        if menu_idx == 0 {
            Self::close_levels_above(window, &mut state, depth);
            return;
        }

        // 2. If the submenu for menu_idx is ALREADY open at depth + 1: return immediately without touching UI models
        if state.open_levels.len() > depth + 1 && state.open_levels[depth + 1].invoking_menu_idx == menu_idx {
            return;
        }

        // 3. Resolve target children
        let Some(children_slice) = menu.find_children_by_menu_index(menu_idx) else {
            Self::close_levels_above(window, &mut state, depth);
            return;
        };

        if children_slice.is_empty() {
            Self::close_levels_above(window, &mut state, depth);
            return;
        }

        let children = children_slice.to_vec();

        let parent_level = match state.open_levels.get(depth) {
            Some(l) => l.clone(),
            None => return,
        };

        let scale = window.window().scale_factor();
        let win_size = window.window().size();
        let win_w = (win_size.width.az::<f32>() / scale).max(200.0);
        let win_h = (win_size.height.az::<f32>() / scale).max(200.0);

        let max_h = (win_h * 0.70).max(180.0).min(win_h - (MENU_WINDOW_MARGIN * 2.0));
        let content_h = Self::calculate_items_content_height(&children);
        let effective_h = content_h.min(max_h);

        // Alignment: Align top of submenu with the Y-level of the invoking parent row
        let ideal_y = parent_level.y + visible_row_y - 2.0;

        let new_y = if ideal_y + effective_h <= win_h - MENU_WINDOW_MARGIN {
            ideal_y.max(MENU_WINDOW_MARGIN)
        } else {
            // Insufficient space downwards: shift up so bottom stays within margin
            (win_h - effective_h - MENU_WINDOW_MARGIN).max(MENU_WINDOW_MARGIN)
        };

        // Horizontal placement: Open to the right of parent menu, or flip left if overflowing
        let ideal_x = parent_level.x + parent_level.width - 2.0;
        let new_x = if ideal_x + MENU_WIDTH <= win_w - MENU_WINDOW_MARGIN {
            ideal_x
        } else {
            (parent_level.x - MENU_WIDTH + 2.0).max(MENU_WINDOW_MARGIN)
        };

        // Close only the stale deeper submenu levels (tail rows) and un-highlight their parent row.
        // Levels 0..=depth (including the scroll position of the parent) are left untouched.
        Self::close_levels_above(window, &mut state, depth);

        let new_depth = depth + 1;
        state.open_levels.push(OpenLevelInfo {
            invoking_menu_idx: menu_idx,
            x: new_x,
            y: new_y,
            width: MENU_WIDTH,
        });

        let levels = window.get_context_menu_levels();

        // Highlight the invoking parent row by patching exactly that single item row in place.
        Self::set_submenu_highlight(&levels, depth, menu_idx, true);

        // Append exactly one new level row; existing level rows are never replaced.
        let new_level = ContextMenuLevelModel {
            depth: new_depth.saturating_as::<i32>(),
            active_menu_index: 0,
            x: new_x,
            y: new_y,
            width: MENU_WIDTH,
            max_height: max_h,
            height: effective_h,
            items: CtxMenuDispatcher::items_to_slint_model(&children, new_depth.saturating_as::<i32>()),
        };

        if let Some(vec_model) = levels.as_any().downcast_ref::<VecModel<ContextMenuLevelModel>>() {
            vec_model.push(new_level);
        } else {
            tracing::warn!("Context menu levels model is not a VecModel; submenu level not appended");
        }
    }

    /// Closes all submenu levels deeper than `depth` by removing only the tail level rows from the
    /// live Slint model and clearing the highlight of the parent row that invoked the closed submenu.
    /// Levels 0..=depth keep their instances (and therefore scroll positions and hover states).
    fn close_levels_above(window: &MainWindow, state: &mut PresenterState, depth: usize) {
        if state.open_levels.len() <= depth + 1 {
            return;
        }

        let previous_active_idx = state.open_levels[depth + 1].invoking_menu_idx;
        state.open_levels.truncate(depth + 1);

        let levels = window.get_context_menu_levels();
        if let Some(vec_model) = levels.as_any().downcast_ref::<VecModel<ContextMenuLevelModel>>() {
            while vec_model.row_count() > depth + 1 {
                vec_model.remove(vec_model.row_count() - 1);
            }
        } else {
            tracing::warn!("Context menu levels model is not a VecModel; deeper levels not removed");
        }

        Self::set_submenu_highlight(&levels, depth, previous_active_idx, false);
    }

    /// Updates the highlight flag of exactly one item row (the submenu parent row with `menu_idx`)
    /// inside the existing items model of the level at `depth`. No rows are added, removed or recreated.
    fn set_submenu_highlight(levels: &ModelRc<ContextMenuLevelModel>, depth: usize, menu_idx: usize, active: bool) {
        if menu_idx == 0 {
            return;
        }
        let Some(level) = levels.row_data(depth) else {
            return;
        };
        let items = level.items;
        for row in 0..items.row_count() {
            if let Some(mut item) = items.row_data(row)
                && item.has_submenu
                && item.menu_index == menu_idx.saturating_as::<i32>()
            {
                if item.is_checked != active {
                    item.is_checked = active;
                    items.set_row_data(row, item);
                }
                break;
            }
        }
    }

    /// Closes the active context menu popup and clears all levels.
    pub fn dismiss(window: &MainWindow) {
        window.set_is_context_menu_open(false);
        window.set_context_menu_levels(ModelRc::new(VecModel::from(vec![])));
        let mut state = ACTIVE_PRESENTER_STATE.lock();
        state.open_levels.clear();
        state.active_menu = None;
    }

    /// Calculates preferred content height for a list of context menu items.
    pub fn calculate_items_content_height(items: &[CtxMenuItem]) -> f32 {
        let mut height = 8.0; // 4px padding-top + 4px padding-bottom
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                height += 1.0; // 1px spacing
            }
            height += match item.kind {
                CtxMenuItemKind::Separator => 7.0,
                CtxMenuItemKind::Header => 22.0,
                _ => 28.0,
            };
        }
        height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx_menu::CtxMenuBuilder;

    #[test]
    fn the_content_height_adds_rows_separators_header_spacing_and_padding() {
        let menu = CtxMenuBuilder::new()
            .header_with_tooltip("Header".to_string(), "tip".to_string())
            .action(1, "One", None, None, true)
            .separator()
            .action(2, "Two", None, None, true)
            .build();
        // 8 padding + 22 header + 28 + 7 + 28 + 3 gaps
        assert!((CtxMenuPresenter::calculate_items_content_height(&menu.items) - 96.0).abs() < f32::EPSILON);
        assert!((CtxMenuPresenter::calculate_items_content_height(&[]) - 8.0).abs() < f32::EPSILON);
    }
}
