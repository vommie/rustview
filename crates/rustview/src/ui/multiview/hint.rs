//! The hint of the Multiview (UI thread only): a short message centered over the view when a key has nothing to act on ("Select
//! a medium first", `viewer::multiview::hint`).
//!
//! The canvas of the host draws it (`MultiviewHint`, above the media and the bars, no pointer input); this module sets its text
//! and ends its hold. It holds and fades like the feedback of a rating (`media_feedback::{HOLD, FADE}`): after the hold `shown`
//! goes false and the canvas fades it out; a new hint replaces the one on screen at once.

use crate::ui::MultiviewTabBus;
use crate::viewer::multiview::hint::HintKind;
use crate::viewer::multiview::media_feedback::HOLD;
use crate::viewer::multiview::route::HostMode;
use slint::{ComponentHandle, Timer, TimerMode};

thread_local! {
    /// Ends the hold of the hint of the fullscreen window and of the tab view.
    static HOLDS: [Timer; 2] = [Timer::default(), Timer::default()];
}

/// Shows the hint `kind` over the view of `host`.
pub fn show(host: HostMode, kind: HintKind) {
    let Some(slot) = slot(host) else { return };
    if !set(host, Some(kind)) {
        tracing::debug!(?host, "[Multiview: Hint] No window shows the view; no hint");
        return;
    }
    HOLDS.with(|holds| {
        holds[slot].start(TimerMode::SingleShot, HOLD, move || {
            set(host, None);
        });
    });
    tracing::info!(?kind, ?host, "[Multiview: Hint] Shown");
}

/// Puts the hint `kind` on the canvas of `host`, or lets it fade out (`None`). Returns whether the host has a window.
fn set(host: HostMode, kind: Option<HintKind>) -> bool {
    match host {
        HostMode::Fullscreen => {
            let Some(win) = super::session::existing_window() else {
                return false;
            };
            if let Some(kind) = kind {
                win.set_hint_text(kind.text().into());
            }
            win.set_hint_shown(kind.is_some());
        }
        HostMode::TabView => {
            let Some(win) = super::tab_host::main_window() else {
                return false;
            };
            let bus = win.global::<MultiviewTabBus>();
            if let Some(kind) = kind {
                bus.set_hint_text(kind.text().into());
            }
            bus.set_hint_shown(kind.is_some());
        }
        HostMode::TabGrid | HostMode::Hidden => return false,
    }
    true
}

/// The timer of the hosts that draw a hint (the tab grid's miniatures do not).
const fn slot(host: HostMode) -> Option<usize> {
    match host {
        HostMode::Fullscreen => Some(0),
        HostMode::TabView => Some(1),
        HostMode::TabGrid | HostMode::Hidden => None,
    }
}

/// The host stops showing its Multiview: a hint on its canvas goes at once.
pub fn clear(host: HostMode) {
    let Some(slot) = slot(host) else { return };
    HOLDS.with(|holds| holds[slot].stop());
    set(host, None);
}
