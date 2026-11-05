//! How much of a control bar fits (pure): the steps a bar collapses through as its video gets narrower, and which step a
//! width gets. The canvas measures what each step needs besides the progress bar (`ui/multiview_video_bar.slint` reports it,
//! nothing is guessed here), the progress bar keeps a minimum width, and a bar grows back only with some room to spare.

/// The steps of a bar, from the full bar to the narrowest. Each step keeps what the one before it collapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BarFit {
    /// Everything: play, both times, the progress bar, the A-B group, the filter button, the speaker and the volume slider.
    Full,
    /// The volume slider gives way: the speaker opens a vertical volume bar.
    AudioButton,
    /// The remaining time goes as well.
    NoRemaining,
    /// The A-B group becomes one button that shows the group in a bubble.
    AbButton,
    /// The filter button gives way to "..." (its menu has the filters).
    FilterMore,
    /// The sound and the A-B repeat move into the "..." menu as well.
    More,
    /// The current time goes as well (the "..." menu names both times).
    NoTime,
}

impl BarFit {
    pub const ALL: [Self; 7] = [
        Self::Full,
        Self::AudioButton,
        Self::NoRemaining,
        Self::AbButton,
        Self::FilterMore,
        Self::More,
        Self::NoTime,
    ];

    /// The number the canvas knows the step by (its position in `ALL`).
    pub const fn code(self) -> i32 {
        match self {
            Self::Full => 0,
            Self::AudioButton => 1,
            Self::NoRemaining => 2,
            Self::AbButton => 3,
            Self::FilterMore => 4,
            Self::More => 5,
            Self::NoTime => 6,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Full => 0,
            Self::AudioButton => 1,
            Self::NoRemaining => 2,
            Self::AbButton => 3,
            Self::FilterMore => 4,
            Self::More => 5,
            Self::NoTime => 6,
        }
    }

    /// The volume slider is in the bar.
    pub fn has_slider(self) -> bool {
        self == Self::Full
    }

    /// The speaker (or the volume popup it opens) is in the bar, not in the "..." menu.
    pub fn sound_in_bar(self) -> bool {
        self < Self::More
    }

    /// The sound and the A-B repeat are entries of the "..." menu.
    pub fn sound_in_more(self) -> bool {
        !self.sound_in_bar()
    }

    /// The filter button is in the bar (else its filters are a submenu of "...").
    pub fn filter_in_bar(self) -> bool {
        self < Self::FilterMore
    }

    /// The A-B group is one button with a bubble.
    pub fn ab_as_button(self) -> bool {
        matches!(self, Self::AbButton | Self::FilterMore)
    }

    /// The bar has "..." (the filters, later also the sound and the A-B repeat, are its entries).
    pub fn has_more(self) -> bool {
        self >= Self::FilterMore
    }
}

/// What the canvas measured for every step: the width of everything but the progress bar (paddings, gaps, buttons, times),
/// in logical pixels, in the order of `BarFit::ALL`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarNeeds(pub [f32; 7]);

impl BarNeeds {
    /// The widths the canvas reported (`None` unless there is a finite, non-negative width for every step).
    pub fn from_reported(widths: &[f32]) -> Option<Self> {
        let widths: [f32; 7] = widths.try_into().ok()?;
        widths
            .iter()
            .all(|w| w.is_finite() && *w >= 0.0)
            .then_some(Self(widths))
    }

    fn of(&self, fit: BarFit) -> f32 {
        self.0[fit.index()]
    }
}

/// The step for a bar `width` wide (logical pixels) whose progress bar keeps at least `seek_min`. The fullest step that
/// fits is taken, but a step fuller than `previous` only with `hysteresis` to spare, so a width at a boundary keeps its
/// step. When not even the narrowest fits, the narrowest it is (the progress bar gets what is left).
pub fn decide(previous: Option<BarFit>, width: f32, needs: &BarNeeds, seek_min: f32, hysteresis: f32) -> BarFit {
    BarFit::ALL
        .into_iter()
        .find(|&fit| {
            let spare = match previous {
                Some(previous) if fit < previous => hysteresis,
                _ => 0.0,
            };
            needs.of(fit) + seek_min + spare <= width
        })
        .unwrap_or(BarFit::NoTime)
}

/// What a bar remembers about its fit: the widths the canvas measured last and the step it showed.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct BarFitState {
    needs: Option<BarNeeds>,
    shown: Option<BarFit>,
}

impl BarFitState {
    /// The canvas measured the bar. Returns whether that changed anything.
    pub fn measured(&mut self, needs: BarNeeds) -> bool {
        let changed = self.needs != Some(needs);
        self.needs = Some(needs);
        changed
    }

    /// The step for a bar `width` wide now. Until the canvas has measured the bar it is the narrowest (the progress bar is
    /// never squeezed while the bar fades in; the measured step follows a frame later).
    pub fn fit(&mut self, width: f32, seek_min: f32, hysteresis: f32) -> BarFit {
        let Some(needs) = self.needs else {
            return BarFit::NoTime;
        };
        let fit = decide(self.shown, width, &needs, seek_min, hysteresis);
        if self.shown != Some(fit) {
            tracing::debug!(
                from = ?self.shown,
                to = ?fit,
                width,
                "[Multiview: VideoBar] Collapse step"
            );
        }
        self.shown = Some(fit);
        fit
    }

    /// The bar went away: the next bar starts without a step of its own (the measurements stay, they do not depend on it).
    pub fn reset(&mut self) {
        self.shown = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs of a bar 30 px high: every step a bit narrower than the one before.
    const NEEDS: BarNeeds = BarNeeds([400.0, 330.0, 280.0, 200.0, 175.0, 150.0, 100.0]);
    const SEEK: f32 = 90.0;
    const HYST: f32 = 16.0;

    #[test]
    fn the_fullest_step_that_leaves_the_progress_bar_its_minimum() {
        assert_eq!(decide(None, 1000.0, &NEEDS, SEEK, HYST), BarFit::Full);
        assert_eq!(decide(None, 490.0, &NEEDS, SEEK, HYST), BarFit::Full);
        assert_eq!(decide(None, 489.0, &NEEDS, SEEK, HYST), BarFit::AudioButton);
        assert_eq!(decide(None, 370.0, &NEEDS, SEEK, HYST), BarFit::NoRemaining);
        assert_eq!(decide(None, 300.0, &NEEDS, SEEK, HYST), BarFit::AbButton);
        assert_eq!(decide(None, 265.0, &NEEDS, SEEK, HYST), BarFit::FilterMore);
        assert_eq!(decide(None, 240.0, &NEEDS, SEEK, HYST), BarFit::More);
        assert_eq!(decide(None, 190.0, &NEEDS, SEEK, HYST), BarFit::NoTime);
    }

    #[test]
    fn too_narrow_for_every_step_is_the_narrowest() {
        assert_eq!(decide(None, 120.0, &NEEDS, SEEK, HYST), BarFit::NoTime);
        assert_eq!(decide(Some(BarFit::Full), 0.0, &NEEDS, SEEK, HYST), BarFit::NoTime);
    }

    #[test]
    fn shrinking_collapses_at_once_and_growing_needs_room_to_spare() {
        // Shrinking: the first pixel below the boundary collapses.
        assert_eq!(
            decide(Some(BarFit::Full), 489.0, &NEEDS, SEEK, HYST),
            BarFit::AudioButton
        );
        // Growing back: exactly at the boundary and up to the hysteresis it stays collapsed.
        assert_eq!(
            decide(Some(BarFit::AudioButton), 490.0, &NEEDS, SEEK, HYST),
            BarFit::AudioButton
        );
        assert_eq!(
            decide(Some(BarFit::AudioButton), 505.0, &NEEDS, SEEK, HYST),
            BarFit::AudioButton
        );
        assert_eq!(
            decide(Some(BarFit::AudioButton), 506.0, &NEEDS, SEEK, HYST),
            BarFit::Full
        );
        // A step that still fits is kept, whatever is between.
        assert_eq!(decide(Some(BarFit::More), 280.0, &NEEDS, SEEK, HYST), BarFit::More);
        assert_eq!(decide(Some(BarFit::More), 306.0, &NEEDS, SEEK, HYST), BarFit::AbButton);
        // A big jump in width skips steps in both directions.
        assert_eq!(decide(Some(BarFit::NoTime), 1000.0, &NEEDS, SEEK, HYST), BarFit::Full);
        assert_eq!(decide(Some(BarFit::Full), 150.0, &NEEDS, SEEK, HYST), BarFit::NoTime);
    }

    #[test]
    fn a_width_wobbling_at_a_boundary_does_not_flicker() {
        let mut state = BarFitState::default();
        assert!(state.measured(NEEDS));
        let mut seen = Vec::new();
        for width in [495.0, 489.0, 491.0, 489.5, 495.0, 500.0, 488.0, 505.0] {
            seen.push(state.fit(width, SEEK, HYST));
        }
        assert_eq!(seen[0], BarFit::Full);
        assert!(seen[1..].iter().all(|&fit| fit == BarFit::AudioButton), "{seen:?}");
        assert_eq!(state.fit(507.0, SEEK, HYST), BarFit::Full);
    }

    #[test]
    fn an_unmeasured_bar_is_the_narrowest_and_a_new_bar_has_no_history() {
        let mut state = BarFitState::default();
        assert_eq!(state.fit(2000.0, SEEK, HYST), BarFit::NoTime);
        state.measured(NEEDS);
        assert!(!state.measured(NEEDS), "the same widths again change nothing");
        assert_eq!(state.fit(495.0, SEEK, HYST), BarFit::Full);
        assert_eq!(state.fit(489.0, SEEK, HYST), BarFit::AudioButton);
        state.reset();
        assert_eq!(state.fit(495.0, SEEK, HYST), BarFit::Full);
    }

    #[test]
    fn the_reported_widths_need_one_finite_width_per_step() {
        assert_eq!(BarNeeds::from_reported(&NEEDS.0), Some(NEEDS));
        assert_eq!(BarNeeds::from_reported(&[1.0; 6]), None);
        assert_eq!(BarNeeds::from_reported(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, f32::NAN]), None);
        assert_eq!(BarNeeds::from_reported(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, -1.0]), None);
    }

    #[test]
    fn what_each_step_keeps() {
        assert!(BarFit::Full.has_slider() && !BarFit::AudioButton.has_slider());
        assert!(BarFit::FilterMore.sound_in_bar() && !BarFit::More.sound_in_bar() && BarFit::NoTime.sound_in_more());
        assert!(BarFit::AbButton.filter_in_bar() && !BarFit::FilterMore.filter_in_bar());
        assert!(
            BarFit::FilterMore.ab_as_button() && !BarFit::More.ab_as_button() && !BarFit::NoRemaining.ab_as_button()
        );
        assert!(!BarFit::AbButton.has_more() && BarFit::FilterMore.has_more() && BarFit::NoTime.has_more());
        let codes: Vec<i32> = BarFit::ALL.iter().map(|fit| fit.code()).collect();
        assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6]);
    }
}
