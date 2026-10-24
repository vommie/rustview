//! Rust side of `ui/hold.slint` (hold to confirm).
//!
//! Slint globals belong to one window, so every window with elements that have to be held gets the duration of
//! `general.hold_to_confirm_ms` through [`install`]: at startup, when the setting changes, and when the window is made.
//! The settings window also follows the value while it is being edited (`changed hold-to-confirm-ms` in Slint).

use crate::ui::HoldConfirm;
use slint::{ComponentHandle, Global};

/// Sets how long the held elements of `component` must be held (0 = a click is enough).
pub fn install<C: ComponentHandle>(component: &C, hold_ms: u32)
where
    for<'a> HoldConfirm<'a>: Global<'a, C>,
{
    component.global::<HoldConfirm>().set_duration(i64::from(hold_ms));
}
