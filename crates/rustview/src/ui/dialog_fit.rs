//! Dialog windows follow their content: the body of every dialog (`ui/dialog_body.slint`) reports through the `DialogFit`
//! global when its content grew while it showed all of it (a note or an error appeared), and the window becomes that much
//! taller, at most down to the bottom of its screen. A dialog the user made lower than its content keeps its height and
//! scrolls. Every dialog window calls `install` once, right after it is created.

use crate::ui::DialogFit;
use rustview_core::util::cast::FloatToInt;
use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit::dpi::PhysicalSize;

/// Lets `dialog` grow with its content.
pub fn install<C>(dialog: &C)
where
    C: ComponentHandle + 'static,
    for<'a> DialogFit<'a>: slint::Global<'a, C>,
{
    let weak = dialog.as_weak();
    dialog.global::<DialogFit>().on_grow(move |by| {
        if let Some(dialog) = weak.upgrade() {
            grow(dialog.window(), by);
        }
    });
}

/// Makes the open dialog `by` logical pixels taller (a hidden one gets its fitting height when it is shown).
fn grow(window: &slint::Window, by: f32) {
    if !window.is_visible() || by <= 0.0 {
        return;
    }
    // Through winit, so Slint does not take the size for one set by the program (it would keep it the next time the dialog
    // is shown instead of fitting the content again).
    window.with_winit_window(|w| {
        let size = w.inner_size();
        let extra = (f64::from(by) * w.scale_factor()).ceil().to_int::<u32>();
        let mut height = size.height.saturating_add(extra);
        if let (Some(monitor), Ok(top)) = (w.current_monitor(), w.inner_position()) {
            let bottom = i64::from(monitor.position().y) + i64::from(monitor.size().height);
            let room = u32::try_from((bottom - i64::from(top.y)).max(0)).unwrap_or(u32::MAX);
            height = height.min(room.max(size.height));
        }
        tracing::info!(
            from = size.height,
            to = height,
            "[DialogFit] The content of a dialog grew; the window follows"
        );
        let _ = w.request_inner_size(PhysicalSize::new(size.width, height));
    });
}
