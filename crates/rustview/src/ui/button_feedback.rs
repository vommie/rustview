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
