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
