//! When the tag tree is filled again after its height changed the rows per column.
//!
//! Filling the tree builds every visible tag again, which takes tens of milliseconds with hundreds of tags. A single change
//! (the window is maximized, a panel folds) is shown at once. While a resizer is dragged the height changes many times a
//! second: the tree then waits until the pointer rests for a moment, and at the latest every 400 ms, so the drag stays
//! smooth and the tree still follows.

use std::time::{Duration, Instant};

/// A change this long after the last fill is shown at once.
const CALM: Duration = Duration::from_millis(250);
/// How long the height has to rest before a waiting fill happens.
const REST: Duration = Duration::from_millis(80);
/// A waiting fill happens after this time even if the height keeps changing.
const LONGEST_WAIT: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refill {
    Now,
    /// Fill when this time has passed without another change (a new change asks again).
    After(Duration),
}

#[derive(Debug, Default)]
pub struct RefillPace {
    last_fill: Option<Instant>,
    waiting_since: Option<Instant>,
}

impl RefillPace {
    /// The rows per column changed at `now`.
    pub fn on_change(&mut self, now: Instant) -> Refill {
        let calm = self.last_fill.is_none_or(|last| now.duration_since(last) >= CALM);
        if calm && self.waiting_since.is_none() {
            return Refill::Now;
        }
        let since = *self.waiting_since.get_or_insert(now);
        let waited = now.duration_since(since);
        if waited >= LONGEST_WAIT {
            Refill::Now
        } else {
            Refill::After(REST.min(LONGEST_WAIT.saturating_sub(waited)))
        }
    }

    /// The tree was filled at `now`.
    pub fn filled(&mut self, now: Instant) {
        self.last_fill = Some(now);
        self.waiting_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_change_is_shown_at_once_and_a_drag_waits_for_a_rest() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut pace = RefillPace::default();
        assert_eq!(pace.on_change(at(0)), Refill::Now, "the first change");
        pace.filled(at(0));
        // A resizer is dragged: changes every 50 ms.
        assert_eq!(pace.on_change(at(50)), Refill::After(REST));
        assert_eq!(pace.on_change(at(100)), Refill::After(REST));
        assert_eq!(
            pace.on_change(at(400)),
            Refill::After(Duration::from_millis(50)),
            "never later than 400 ms"
        );
        assert_eq!(pace.on_change(at(450)), Refill::Now);
        pace.filled(at(450));
        assert_eq!(pace.on_change(at(500)), Refill::After(REST), "the drag goes on");
        pace.filled(at(580));
        // Long after the drag a change is shown at once again.
        assert_eq!(pace.on_change(at(2000)), Refill::Now);
    }
}
