//! The life of the Save button of a tile (pure, the clock is passed in): it appears when the picture has unsaved edits and is
//! emphasized, shows that it is busy while the file is written, turns plain with a check mark once the file is saved, and
//! fades away shortly after. The host arms one single-shot timer for the next change (`wakeup`); nothing polls.

use std::time::{Duration, Instant};

/// How long the saved state stays at full strength.
pub const SAVED_HOLD: Duration = Duration::from_millis(1200);
/// How long it takes to fade out (the Slint animation of the button).
pub const SAVED_FADE: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveBadge {
    /// No button.
    #[default]
    Hidden,
    /// Unsaved edits: the button is there and emphasized.
    Dirty,
    /// The file is being written.
    Saving,
    /// Saved at this moment: plain, then fading out, then gone.
    Saved(Instant),
}

/// What the button looks like now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveLook {
    Emphasized,
    Busy,
    /// Saved; `fading` once the hold is over.
    Done {
        fading: bool,
    },
}

impl SaveBadge {
    /// The look at `now`; `None` = no button.
    pub fn look(self, now: Instant) -> Option<SaveLook> {
        match self {
            Self::Hidden => None,
            Self::Dirty => Some(SaveLook::Emphasized),
            Self::Saving => Some(SaveLook::Busy),
            Self::Saved(at) => {
                let age = now.saturating_duration_since(at);
                (age < SAVED_HOLD + SAVED_FADE).then_some(SaveLook::Done {
                    fading: age >= SAVED_HOLD,
                })
            }
        }
    }

    /// When the look changes next by itself (the start of the fade, the end of it).
    pub fn wakeup(self, now: Instant) -> Option<Instant> {
        let Self::Saved(at) = self else { return None };
        [at + SAVED_HOLD, at + SAVED_HOLD + SAVED_FADE]
            .into_iter()
            .find(|t| *t > now)
    }

    /// The badge after the edits changed: dirty while there are edits, else hidden (a saved badge keeps fading).
    pub fn after_edit(self, dirty: bool) -> Self {
        match (self, dirty) {
            (_, true) => Self::Dirty,
            (Self::Saved(at), false) => Self::Saved(at),
            _ => Self::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_appears_with_edits_shows_the_save_and_fades_away() {
        let t0 = Instant::now();
        let badge = SaveBadge::Hidden.after_edit(true);
        assert_eq!(badge.look(t0), Some(SaveLook::Emphasized));
        assert_eq!(badge.wakeup(t0), None);
        let saved = SaveBadge::Saved(t0);
        assert_eq!(saved.look(t0), Some(SaveLook::Done { fading: false }));
        assert_eq!(saved.wakeup(t0), Some(t0 + SAVED_HOLD));
        let fading = t0 + SAVED_HOLD + Duration::from_millis(10);
        assert_eq!(saved.look(fading), Some(SaveLook::Done { fading: true }));
        assert_eq!(saved.wakeup(fading), Some(t0 + SAVED_HOLD + SAVED_FADE));
        let gone = t0 + SAVED_HOLD + SAVED_FADE;
        assert_eq!(saved.look(gone), None);
        assert_eq!(saved.wakeup(gone), None);
    }

    #[test]
    fn a_new_edit_makes_a_fading_button_emphasized_again() {
        let t0 = Instant::now();
        assert_eq!(SaveBadge::Saved(t0).after_edit(true), SaveBadge::Dirty);
        assert_eq!(SaveBadge::Saved(t0).after_edit(false), SaveBadge::Saved(t0));
        assert_eq!(SaveBadge::Dirty.after_edit(false), SaveBadge::Hidden);
        assert_eq!(SaveBadge::Saving.look(t0), Some(SaveLook::Busy));
    }
}
