//! The short hint a Multiview shows when a key has nothing to act on (pure): its text and where it goes.
//!
//! It looks and times like the feedback of a rating (`media_feedback::{HOLD, FADE}`): full strength first, then it fades;
//! a new hint replaces the one on screen at once. The host shows it centered over the view (`ui::multiview::hint`).

use super::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    /// Several media are shown and none is selected: a key that acts on one medium has no target.
    NoSelection,
}

impl HintKind {
    pub const fn text(self) -> &'static str {
        match self {
            Self::NoSelection => "Select a medium first",
        }
    }
}

/// Where a hint is shown: the window of the host and the view in it (physical pixels, window coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HintAnchor {
    pub host_xid: u64,
    pub view: Rect,
    pub dpi: f32,
}

impl HintAnchor {
    /// The middle of the view, in window coordinates.
    pub fn center(&self) -> (i32, i32) {
        (self.view.x + self.view.w / 2, self.view.y + self.view.h / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_is_centered_in_the_view() {
        let anchor = HintAnchor {
            host_xid: 1,
            view: Rect::new(100, 50, 801, 400),
            dpi: 1.0,
        };
        assert_eq!(anchor.center(), (500, 250));
    }

    #[test]
    fn the_text_asks_for_a_selection() {
        assert_eq!(HintKind::NoSelection.text(), "Select a medium first");
    }
}
