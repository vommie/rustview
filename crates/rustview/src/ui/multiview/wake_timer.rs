//! The tick timer of a host (UI thread only): a single-shot timer armed for the moment the shown instance says something
//! changes by itself (`MultiviewInstance::tick_deadline`). It never runs while nothing is pending, and an earlier wake-up is
//! never pushed back by a later one.

use slint::{Timer, TimerMode};
use std::time::{Duration, Instant};

/// A timer fires a little after the moment it was armed for, never before: the deadline has passed when the tick looks.
const SLACK: Duration = Duration::from_millis(2);

#[derive(Default)]
pub struct WakeTimer {
    timer: Timer,
    due: Option<Instant>,
}

impl WakeTimer {
    /// Arms the timer for `deadline` unless it already fires at or before it; `None` leaves it as it is (a wake-up that is
    /// already armed is harmless: the tick finds nothing to do and does not arm again).
    pub fn arm(&mut self, deadline: Option<Instant>, now: Instant, fire: impl FnMut() + 'static) {
        let Some(deadline) = deadline else {
            return;
        };
        if self.timer.running() && self.due.is_some_and(|due| due <= deadline) {
            return;
        }
        self.due = Some(deadline);
        self.timer.start(
            TimerMode::SingleShot,
            deadline.saturating_duration_since(now) + SLACK,
            fire,
        );
    }

    /// The timer fired: the next `arm` starts it again.
    pub fn fired(&mut self) {
        self.due = None;
    }

    pub fn stop(&mut self) {
        self.timer.stop();
        self.due = None;
    }

    pub fn running(&self) -> bool {
        self.timer.running()
    }
}
