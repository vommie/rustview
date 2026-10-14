//! Conversion of the UI-independent button feedback state into the Slint struct of `ui/button_feedback_types.slint`.

use crate::button_feedback::{ButtonFeedback, FeedbackKind};
use crate::ui::{ButtonFeedback as SlintFeedback, FeedbackKind as SlintKind};

pub fn to_slint(feedback: ButtonFeedback) -> SlintFeedback {
    SlintFeedback {
        kind: match feedback.kind {
            FeedbackKind::None => SlintKind::None,
            FeedbackKind::Success => SlintKind::Success,
            FeedbackKind::Error => SlintKind::Error,
        },
        seq: feedback.seq as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outcome_and_the_sequence_cross_the_boundary() {
        let mut fb = ButtonFeedback::default();
        fb.error();
        let slint = to_slint(fb);
        assert_eq!(slint.kind, SlintKind::Error);
        assert_eq!(slint.seq, 1);
        assert_eq!(to_slint(ButtonFeedback::default()).kind, SlintKind::None);
    }
}
