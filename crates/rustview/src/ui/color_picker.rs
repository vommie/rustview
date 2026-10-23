//! Rust side of `ui/color_picker.slint`.
//!
//! Slint cannot take a string apart, so the hex field of the color picker asks this module to parse and format its text.
//! Call [`install`] once for every window that contains a `ColorPicker` or a `ColorSwatchButton`. [`open`] shows the
//! picker in a window of its own.

use rustview_core::util::hex_color;
use crate::ui::{ColorCodec, ColorPickerWindow};
use slint::{Color, ComponentHandle, Global, SharedString};
use std::cell::RefCell;
use std::rc::Rc;

pub fn install<C: ComponentHandle>(component: &C)
where
    for<'a> ColorCodec<'a>: Global<'a, C>,
{
    let codec = component.global::<ColorCodec>();
    codec.on_to_hex(|color, with_alpha| to_hex(color, with_alpha).into());
    codec.on_is_valid(|text, with_alpha| parse(&text, with_alpha).is_some());
    codec.on_from_hex(|text| parse(&text, true).unwrap_or(Color::from_argb_u8(255, 0, 0, 0)));
}

fn to_hex(color: Color, with_alpha: bool) -> String {
    let (r, g, b) = (color.red(), color.green(), color.blue());
    if with_alpha {
        hex_color::format_rgba([r, g, b, color.alpha()])
    } else {
        hex_color::format_rgb([r, g, b])
    }
}

/// `None` unless `text` is a color in the accepted forms (`#rgb`, `#rrggbb`, and with alpha `#rgba`, `#rrggbbaa`).
fn parse(text: &SharedString, with_alpha: bool) -> Option<Color> {
    let [r, g, b, a] = hex_color::parse_rgba(text)?;
    Some(Color::from_argb_u8(if with_alpha { a } else { 255 }, r, g, b))
}

thread_local! {
    static WINDOW: RefCell<Option<ColorPickerWindow>> = const { RefCell::new(None) };
}

/// What the picker window starts with.
pub struct PickerRequest {
    pub title: &'static str,
    pub initial: Color,
    /// Offers a "Default" button that restores this color.
    pub default: Option<Color>,
    pub show_alpha: bool,
}

/// Opens the color picker in a window of its own, centered over `parent`. Every change the user makes goes to `on_change`
/// at once; Cancel, Esc or closing the window reports the initial color again, OK keeps the last one. A picker that is
/// already open is replaced. (Window managers that ignore requested positions, as Wayland compositors do, place it
/// themselves.) UI thread only.
pub fn open<P: ComponentHandle>(parent: &P, request: PickerRequest, on_change: impl Fn(Color) + 'static) {
    let on_change: Rc<dyn Fn(Color)> = Rc::new(on_change);
    let created = WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            match ColorPickerWindow::new() {
                Ok(window) => {
                    install(&window);
                    *slot = Some(window);
                }
                Err(e) => tracing::error!(error = %e, "[ColorPicker] Failed to create the picker window"),
            }
        }
        slot.as_ref().map(ComponentHandle::as_weak)
    });
    let Some(window) = created.and_then(|weak| weak.upgrade()) else {
        return;
    };

    window.set_dialog_title(request.title.into());
    window.set_show_alpha(request.show_alpha);
    window.set_default_color(request.default.unwrap_or(Color::from_argb_u8(0, 0, 0, 0)));
    window.set_original(request.initial);
    window.set_value(request.initial);

    let initial = request.initial;
    let changed = on_change.clone();
    window.on_edited(move |color| changed(color));
    let (weak, cancelled) = (window.as_weak(), on_change);
    window.on_cancelled(move || {
        cancelled(initial);
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
    });
    let weak = window.as_weak();
    window.on_accepted(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
    });
    let weak = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(window) = weak.upgrade() {
            window.invoke_cancelled();
        }
        slint::CloseRequestResponse::HideWindow
    });

    if let Err(e) = window.show() {
        tracing::error!(error = %e, "[ColorPicker] Failed to show the picker window");
        return;
    }
    center_over(&window, parent);
    tracing::debug!("[ColorPicker] Picker window opened");
}

/// Hides the picker window (for example when its parent closes). The color stays as the user last left it.
pub fn close() {
    WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().as_ref() {
            let _ = window.hide();
        }
    });
}

/// The application exits: the picker window is hidden and destroyed.
pub fn shutdown() {
    let window = WINDOW.with(|slot| slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()));
    if let Some(window) = window {
        let _ = window.hide();
        tracing::debug!("[Shutdown: Windows] Color picker window destroyed");
    }
}

/// Puts `window` in the middle of `parent` (physical pixels, as both report them).
fn center_over<P: ComponentHandle>(window: &ColorPickerWindow, parent: &P) {
    let (parent_pos, parent_size) = (parent.window().position(), parent.window().size());
    let size = window.window().size();
    let x = parent_pos.x + (parent_size.width as i32 - size.width as i32) / 2;
    let y = parent_pos.y + (parent_size.height as i32 - size.height as i32) / 2;
    window.window().set_position(slint::PhysicalPosition::new(x, y));
}
