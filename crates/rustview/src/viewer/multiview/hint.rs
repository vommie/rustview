//! The short hint a Multiview shows when a key has nothing to act on (pure): its text and where it goes.
//!
//! A key that needs a medium does nothing while several are shown and none is selected ("Select a medium first"), or when it does
//! not fit the selected one ("Nothing to save", ...).
//!
//! It looks and times like the feedback of a rating (`media_feedback::{HOLD, FADE}`): full strength first, then it fades;
//! a new hint replaces the one on screen at once. The host shows it centered over the view (`ui::multiview::hint`).

use super::layout::Rect;

/// Why a key did nothing. Every reason but `NoSelection` concerns the selected medium, whose marks the key shows for a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    /// Several media are shown and none is selected: a key that acts on one medium has no target.
    NoSelection,
    NothingToSave,
    NothingToUndo,
    NothingToRedo,
    NoCropToApply,
    /// The zoom keys on a video (mpv has its own zoom).
    VideoNotZoomable,
    /// The zoom keys on a picture that is still loading or could not be shown.
    PictureNotZoomable,
    /// Undo, redo or another save while the picture is being written.
    SaveInProgress,
    /// The rating keys on a medium that cannot be rated (pending, deleted).
    NotRateable,
    /// The key of a media bar button the selected medium does not offer right now.
    NotOffered,
}

impl HintKind {
    pub const fn text(self) -> &'static str {
        match self {
            Self::NoSelection => "Select a medium first",
            Self::NothingToSave => "Nothing to save",
            Self::NothingToUndo => "Nothing to undo",
            Self::NothingToRedo => "Nothing to redo",
            Self::NoCropToApply => "No crop to apply",
            Self::VideoNotZoomable => "Videos cannot be zoomed here",
            Self::PictureNotZoomable => "This picture cannot be zoomed",
            Self::SaveInProgress => "Saving in progress",
            Self::NotRateable => "This medium cannot be rated",
            Self::NotOffered => "Not available for this medium",
        }
    }

    /// The hint is about the selected medium (not about a missing selection).
    pub const fn concerns_selection(self) -> bool {
        !matches!(self, Self::NoSelection)
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
    fn the_text_asks_for_a_selection_or_names_the_reason() {
        assert_eq!(HintKind::NoSelection.text(), "Select a medium first");
        assert!(!HintKind::NoSelection.concerns_selection());
        assert_eq!(HintKind::NothingToSave.text(), "Nothing to save");
        assert!(HintKind::VideoNotZoomable.concerns_selection());
    }
}
