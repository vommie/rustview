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
    CloseTab,
    Rename,
    CtrlClick,
    GridLeft,
    GridMiddle,
    GridWheel { up: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    None,
    CloseFullscreen,
    ConvertToTab,
    OpenTabFullscreen,
    CloseActiveTab,
    RequestRename,
    ActivateTab,
    ScrollGrid { up: bool },
    /// Action button: open the medium as a normal background tab.
    OpenTileAsTab(u64),
    /// Action button: the medium left the Multiview, so it is deselected in the Browse view too.
    DeselectInBrowse(u64),
    /// Action button: a medium left the Multiview of a tab, so everything that follows the tab's media (tab bar, inspector,
    /// tag and rating targets) has to catch up.
    TabMemberRemoved,
    /// Action button (or DEL on a single medium): the delete dialog opens for the medium.
    DeleteTile(u64),
    /// Action button "Open With": the drop-down with the applications of the medium opens, or closes if it is open.
    ToggleOpenWith(u64),
    /// An item of the "Open With" drop-down was chosen (the id the item has in the menu).
    OpenWithItem(u64, i32),
    /// Primary bar button: the mouse cursor hiding after it rested is switched on or off (a setting of the application).
    SetIdleCursorHide(bool),
    /// Previous/next medium of the single image the Multiview shows (keys and mouse wheel).
    Navigate { forward: bool },
}

pub fn route(mode: HostMode, origin: OriginKind, event: RoutedEvent, click: ClickKind) -> UiAction {
    use RoutedEvent::*;
    match (mode, event) {
        (HostMode::Hidden, _) => UiAction::None,
        (_, Rename) => UiAction::RequestRename,
        (HostMode::Fullscreen, ToggleFullscreen | CloseTab) => UiAction::CloseFullscreen,
        (HostMode::Fullscreen, CtrlClick) => match (click, origin) {
            (ClickKind::CtrlDouble, OriginKind::Browse) => UiAction::ConvertToTab,
            (ClickKind::CtrlDouble, OriginKind::Tab) => UiAction::CloseFullscreen,
            _ => UiAction::None,
        },
        (HostMode::TabView, ToggleFullscreen) => UiAction::OpenTabFullscreen,
        (HostMode::TabView, CloseTab) => UiAction::CloseActiveTab,
        (HostMode::TabGrid, GridLeft) => UiAction::ActivateTab,
        (HostMode::TabGrid, GridMiddle) => UiAction::OpenTabFullscreen,
        (HostMode::TabGrid, GridWheel { up }) => UiAction::ScrollGrid { up },
        _ => UiAction::None,
    }
}
