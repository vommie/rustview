//! The probe's own pointer, wheel and key events: they enter the main window the way real ones do after winit, so every
//! touch area, drag and resize runs its normal path, whatever the real pointer does meanwhile.

use super::recorder;
use rustview_core::util::cast::Az;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{LogicalPosition, SharedString};

/// Logical pixels one notch of a mouse wheel scrolls.
const WHEEL_NOTCH_PX: f32 = 60.0;

/// How far a sweep is from its start (0 to 1): `cycles` times there and back over `progress` 0 to 1.
pub fn triangle(progress: f32, cycles: u32) -> f32 {
    if !(0.0..1.0).contains(&progress) {
        return 0.0;
    }
    let phase = (progress * cycles.max(1).az::<f32>()).fract();
    1.0 - (2.0 * phase - 1.0).abs()
}

fn position(point: (f32, f32)) -> LogicalPosition {
    LogicalPosition::new(point.0, point.1)
}

pub fn move_to(window: &slint::Window, point: (f32, f32)) {
    recorder::note_input();
    window.dispatch_event(WindowEvent::PointerMoved {
        position: position(point),
    });
}

pub fn press(window: &slint::Window, point: (f32, f32)) {
    move_to(window, point);
    window.dispatch_event(WindowEvent::PointerPressed {
        position: position(point),
        button: PointerEventButton::Left,
    });
}

pub fn release(window: &slint::Window, point: (f32, f32)) {
    recorder::note_input();
    window.dispatch_event(WindowEvent::PointerReleased {
        position: position(point),
        button: PointerEventButton::Left,
    });
}

/// Turns the wheel by one notch at `point`; `down` moves toward the end of a list.
pub fn wheel(window: &slint::Window, point: (f32, f32), down: bool) {
    recorder::note_input();
    let delta_y = if down { -WHEEL_NOTCH_PX } else { WHEEL_NOTCH_PX };
    window.dispatch_event(WindowEvent::PointerScrolled {
        position: position(point),
        delta_x: 0.0,
        delta_y,
    });
}

/// Presses and releases the key with that name; `false` for a name the probe does not know.
pub fn key(window: &slint::Window, name: &str) -> bool {
    let text: SharedString = match name {
        "escape" => Key::Escape.into(),
        "return" => Key::Return.into(),
        "delete" => Key::Delete.into(),
        "up" => Key::UpArrow.into(),
        "down" => Key::DownArrow.into(),
        "pagedown" => Key::PageDown.into(),
        "pageup" => Key::PageUp.into(),
        _ => return false,
    };
    recorder::note_input();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
    true
}

#[cfg(test)]
mod tests {
    use super::triangle;

    #[test]
    fn a_sweep_goes_there_and_back_once_per_cycle() {
        assert!(triangle(0.0, 1).abs() < 1e-6);
        assert!((triangle(0.25, 1) - 0.5).abs() < 1e-6);
        assert!((triangle(0.5, 1) - 1.0).abs() < 1e-6);
        assert!((triangle(0.75, 1) - 0.5).abs() < 1e-6);
        assert!((triangle(0.25, 2) - 1.0).abs() < 1e-6);
        assert!(triangle(0.5, 2).abs() < 1e-6);
        assert!(triangle(1.0, 3).abs() < 1e-6, "the pointer ends where it started");
        assert!(triangle(f32::NAN, 1).abs() < 1e-6);
    }
}
