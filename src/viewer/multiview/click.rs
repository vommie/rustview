//! Double click detection with an explicit clock (pure).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickKind {
    None,
    Double,
    CtrlDouble,
}

/// Two clicks within `window` and with the same modifier state form a double click; a click with a different modifier
/// state starts a new sequence.
#[derive(Debug, Default)]
pub struct ClickDetector {
    last: Option<(Instant, bool)>,
}

impl ClickDetector {
    pub fn register(&mut self, now: Instant, is_ctrl: bool, window: Duration) -> ClickKind {
        match self.last {
            Some((at, ctrl)) if ctrl == is_ctrl && now.saturating_duration_since(at) < window => {
                self.last = None;
                if is_ctrl {
                    ClickKind::CtrlDouble
                } else {
                    ClickKind::Double
                }
            }
            _ => {
                self.last = Some((now, is_ctrl));
                ClickKind::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_millis(400);

    #[test]
    fn two_quick_clicks_make_a_double_click() {
        let t0 = Instant::now();
        let mut d = ClickDetector::default();
        assert_eq!(d.register(t0, false, WINDOW), ClickKind::None);
        assert_eq!(
            d.register(t0 + Duration::from_millis(120), false, WINDOW),
            ClickKind::Double
        );
    }

    #[test]
    fn ctrl_clicks_report_a_ctrl_double_click() {
        let t0 = Instant::now();
        let mut d = ClickDetector::default();
        assert_eq!(d.register(t0, true, WINDOW), ClickKind::None);
        assert_eq!(
            d.register(t0 + Duration::from_millis(399), true, WINDOW),
            ClickKind::CtrlDouble
        );
    }

    #[test]
    fn a_slow_second_click_starts_a_new_sequence() {
        let t0 = Instant::now();
        let mut d = ClickDetector::default();
        assert_eq!(d.register(t0, false, WINDOW), ClickKind::None);
        assert_eq!(
            d.register(t0 + Duration::from_millis(400), false, WINDOW),
            ClickKind::None
        );
        assert_eq!(
            d.register(t0 + Duration::from_millis(500), false, WINDOW),
            ClickKind::Double,
            "the slow click became the first click"
        );
    }

    #[test]
    fn mixed_modifiers_never_pair_up() {
        let t0 = Instant::now();
        let mut d = ClickDetector::default();
        assert_eq!(d.register(t0, false, WINDOW), ClickKind::None);
        assert_eq!(
            d.register(t0 + Duration::from_millis(50), true, WINDOW),
            ClickKind::None
        );
        assert_eq!(
            d.register(t0 + Duration::from_millis(100), false, WINDOW),
            ClickKind::None
        );
    }

    #[test]
    fn a_third_click_starts_over() {
        let t0 = Instant::now();
        let mut d = ClickDetector::default();
        d.register(t0, false, WINDOW);
        assert_eq!(
            d.register(t0 + Duration::from_millis(10), false, WINDOW),
            ClickKind::Double
        );
        assert_eq!(
            d.register(t0 + Duration::from_millis(20), false, WINDOW),
            ClickKind::None
        );
    }

    #[test]
    fn a_clock_that_runs_backwards_does_not_panic() {
        let t0 = Instant::now() + Duration::from_secs(5);
        let mut d = ClickDetector::default();
        d.register(t0, false, WINDOW);
        assert_eq!(
            d.register(t0 - Duration::from_secs(1), false, WINDOW),
            ClickKind::Double
        );
    }
}
