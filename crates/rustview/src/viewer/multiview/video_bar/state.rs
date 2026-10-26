//! What the control bar of one video tile remembers (pure): whether its times show milliseconds, the drag on its progress
//! bar, the seek that is on its way, the last audible level and when it shows.

use super::ab_state::AbBarState;
use super::reveal::BarReveal;
use super::volume;
use rustview_core::library::models::MediaId;
use std::time::{Duration, Instant};

/// How long the bar shows the target of a seek while mpv has not arrived there yet (it does not jump back for a moment).
const SEEK_SETTLE: Duration = Duration::from_millis(1000);

/// mpv is at the target of an exact seek when its position is this close to it.
const SEEK_ARRIVED_SECS: f64 = 0.25;

/// A seek by keyframes lands on the keyframe next to the target, which can be some seconds away.
const KEYFRAME_SEEK_ARRIVED_SECS: f64 = 10.0;

#[derive(Debug, Clone, Copy)]
struct PendingSeek {
    target: f64,
    until: Instant,
    arrived_within: f64,
}

#[derive(Debug, Default)]
pub struct VideoBarState {
    pub reveal: BarReveal,
    /// A click on a time switched the milliseconds for this medium (a new medium starts as the settings say).
    ms_switched: Option<(MediaId, bool)>,
    /// The progress bar is dragged: where the pointer is, as a share of the bar.
    drag: Option<f32>,
    /// The time of the last seek, until when the bar shows it instead of mpv's position, and how close mpv has to come.
    pending_seek: Option<PendingSeek>,
    /// The volume slider is dragged.
    volume_drag: bool,
    /// The pointer is over the progress bar: where, as a share of the bar.
    hover: Option<f32>,
    last_audible: Option<f64>,
    /// The A-B repeat group (popups, the strip under the pointer, a dragged flag).
    pub ab: AbBarState,
}

impl VideoBarState {
    /// Whether the times of `media` show milliseconds (`default` = the setting).
    pub fn ms_shown(&self, media: MediaId, default: bool) -> bool {
        match self.ms_switched {
            Some((switched, shown)) if switched == media => shown,
            _ => default,
        }
    }

    /// A click on a time: the milliseconds of both times switch, for this medium only.
    pub fn toggle_ms(&mut self, media: MediaId, default: bool) {
        let shown = !self.ms_shown(media, default);
        self.ms_switched = Some((media, shown));
    }

    /// A drag on the progress bar runs, the volume slider or an A-B flag is dragged, or an A-B popup is open: the bar stays
    /// whatever the pointer does.
    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.volume_drag || self.ab.holds_bar()
    }

    /// The progress bar is pressed or dragged to `share` (`released`: let go there). `seek` is how the video goes to `secs`
    /// (`None`: it does not seek for this step, `Some(true)`: exactly).
    pub fn seek_to(&mut self, share: f32, secs: f64, now: Instant, released: bool, seek: Option<bool>) {
        self.drag = (!released).then_some(share.clamp(0.0, 1.0));
        if let Some(exact) = seek {
            self.pending_seek = Some(PendingSeek {
                target: secs,
                until: now + SEEK_SETTLE,
                arrived_within: if exact {
                    SEEK_ARRIVED_SECS
                } else {
                    KEYFRAME_SEEK_ARRIVED_SECS
                },
            });
        }
    }

    /// The position the bar shows: the target of a seek until mpv is there (or gives up), else mpv's.
    pub fn shown_position(&mut self, actual: f64, now: Instant) -> f64 {
        let Some(pending) = self.pending_seek else {
            return actual;
        };
        if self.drag.is_none() && ((actual - pending.target).abs() < pending.arrived_within || now >= pending.until) {
            self.pending_seek = None;
            return actual;
        }
        pending.target
    }

    /// When the shown target of a seek gives way to mpv's position at the latest (`None`: no seek on its way).
    pub fn seek_deadline(&self) -> Option<Instant> {
        self.pending_seek.map(|pending| pending.until)
    }

    pub fn set_volume_drag(&mut self, dragging: bool) {
        self.volume_drag = dragging;
    }

    pub fn set_hover(&mut self, share: Option<f32>) {
        self.hover = share.map(|s| s.clamp(0.0, 1.0));
    }

    pub fn hover(&self) -> Option<f32> {
        self.hover
    }

    /// Remembers `level` as the one a click on the speaker brings back to a silent video.
    pub fn note_level(&mut self, level: f64) {
        if volume::is_audible(level) {
            self.last_audible = Some(level);
        }
    }

    pub fn last_audible(&self) -> Option<f64> {
        self.last_audible
    }

    /// The bar went away (its host does not show it): a drag or hover that was on it is over.
    pub fn release(&mut self) {
        self.drag = None;
        self.volume_drag = false;
        self.hover = None;
        self.ab.release();
        self.reveal.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_millisecond_switch_belongs_to_one_medium() {
        let mut state = VideoBarState::default();
        assert!(!state.ms_shown(7, false));
        state.toggle_ms(7, false);
        assert!(state.ms_shown(7, false));
        assert!(!state.ms_shown(8, false), "a new medium starts as the settings say");
        state.toggle_ms(8, true);
        assert!(!state.ms_shown(8, true));
        assert!(
            state.ms_shown(7, true),
            "the switch of medium 7 is gone, so the setting counts again"
        );
    }

    #[test]
    fn a_seek_shows_its_target_until_mpv_arrives_or_gives_up() {
        let t0 = Instant::now();
        let mut state = VideoBarState::default();
        state.seek_to(0.5, 60.0, t0, false, Some(true));
        assert!(state.dragging());
        assert!((state.shown_position(10.0, t0) - 60.0).abs() < f64::EPSILON);
        state.seek_to(0.6, 72.0, t0, true, Some(true));
        assert!(!state.dragging());
        assert!(
            (state.shown_position(10.0, t0) - 72.0).abs() < f64::EPSILON,
            "mpv is not there yet"
        );
        assert!((state.shown_position(72.1, t0) - 72.1).abs() < f64::EPSILON, "arrived");
        assert!(
            (state.shown_position(11.0, t0) - 11.0).abs() < f64::EPSILON,
            "nothing pending any more"
        );
        state.seek_to(0.9, 90.0, t0, true, Some(true));
        assert!(
            (state.shown_position(11.0, t0 + SEEK_SETTLE) - 11.0).abs() < f64::EPSILON,
            "gave up"
        );
    }

    #[test]
    fn a_keyframe_seek_arrives_near_its_target_and_a_release_without_seek_keeps_the_last_one() {
        let t0 = Instant::now();
        let mut state = VideoBarState::default();
        state.seek_to(0.5, 60.0, t0, false, Some(false));
        state.seek_to(0.5, 60.0, t0, true, None);
        assert!(!state.dragging());
        assert!((state.shown_position(30.0, t0) - 60.0).abs() < f64::EPSILON);
        assert!(
            (state.shown_position(56.0, t0) - 56.0).abs() < f64::EPSILON,
            "the keyframe before the target counts"
        );
    }

    #[test]
    fn only_audible_levels_are_remembered() {
        let mut state = VideoBarState::default();
        state.note_level(0.0);
        assert_eq!(state.last_audible(), None);
        state.note_level(40.0);
        state.note_level(0.0);
        assert_eq!(state.last_audible(), Some(40.0));
    }
}
