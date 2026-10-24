//! The keys of the main window: Browse, the tab view (a Multiview in a tab) and the tab grid.
//!
//! Every action runs through the same callback or menu action as its button, menu entry or mouse gesture, so a key never has
//! a path of its own: the rating keys through `rate-key` (`ui::rating_hotkey`), the Multiview keys of the tab view through the
//! tab view's callbacks (`MultiviewCommand`s on the selected medium), the new Browse keys through the media menu's actions.

use super::{Resolved, Trigger, resolve};
use crate::ui::MainWindow;
use crate::ui::keymap::{Action, Area, KeyPress};
use crate::ui::state::{CTX_ACTION_OPEN_IN_BG_TAB, CTX_ACTION_OPEN_IN_FULLSCREEN, CTX_ACTION_REFRESH_FILES};
use rustview_core::util::cast::WrappingAs;
use slint::ComponentHandle;
use slint::platform::Key;

/// The view the main window shows (`MainWindow.active-view-mode`).
const BROWSE: i32 = 0;
const TAB_VIEW: i32 = 1;

/// Connects the key callback of the main window.
pub fn install(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_hotkey(move |text, control, alt, shift, meta, repeat, typing| {
        let Some(window) = weak.upgrade() else {
            return false;
        };
        let press = KeyPress {
            text: text.as_str(),
            control,
            alt,
            shift,
            meta,
            repeat,
        };
        dispatch(&window, &Trigger::Press(press), typing)
    });
}

/// Runs the action `trigger` stands for in the view the main window shows; `false` when it has none (the key goes on).
pub fn dispatch(window: &MainWindow, trigger: &Trigger<'_>, typing: bool) -> bool {
    let mode = window.get_active_view_mode();
    let areas: &[Area] = match mode {
        TAB_VIEW => &[Area::General, Area::Tab],
        _ => &[Area::General, Area::Browse],
    };
    let action = match resolve(trigger, areas, typing) {
        Resolved::Pass => return false,
        Resolved::Swallow => return true,
        Resolved::Run(action) => action,
    };
    // The tab grid shares Browse's Rename (it acts on the last used panel); the other Browse keys need the Browse view.
    if mode != BROWSE && mode != TAB_VIEW && action.area() == Area::Browse && action != Action::BrowseRename {
        return false;
    }
    tracing::debug!(action = action.id(), mode, "[Hotkeys] Main window");
    run(window, action, trigger)
}

fn run(window: &MainWindow, action: Action, trigger: &Trigger<'_>) -> bool {
    if let Some(rating) = action.rating() {
        window.invoke_rate_key(i32::from(rating));
        return true;
    }
    match action {
        Action::NextTab => window.invoke_tab_wheel_scrolled(true),
        Action::PreviousTab => window.invoke_tab_wheel_scrolled(false),
        Action::ShowBrowse => window.invoke_browse_clicked(),
        Action::ShowTabGrid => window.invoke_tab_grid_clicked(),
        Action::OpenSettings => window.invoke_open_settings(),
        Action::ShowHotkeys => window.invoke_open_hotkey_settings(),
        Action::BrowseOpen => window.invoke_browse_enter_pressed(),
        Action::BrowseOpenFullscreen => menu_action(window, CTX_ACTION_OPEN_IN_FULLSCREEN),
        Action::BrowseOpenBackground => menu_action(window, CTX_ACTION_OPEN_IN_BG_TAB),
        Action::BrowseRefresh => menu_action(window, CTX_ACTION_REFRESH_FILES),
        Action::BrowseRename | Action::TabRename => window.invoke_request_rename(),
        Action::BrowseDelete => window.invoke_request_delete(),
        Action::BrowseSelectAll => window.invoke_select_all_media(),
        Action::BrowseClearSelection => window.invoke_clear_media_selection(),
        Action::BrowseParent => window.invoke_browse_go_up_requested(),
        Action::BrowseBack => window.invoke_browse_history_requested(-1),
        Action::BrowseForward => window.invoke_browse_history_requested(1),
        Action::BrowseNewFolder => {
            if window.get_is_new_folder_btn_enabled() {
                window.invoke_new_folder_clicked();
            }
        }
        Action::BrowseFilter => {
            if window.get_active_view_mode() != BROWSE {
                return false;
            }
            window.invoke_focus_search();
        }
        Action::BrowseToggleTable => window.invoke_toggle_table_view(),
        Action::BrowseCursor | Action::BrowsePage | Action::BrowseFirstLast => {
            let Some(direction) = cursor_direction(&trigger.key_text()) else {
                return false;
            };
            let page_rows = window.invoke_media_page_rows();
            window.invoke_media_navigate(direction, trigger.shift(), page_rows);
        }
        Action::TabDelete => window.invoke_viewer_delete_key(),
        Action::TabRefresh => {
            if let Some(media_id) = crate::ui::multiview::tab_host::key_target_media() {
                window.invoke_refresh_medium(media_id.wrapping_as::<i32>());
            }
        }
        Action::TabZoomReset => window.invoke_viewer_zoom_key(0),
        Action::TabZoomIn => window.invoke_viewer_zoom_key(1),
        Action::TabZoomOut => window.invoke_viewer_zoom_key(2),
        Action::TabApplyCrop => return window.invoke_edit_key(1),
        Action::TabSave => return window.invoke_edit_key(2),
        Action::TabSaveAs => return window.invoke_edit_key(3),
        Action::TabUndo => return window.invoke_edit_key(4),
        Action::TabRedo => return window.invoke_edit_key(5),
        Action::TabFullscreen => window.invoke_tab_grid_item_middle_clicked(window.get_viewer_tab_id()),
        Action::TabClose => window.invoke_viewer_close_tab(),
        // Esc and type-ahead find are handled by the window itself; the fullscreen and the video player have their own keys.
        _ => return false,
    }
    true
}

/// A key of the media menu, on the current Browse selection: the same action as the menu entry.
fn menu_action(window: &MainWindow, action_id: u64) {
    window.invoke_context_menu_item_clicked(action_id.wrapping_as::<i32>());
}

/// The direction code of `media-navigate` for a cursor key.
fn cursor_direction(key: &str) -> Option<i32> {
    let directions = [
        (Key::UpArrow, 0),
        (Key::DownArrow, 1),
        (Key::LeftArrow, 2),
        (Key::RightArrow, 3),
        (Key::PageUp, 4),
        (Key::PageDown, 5),
        (Key::Home, 6),
        (Key::End, 7),
    ];
    directions
        .iter()
        .find(|(k, _)| slint::SharedString::from(*k).as_str() == key)
        .map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_keys_map_to_the_directions_of_media_navigate() {
        let code = |key: Key| cursor_direction(slint::SharedString::from(key).as_str());
        assert_eq!(code(Key::UpArrow), Some(0));
        assert_eq!(code(Key::RightArrow), Some(3));
        assert_eq!(code(Key::End), Some(7));
        assert_eq!(code(Key::F2), None);
    }
}
