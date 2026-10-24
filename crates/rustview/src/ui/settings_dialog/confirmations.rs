//! Settings > General > Confirmations: the hold duration of the elements that act only when held (`ui/hold.slint`).

use crate::ui::SettingsWindow;
use rustview_core::settings::schema::{GeneralSettings, MAX_HOLD_TO_CONFIRM_MS};
use rustview_core::util::cast::SaturatingAs;

pub(super) fn populate(window: &SettingsWindow, general: &GeneralSettings) {
    window.set_hold_to_confirm_ms(general.hold_to_confirm_ms.saturating_as::<i32>());
    crate::ui::hold_confirm::install(window, general.hold_to_confirm_ms);
}

pub(super) fn save(window: &SettingsWindow, general: &mut GeneralSettings) {
    general.hold_to_confirm_ms = window
        .get_hold_to_confirm_ms()
        .clamp(0, MAX_HOLD_TO_CONFIRM_MS.saturating_as::<i32>())
        .saturating_as::<u32>();
}
