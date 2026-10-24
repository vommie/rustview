//! Decides what an event from a Multiview player or tile means in the current host (pure).
//!
//! mpv players report clicks, key presses and window-state flips from their observer threads; what those mean depends on
//! where the Multiview is shown right now. Keeping the table here makes the behavior testable without any UI.

use super::click::ClickKind;

/// Where a Multiview is currently shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMode {
    Hidden,
    Fullscreen,
    TabView,
    TabGrid,
}

/// What a fullscreen Multiview was opened from: the Browse selection (it follows it and may become a tab) or a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginKind {
    Browse,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedEvent {
    ToggleFullscreen,
    Rename,
    CtrlClick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    None,
    CloseFullscreen,
    ConvertToTab,
    OpenTabFullscreen,
    CloseActiveTab,
    RequestRename,
    /// Action button: open the medium as a normal background tab.
    OpenTileAsTab(u64),
    /// Action button: the medium left the Multiview, so it is deselected in the Browse view too.
    DeselectInBrowse(u64),
    /// Action button: a medium left the Multiview of a tab, so everything that follows the tab's media (tab bar, inspector,
    /// tag and rating targets) has to catch up.
    TabMembersChanged,
    /// Action button (or DEL on a single medium): the delete dialog opens for the medium.
    DeleteTile(u64),
    /// Action button "Open With": the drop-down with the applications of the medium opens, or closes if it is open.
    ToggleOpenWith(u64),
    /// An item of the "Open With" drop-down was chosen (the id the item has in the menu).
    OpenWithItem(u64, i32),
    /// Primary bar button: the mouse cursor hiding after it rested is switched on or off (a setting of the application).
    SetIdleCursorHide(bool),
    /// Previous/next medium of the single image the Multiview shows (keys and mouse wheel).
    Navigate {
        forward: bool,
    },
    /// Save button (or Ctrl+S): the edits of the tile are saved over the original (or through the export dialog).
    SaveTile(u64),
    /// "Save As..." of the Save drop-down (or Ctrl+Shift+S): the export dialog opens for the tile.
    SaveTileAs(u64),
    /// The arrow of the Save button: its drop-down opens or closes.
    ToggleSaveMenu(u64),
    /// The remove button of a tile with unsaved edits: the application asks first, then removes the tile.
    ConfirmRemoveTile(u64),
}

pub fn route(mode: HostMode, origin: OriginKind, event: RoutedEvent, click: ClickKind) -> UiAction {
    use RoutedEvent::{CtrlClick, Rename, ToggleFullscreen};
    match (mode, event) {
        (HostMode::Hidden, _) => UiAction::None,
        (_, Rename) => UiAction::RequestRename,
        (HostMode::Fullscreen, ToggleFullscreen) => UiAction::CloseFullscreen,
        (HostMode::Fullscreen, CtrlClick) => match (click, origin) {
            (ClickKind::CtrlDouble, OriginKind::Browse) => UiAction::ConvertToTab,
            (ClickKind::CtrlDouble, OriginKind::Tab) => UiAction::CloseFullscreen,
            _ => UiAction::None,
        },
        (HostMode::TabView, ToggleFullscreen) => UiAction::OpenTabFullscreen,
        _ => UiAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_ctrl_double_click_in_the_fullscreen_means_something() {
        assert_eq!(
            route(
                HostMode::TabGrid,
                OriginKind::Tab,
                RoutedEvent::CtrlClick,
                ClickKind::CtrlDouble
            ),
            UiAction::None
        );
        assert_eq!(
            route(
                HostMode::TabView,
                OriginKind::Tab,
                RoutedEvent::CtrlClick,
                ClickKind::None
            ),
            UiAction::None
        );
        assert_eq!(
            route(
                HostMode::Hidden,
                OriginKind::Tab,
                RoutedEvent::CtrlClick,
                ClickKind::None
            ),
            UiAction::None
        );
        assert_eq!(
            route(
                HostMode::Fullscreen,
                OriginKind::Browse,
                RoutedEvent::CtrlClick,
                ClickKind::CtrlDouble
            ),
            UiAction::ConvertToTab
        );
    }
}
