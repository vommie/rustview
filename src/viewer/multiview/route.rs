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

#[cfg(test)]
mod tests {
    use super::*;
    use HostMode::*;
    use OriginKind::{Browse, Tab};
    use RoutedEvent::*;

    const MODES: [HostMode; 4] = [Hidden, Fullscreen, TabView, TabGrid];
    const EVENTS: [RoutedEvent; 8] = [
        ToggleFullscreen,
        CloseTab,
        Rename,
        CtrlClick,
        GridLeft,
        GridMiddle,
        GridWheel { up: true },
        GridWheel { up: false },
    ];
    const CLICKS: [ClickKind; 3] = [ClickKind::None, ClickKind::Double, ClickKind::CtrlDouble];

    fn r(mode: HostMode, origin: OriginKind, ev: RoutedEvent) -> UiAction {
        route(mode, origin, ev, ClickKind::None)
    }

    #[test]
    fn a_hidden_multiview_ignores_everything() {
        for origin in [Browse, Tab] {
            for ev in EVENTS {
                for click in CLICKS {
                    assert_eq!(route(Hidden, origin, ev, click), UiAction::None, "{ev:?}");
                }
            }
        }
    }

    #[test]
    fn fullscreen_closes_on_toggle_and_close_requests() {
        for origin in [Browse, Tab] {
            assert_eq!(
                r(Fullscreen, origin, ToggleFullscreen),
                UiAction::CloseFullscreen
            );
            assert_eq!(r(Fullscreen, origin, CloseTab), UiAction::CloseFullscreen);
        }
    }

    #[test]
    fn only_a_ctrl_double_click_converts_and_only_from_browse() {
        assert_eq!(
            route(Fullscreen, Browse, CtrlClick, ClickKind::CtrlDouble),
            UiAction::ConvertToTab
        );
        assert_eq!(
            route(Fullscreen, Browse, CtrlClick, ClickKind::None),
            UiAction::None
        );
        assert_eq!(
            route(Fullscreen, Browse, CtrlClick, ClickKind::Double),
            UiAction::None
        );
        assert_eq!(
            route(Fullscreen, Tab, CtrlClick, ClickKind::CtrlDouble),
            UiAction::CloseFullscreen
        );
        assert_eq!(
            route(Fullscreen, Tab, CtrlClick, ClickKind::None),
            UiAction::None
        );
    }

    #[test]
    fn the_tab_view_opens_the_tab_fullscreen_or_closes_it() {
        assert_eq!(
            r(TabView, Tab, ToggleFullscreen),
            UiAction::OpenTabFullscreen
        );
        assert_eq!(r(TabView, Tab, CloseTab), UiAction::CloseActiveTab);
        assert_eq!(
            route(TabView, Tab, CtrlClick, ClickKind::CtrlDouble),
            UiAction::None
        );
    }

    #[test]
    fn the_grid_activates_opens_and_scrolls() {
        assert_eq!(r(TabGrid, Tab, GridLeft), UiAction::ActivateTab);
        assert_eq!(r(TabGrid, Tab, GridMiddle), UiAction::OpenTabFullscreen);
        assert_eq!(
            r(TabGrid, Tab, GridWheel { up: true }),
            UiAction::ScrollGrid { up: true }
        );
        assert_eq!(
            r(TabGrid, Tab, GridWheel { up: false }),
            UiAction::ScrollGrid { up: false }
        );
        assert_eq!(r(TabGrid, Tab, ToggleFullscreen), UiAction::None);
    }

    #[test]
    fn rename_is_routed_in_every_visible_host() {
        for mode in MODES.into_iter().filter(|m| *m != Hidden) {
            assert_eq!(r(mode, Tab, Rename), UiAction::RequestRename, "{mode:?}");
        }
    }

    #[test]
    fn grid_events_never_act_outside_the_grid() {
        for mode in [Fullscreen, TabView] {
            for ev in [GridLeft, GridMiddle, GridWheel { up: true }] {
                assert_eq!(r(mode, Browse, ev), UiAction::None, "{mode:?} {ev:?}");
            }
        }
    }
}
