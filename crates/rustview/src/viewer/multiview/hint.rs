//! The short hint a Multiview shows when a key has nothing to act on (pure): its text.
//!
//! A key that needs a medium does nothing while several are shown and none is selected ("Select a medium first"), or when it does
//! not fit the selected one ("Nothing to save", ...).
//!
//! It looks and times like the feedback of a rating (`media_feedback::{HOLD, FADE}`): full strength first, then it fades;
//! a new hint replaces the one on screen at once. The canvas of the host shows it centered over the view (`ui::multiview::hint`).

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
    /// A video player key while the selected medium is a picture (or a video that does not play).
    NotAVideo,
    /// A key of the area "MPV `ab_repeat.lua`" while the script is not active in the selected video's player.
    AbRepeatOff,
    /// Saving an A-B repeat without a valid A and B.
    AbNoRange,
    /// The load menu or next/previous repeat on a video without saved repeats.
    AbNoSavedRepeats,
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
            Self::NotAVideo => "Select a video",
            Self::AbRepeatOff => "The mpv script ab_repeat.lua is not active here",
            Self::AbNoRange => "Set A and B first",
            Self::AbNoSavedRepeats => "No saved repeats for this video",
        }
    }

    /// The hint is about the selected medium (not about a missing selection).
    pub const fn concerns_selection(self) -> bool {
        !matches!(self, Self::NoSelection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_asks_for_a_selection_or_names_the_reason() {
        assert_eq!(HintKind::NoSelection.text(), "Select a medium first");
        assert!(!HintKind::NoSelection.concerns_selection());
        assert_eq!(HintKind::NothingToSave.text(), "Nothing to save");
        assert!(HintKind::VideoNotZoomable.concerns_selection());
    }
}
