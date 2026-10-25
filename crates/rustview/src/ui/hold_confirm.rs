//! Rust side of `ui/hold.slint` (hold to confirm).
//!
//! Slint globals belong to one window, so every window with elements that have to be held gets the duration of
//! `general.hold_to_confirm_ms` through [`install`]: at startup, when the setting changes, and when the window is made.
//! The settings window also follows the value while it is being edited (`changed hold-to-confirm-ms` in Slint).

use crate::ui::HoldConfirm;
use slint::{ComponentHandle, Global};
use std::sync::atomic::{AtomicU32, Ordering};

/// The duration last installed (windows made later, like the control bars of videos, take it from here).
static CURRENT_MS: AtomicU32 = AtomicU32::new(rustview_core::settings::schema::DEFAULT_HOLD_TO_CONFIRM_MS);

/// Sets how long the held elements of `component` must be held (0 = a click is enough).
pub fn install<C: ComponentHandle>(component: &C, hold_ms: u32)
where
    for<'a> HoldConfirm<'a>: Global<'a, C>,
{
    component.global::<HoldConfirm>().set_duration(i64::from(hold_ms));
    CURRENT_MS.store(hold_ms, Ordering::Relaxed);
}

/// The duration of `general.hold_to_confirm_ms` as it was installed last.
pub fn current_ms() -> u32 {
    CURRENT_MS.load(Ordering::Relaxed)
}
