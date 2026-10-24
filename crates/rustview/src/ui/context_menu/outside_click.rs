//! A click outside an open context menu closes the menu and then goes on to what is under the pointer (as in KDE
//! applications and Slint's own menus): a click into a text field puts the cursor there, a click on a button presses it.
//!
//! The menu's backdrop takes the click (press and release) and reports it on release; the bridge closes the menu and
//! replays the click on the next turn of the event loop, when the backdrop is gone.

use crate::ui::MainWindow;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition};
use std::time::Duration;

/// Replays the click at the window position (`x`, `y`, logical pixels) once the closed menu has left the window.
pub fn replay(window: &MainWindow, x: f32, y: f32, button: PointerEventButton) {
    tracing::debug!(
        ?button,
        "[ContextMenu] A click outside the menu closed it; it goes on to what is under the pointer"
    );
    let weak = window.as_weak();
    // Inside the backdrop's pointer handler the window is still dispatching the real click: queue the replay behind it.
    slint::Timer::single_shot(Duration::ZERO, move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        for event in replay_events(LogicalPosition::new(x, y), button) {
            window.window().dispatch_event(event);
        }
    });
}

/// The events of the replayed click. The move comes first: it changes the item under the pointer from the backdrop to the
/// target, which resets Slint's click counter, so the replay never counts as the second click of a double click.
fn replay_events(position: LogicalPosition, button: PointerEventButton) -> [WindowEvent; 3] {
    [
        WindowEvent::PointerMoved { position },
        WindowEvent::PointerPressed { position, button },
        WindowEvent::PointerReleased { position, button },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replay_is_a_single_click_at_the_press_position() {
        let at = LogicalPosition::new(1280.0, 159.0);
        let events = replay_events(at, PointerEventButton::Left);
        assert!(
            matches!(events[0], WindowEvent::PointerMoved { position } if position == at),
            "a move first, so the replay is never taken for a double click"
        );
        assert!(matches!(
            events[1],
            WindowEvent::PointerPressed { position, button: PointerEventButton::Left } if position == at
        ));
        assert!(matches!(
            events[2],
            WindowEvent::PointerReleased { position, button: PointerEventButton::Left } if position == at
        ));
    }

    #[test]
    fn a_right_click_outside_stays_a_right_click() {
        let events = replay_events(LogicalPosition::new(10.0, 20.0), PointerEventButton::Right);
        assert!(matches!(
            events[1],
            WindowEvent::PointerPressed {
                button: PointerEventButton::Right,
                ..
            }
        ));
        assert!(matches!(
            events[2],
            WindowEvent::PointerReleased {
                button: PointerEventButton::Right,
                ..
            }
        ));
    }
}
