use crate::ctx_menu::{CtxMenu, CtxMenuDispatcher};
use crate::ui::{ContextMenuItemModel, MainWindow};
use slint::{ModelRc, VecModel};

/// Dedicated presenter responsible for rendering and managing multi-level context menu popups in Slint.
pub struct CtxMenuPresenter;

impl CtxMenuPresenter {
    /// Opens a context menu at the specified screen coordinates and populates Level 0.
    pub fn open(
        window: &MainWindow,
        active_menu_slot: &mut Option<CtxMenu>,
        mut menu: CtxMenu,
        x: f32,
        y: f32,
    ) {
        menu.index_items();
        let level0 = CtxMenuDispatcher::items_to_slint_model(&menu.items, 0);

        *active_menu_slot = Some(menu);

        window.set_level0_context_items(level0);
        Self::clear_submenus_from_level(window, 1);

        window.set_context_menu_x(x);
        window.set_context_menu_y(y);
        window.set_is_context_menu_open(true);
    }

    /// Handles flyout requests for submenus up to depth level 5.
    pub fn handle_submenu_request(
        window: &MainWindow,
        active_menu: Option<&CtxMenu>,
        depth: i32,
        menu_idx: usize,
    ) {
        let Some(menu) = active_menu else {
            return;
        };

        if menu_idx == 0 {
            Self::clear_submenus_from_level(window, depth + 1);
            return;
        }

        if let Some(children) = menu.find_children_by_menu_index(menu_idx) {
            let next_depth = depth + 1;
            let model = CtxMenuDispatcher::items_to_slint_model(children, next_depth);
            Self::set_level_model(window, next_depth, model);
            Self::clear_submenus_from_level(window, next_depth + 1);
        }
    }

    /// Closes the active context menu popup.
    pub fn dismiss(window: &MainWindow) {
        window.set_is_context_menu_open(false);
    }

    fn set_level_model(window: &MainWindow, depth: i32, model: ModelRc<ContextMenuItemModel>) {
        match depth {
            0 => window.set_level0_context_items(model),
            1 => window.set_level1_context_items(model),
            2 => window.set_level2_context_items(model),
            3 => window.set_level3_context_items(model),
            4 => window.set_level4_context_items(model),
            5 => window.set_level5_context_items(model),
            _ => {}
        }
    }

    fn clear_submenus_from_level(window: &MainWindow, start_depth: i32) {
        let empty = || ModelRc::new(VecModel::from(vec![]));
        for d in start_depth..=5 {
            match d {
                1 => window.set_level1_context_items(empty()),
                2 => window.set_level2_context_items(empty()),
                3 => window.set_level3_context_items(empty()),
                4 => window.set_level4_context_items(empty()),
                5 => window.set_level5_context_items(empty()),
                _ => {}
            }
        }
    }
}
