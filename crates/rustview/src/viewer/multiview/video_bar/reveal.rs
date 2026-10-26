//! When the control bar of a video shows (pure, the clock is passed in).
//!
//! "Bottom": while the pointer is over the lower part of the video (the bar plus a margin above it). "Anywhere": every
//! movement of the pointer over the video shows the bar, and it goes after `idle` without movement. Both keep the bar while
//! the pointer is on it or a drag on it runs, whether the video plays or is paused. A bar that goes fades out first (when
//! fading is on): its window stays until the fade is over.

use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::video_bar::prefs::RevealMode;
use std::time::{Duration, Instant};

/// What the window of a bar has to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Hidden,
    Shown,
    /// Still mapped while its content fades out.
    FadingOut,
}

/// What the decision needs to know, in host window pixels.
#[derive(Debug, Clone, Copy)]
pub struct RevealInput {
    pub mode: RevealMode,
    pub video: Rect,
    pub bar: Rect,
    pub pointer: Option<(i32, i32)>,
    /// Reveal "bottom": how far above the bar the pointer still shows it.
    pub margin_px: i32,
    /// A drag on the bar runs: it stays whatever the pointer does.
    pub forced: bool,
    pub idle: Duration,
    /// How long the bar fades out (`None`: it goes at once).
    pub fade: Option<Duration>,
}

#[derive(Debug, Default)]
pub struct BarReveal {
    last_pointer: Option<(i32, i32)>,
    /// When the pointer last moved over the video.
    moved_at: Option<Instant>,
    shown: bool,
    /// The bar fades out, and its window goes at this time.
    gone_at: Option<Instant>,
    /// A key moved the video: the bar shows until this time, wherever the pointer is.
    pulse_until: Option<Instant>,
}

impl BarReveal {
    pub fn decide(&mut self, input: &RevealInput, now: Instant) -> Phase {
        let inside = |rect: Rect| input.pointer.is_some_and(|(x, y)| rect.contains(x, y));
        let over_video = inside(input.video);
        if input.pointer != self.last_pointer && over_video {
            self.moved_at = Some(now);
        }
        self.last_pointer = input.pointer;
        let pulsed = self.pulse_until.is_some_and(|until| now < until);
        if !pulsed {
            self.pulse_until = None;
        }
        let wanted = input.forced
            || pulsed
            || inside(input.bar)
            || match input.mode {
                RevealMode::Bottom => {
                    over_video && input.pointer.is_some_and(|(_, y)| y >= input.bar.y - input.margin_px)
                }
                RevealMode::Anywhere => {
                    over_video
                        && self
                            .moved_at
                            .is_some_and(|at| now.saturating_duration_since(at) < input.idle)
                }
                RevealMode::Hover => over_video,
            };
        if wanted {
            self.shown = true;
            self.gone_at = None;
            return Phase::Shown;
        }
        if std::mem::take(&mut self.shown) {
            self.gone_at = input.fade.map(|fade| now + fade);
        }
        match self.gone_at {
            Some(at) if now < at => Phase::FadingOut,
            _ => {
                self.gone_at = None;
                Phase::Hidden
            }
        }
    }

    /// A key moved the video (a jump, a frame step, an A-B repeat key; never the loop's own jumps): the bar shows until
    /// `until`, like the volume bar after a wheel step.
    pub fn pulse(&mut self, until: Instant) {
        self.pulse_until = Some(self.pulse_until.map_or(until, |current| current.max(until)));
    }

    /// The bar is on screen (shown or fading out): it shows the playing time, which moves.
    pub fn visible(&self) -> bool {
        self.shown || self.gone_at.is_some()
    }

    /// The next moment after `now` at which `decide` may answer differently without the pointer moving: the idle time after the
    /// last movement runs out, the fade ends or a pulse is over. `None`: nothing changes until the pointer moves.
    pub fn deadline(&self, idle: Duration, now: Instant) -> Option<Instant> {
        [self.moved_at.map(|at| at + idle), self.gone_at, self.pulse_until]
            .into_iter()
            .flatten()
            .filter(|at| *at > now)
            .min()
    }

    /// The bar goes at once (the host does not show it any more).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_is_the_next_change_without_movement() {
        let t0 = Instant::now();
        let idle = Duration::from_millis(1500);
        let mut reveal = BarReveal::default();
        assert_eq!(reveal.deadline(idle, t0), None);
        let mut moving = input(RevealMode::Anywhere, Some((100, 100)));
        moving.fade = Some(Duration::from_millis(200));
        assert_eq!(reveal.decide(&moving, t0), Phase::Shown);
        assert!(reveal.visible());
        assert_eq!(reveal.deadline(idle, t0), Some(t0 + idle));
        let later = t0 + idle;
        assert_eq!(reveal.decide(&moving, later), Phase::FadingOut);
        assert_eq!(reveal.deadline(idle, later), Some(later + Duration::from_millis(200)));
        let gone = later + Duration::from_millis(200);
        assert_eq!(reveal.decide(&moving, gone), Phase::Hidden);
        assert!(!reveal.visible());
        assert_eq!(reveal.deadline(idle, gone), None);
    }

    const VIDEO: Rect = Rect::new(0, 0, 800, 450);
    const BAR: Rect = Rect::new(0, 420, 800, 30);

    fn input(mode: RevealMode, pointer: Option<(i32, i32)>) -> RevealInput {
        RevealInput {
            mode,
            video: VIDEO,
            bar: BAR,
            pointer,
            margin_px: 12,
            forced: false,
            idle: Duration::from_millis(1500),
            fade: Some(Duration::from_millis(150)),
        }
    }

    #[test]
    fn bottom_shows_near_the_bottom_only_and_fades_out() {
        let t0 = Instant::now();
        let mut reveal = BarReveal::default();
        assert_eq!(
            reveal.decide(&input(RevealMode::Bottom, Some((400, 200))), t0),
            Phase::Hidden
        );
        assert_eq!(
            reveal.decide(&input(RevealMode::Bottom, Some((400, 410))), t0),
            Phase::Shown,
            "within the margin"
        );
        assert_eq!(
            reveal.decide(
                &input(RevealMode::Bottom, Some((400, 410))),
                t0 + Duration::from_secs(9)
            ),
            Phase::Shown,
            "resting is fine"
        );
        let gone = t0 + Duration::from_secs(10);
        assert_eq!(
            reveal.decide(&input(RevealMode::Bottom, Some((400, 100))), gone),
            Phase::FadingOut
        );
        assert_eq!(
            reveal.decide(
                &input(RevealMode::Bottom, Some((400, 100))),
                gone + Duration::from_millis(100)
            ),
            Phase::FadingOut
        );
        assert_eq!(
            reveal.decide(
                &input(RevealMode::Bottom, Some((400, 100))),
                gone + Duration::from_millis(160)
            ),
            Phase::Hidden
        );
        assert_eq!(reveal.decide(&input(RevealMode::Bottom, None), gone), Phase::Hidden);
    }

    #[test]
    fn anywhere_shows_on_movement_and_goes_when_the_pointer_rests() {
        let t0 = Instant::now();
        let mut reveal = BarReveal::default();
        assert_eq!(
            reveal.decide(&input(RevealMode::Anywhere, Some((100, 100))), t0),
            Phase::Shown
        );
        let rest = t0 + Duration::from_millis(1000);
        assert_eq!(
            reveal.decide(&input(RevealMode::Anywhere, Some((100, 100))), rest),
            Phase::Shown
        );
        let later = t0 + Duration::from_millis(1600);
        assert_eq!(
            reveal.decide(&input(RevealMode::Anywhere, Some((100, 100))), later),
            Phase::FadingOut
        );
        let moved = later + Duration::from_millis(50);
        assert_eq!(
            reveal.decide(&input(RevealMode::Anywhere, Some((120, 100))), moved),
            Phase::Shown
        );
        // Resting on the bar keeps it.
        let on_bar = moved + Duration::from_secs(5);
        assert_eq!(
            reveal.decide(&input(RevealMode::Anywhere, Some((120, 430))), on_bar),
            Phase::Shown
        );
        assert_eq!(
            reveal.decide(
                &input(RevealMode::Anywhere, Some((120, 430))),
                on_bar + Duration::from_secs(5)
            ),
            Phase::Shown
        );
    }

    #[test]
    fn hover_shows_while_the_pointer_is_over_the_video_resting_or_not() {
        let t0 = Instant::now();
        let mut reveal = BarReveal::default();
        assert_eq!(
            reveal.decide(&input(RevealMode::Hover, Some((100, 100))), t0),
            Phase::Shown
        );
        assert_eq!(
            reveal.decide(
                &input(RevealMode::Hover, Some((100, 100))),
                t0 + Duration::from_secs(30)
            ),
            Phase::Shown
        );
        assert_eq!(
            reveal.decide(
                &input(RevealMode::Hover, Some((900, 100))),
                t0 + Duration::from_secs(31)
            ),
            Phase::FadingOut
        );
    }

    #[test]
    fn a_drag_keeps_the_bar_and_without_fading_it_goes_at_once() {
        let t0 = Instant::now();
        let mut reveal = BarReveal::default();
        let mut dragging = input(RevealMode::Bottom, Some((400, 10)));
        dragging.forced = true;
        dragging.fade = None;
        assert_eq!(reveal.decide(&dragging, t0), Phase::Shown);
        dragging.forced = false;
        assert_eq!(reveal.decide(&dragging, t0), Phase::Hidden);
    }
}
