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
