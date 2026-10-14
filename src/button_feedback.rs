//! UI-independent state of the short visual feedback a button gives after an action (pure).
//!
//! A button owns one [`ButtonFeedback`]. Every `success()` / `error()` bumps `seq`, so repeating the same outcome plays
//! the feedback again; the UI component starts its animation whenever `seq` changes and clears itself, so nobody has to
//! reset the state. The Slint side lives in `ui/button_feedback*.slint`, the conversion in `ui::button_feedback_bridge`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FeedbackKind {
    #[default]
    None,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ButtonFeedback {
    pub kind: FeedbackKind,
    pub seq: u32,
}

impl ButtonFeedback {
    pub fn trigger(&mut self, kind: FeedbackKind) {
        self.kind = kind;
        self.seq = self.seq.wrapping_add(1);
    }

    pub fn success(&mut self) {
        self.trigger(FeedbackKind::Success);
    }

    pub fn error(&mut self) {
        self.trigger(FeedbackKind::Error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_button_shows_no_feedback() {
        let fb = ButtonFeedback::default();
        assert_eq!((fb.kind, fb.seq), (FeedbackKind::None, 0));
    }

    #[test]
    fn every_trigger_changes_the_sequence_even_for_the_same_outcome() {
        let mut fb = ButtonFeedback::default();
        fb.success();
        let first = fb;
        fb.success();
        assert_eq!(fb.kind, FeedbackKind::Success);
        assert_ne!(fb.seq, first.seq, "a repeated success must replay");
        fb.error();
        assert_eq!(fb.kind, FeedbackKind::Error);
        assert_eq!(fb.seq, first.seq.wrapping_add(2));
    }

    #[test]
    fn the_sequence_wraps_instead_of_panicking() {
        let mut fb = ButtonFeedback {
            kind: FeedbackKind::None,
            seq: u32::MAX,
        };
        fb.error();
        assert_eq!(fb.seq, 0);
    }
}
