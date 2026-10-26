//! The on-screen feedback of an interaction with a medium of a Multiview (pure, the clock is passed in): the one animation every
//! such feedback uses, so they all look and time alike. Today: a rating given with the keyboard, a picture that was saved, a
//! video muted or unmuted with its key, and the volume bar of a video (`volume_feedback`, which uses the same timing).
//!
//! A tile gets a pulse: an icon in its middle ([`FeedbackIcon`], [`ICON_EDGE`] logical pixels), full strength for [`HOLD`], then
//! fading out for [`FADE`] (one second in all). A new pulse cancels the running one of the tile at once: the new icon starts at
//! full strength, with no cross-fade and no queue. Nothing here keeps time by itself: the host asks `wakeup` how long it may sleep
//! and arms a single-shot timer only while a pulse is visible. The canvas of the host draws the icon over every tile, picture or
//! video (`TileView::feedback`).

use super::follow::TileKey;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long the icon stays at full strength.
pub const HOLD: Duration = Duration::from_millis(750);
/// How long the icon takes to fade out after the hold.
pub const FADE: Duration = Duration::from_millis(250);
/// The edge of the icon in logical pixels.
pub const ICON_EDGE: u32 = 50;
/// Distance between two frames of the fade.
pub const FRAME: Duration = Duration::from_millis(16);

/// The icon of a pulse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeedbackIcon {
    /// The medium was rated (0 = no rating).
    Rating(u8),
    /// The picture was saved.
    Saved,
    /// A video was muted (crossed-out speaker) or unmuted (speaker) with the key.
    Sound { muted: bool },
}

impl FeedbackIcon {
    /// The number the canvas gets (`feedback-icon` of `MultiviewTileView`): 0..5 a rating, [`SAVED_CODE`] the save icon.
    pub fn code(self) -> i32 {
        match self {
            Self::Rating(rating) => i32::from(rating.min(5)),
            Self::Saved => SAVED_CODE,
            Self::Sound { muted: false } => SOUND_ON_CODE,
            Self::Sound { muted: true } => SOUND_OFF_CODE,
        }
    }
}

/// `FeedbackIcon::Saved` for the canvas.
pub const SAVED_CODE: i32 = 6;
/// `FeedbackIcon::Sound` (only ever on a video tile).
pub const SOUND_ON_CODE: i32 = 7;
pub const SOUND_OFF_CODE: i32 = 8;

/// The strength of a pulse that started at `started`: 1.0 during [`HOLD`], then fading to 0.0 over [`FADE`]; `None` once over.
pub fn pulse_alpha(started: Instant, now: Instant) -> Option<f32> {
    let age = now.saturating_duration_since(started);
    if age < HOLD {
        return Some(1.0);
    }
    let faded = age.saturating_sub(HOLD);
    if faded >= FADE {
        return None;
    }
    Some(1.0 - faded.as_secs_f32() / FADE.as_secs_f32())
}

/// How long until a pulse that started at `started` changes on screen: the rest of the hold, a frame during the fade, zero
/// once it is over (it has to be dropped now).
pub fn pulse_wakeup(started: Instant, now: Instant) -> Duration {
    let age = now.saturating_duration_since(started);
    if age < HOLD {
        HOLD.saturating_sub(age)
    } else if age < HOLD + FADE {
        FRAME
    } else {
        Duration::ZERO
    }
}

/// What is drawn on one tile right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PulseView {
    pub icon: FeedbackIcon,
    /// 1.0 = full strength, 0.0 = gone. Never 0.0 for a pulse that is still shown.
    pub alpha: f32,
}

#[derive(Debug, Clone, Copy)]
struct Pulse {
    icon: FeedbackIcon,
    started: Instant,
}

impl Pulse {
    fn alpha(&self, now: Instant) -> Option<f32> {
        pulse_alpha(self.started, now)
    }
}

/// The pulses of one Multiview, by tile.
#[derive(Debug, Default)]
pub struct MediaFeedback {
    pulses: HashMap<TileKey, Pulse>,
}

impl MediaFeedback {
    /// Starts a pulse with `icon` on every tile of `tiles`, replacing the one that runs there.
    pub fn trigger(&mut self, tiles: impl IntoIterator<Item = TileKey>, icon: FeedbackIcon, now: Instant) {
        for tile in tiles {
            self.pulses.insert(tile, Pulse { icon, started: now });
        }
    }

    /// What tile `tile` shows at `now`.
    pub fn view(&self, tile: TileKey, now: Instant) -> Option<PulseView> {
        let pulse = self.pulses.get(&tile)?;
        pulse.alpha(now).map(|alpha| PulseView {
            icon: pulse.icon,
            alpha,
        })
    }

    /// Drops the pulses that are over. Returns whether any was.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.pulses.len();
        self.pulses.retain(|_, pulse| pulse.alpha(now).is_some());
        self.pulses.len() != before
    }

    /// Forgets the pulses of tiles that are gone.
    pub fn retain_tiles(&mut self, keep: impl Fn(TileKey) -> bool) {
        self.pulses.retain(|tile, _| keep(*tile));
    }

    /// Ends every pulse (the Multiview is not shown any more).
    pub fn clear(&mut self) {
        self.pulses.clear();
    }

    pub fn is_active(&self) -> bool {
        !self.pulses.is_empty()
    }

    /// The tiles that have a pulse (shown or about to be dropped).
    pub fn tiles(&self) -> impl Iterator<Item = TileKey> + '_ {
        self.pulses.keys().copied()
    }

    /// How long until something changes on screen, `None` when no pulse is shown. While every pulse still holds that is the
    /// time until the first fade starts; during a fade it is the frame distance.
    pub fn wakeup(&self, now: Instant) -> Option<Duration> {
        self.pulses.values().map(|pulse| pulse_wakeup(pulse.started, now)).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn full_strength_during_the_hold_then_a_linear_fade() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        fb.trigger([1], FeedbackIcon::Rating(3), t0);
        assert_eq!(
            fb.view(1, t0),
            Some(PulseView {
                icon: FeedbackIcon::Rating(3),
                alpha: 1.0
            })
        );
        assert_eq!(fb.view(1, t0 + ms(749)).map(|v| v.alpha), Some(1.0));
        let mid = fb.view(1, t0 + ms(875)).expect("fading").alpha;
        assert!((mid - 0.5).abs() < 1e-3, "{mid}");
        let late = fb.view(1, t0 + ms(999)).expect("still shown").alpha;
        assert!(late > 0.0 && late < 0.01, "{late}");
        assert_eq!(fb.view(1, t0 + ms(1000)), None);
        assert_eq!(fb.view(1, t0 + ms(5000)), None);
    }

    #[test]
    fn the_sound_icons_have_codes_of_their_own() {
        let codes = [
            FeedbackIcon::Rating(5).code(),
            FeedbackIcon::Saved.code(),
            FeedbackIcon::Sound { muted: false }.code(),
            FeedbackIcon::Sound { muted: true }.code(),
        ];
        assert_eq!(codes, [5, SAVED_CODE, SOUND_ON_CODE, SOUND_OFF_CODE]);
    }

    #[test]
    fn the_total_time_is_one_second_and_the_fade_is_the_last_quarter() {
        assert_eq!(HOLD + FADE, ms(1000));
        assert_eq!(FADE, ms(250));
    }

    #[test]
    fn a_new_rating_cancels_the_running_one_at_once() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        fb.trigger([7], FeedbackIcon::Rating(2), t0);
        let t1 = t0 + ms(900); // in the middle of the fade
        assert!(fb.view(7, t1).expect("fading").alpha < 0.7);
        fb.trigger([7], FeedbackIcon::Rating(5), t1);
        assert_eq!(
            fb.view(7, t1),
            Some(PulseView {
                icon: FeedbackIcon::Rating(5),
                alpha: 1.0
            })
        );
        // the clock of the new pulse runs from t1, not from t0
        assert_eq!(fb.view(7, t1 + ms(700)).map(|v| v.alpha), Some(1.0));
        assert_eq!(fb.view(7, t1 + ms(1000)), None);
    }

    #[test]
    fn the_same_rating_again_restarts_the_animation() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        fb.trigger([1], FeedbackIcon::Rating(4), t0);
        fb.trigger([1], FeedbackIcon::Rating(4), t0 + ms(950));
        assert_eq!(
            fb.view(1, t0 + ms(950)),
            Some(PulseView {
                icon: FeedbackIcon::Rating(4),
                alpha: 1.0
            })
        );
    }

    #[test]
    fn rapid_sequence_shows_only_the_last_icon() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        for (i, rating) in [1u8, 4, 0, 5].into_iter().enumerate() {
            fb.trigger([1, 2], FeedbackIcon::Rating(rating), t0 + ms(50 * i as u64));
        }
        let now = t0 + ms(150);
        for tile in [1, 2] {
            assert_eq!(
                fb.view(tile, now),
                Some(PulseView {
                    icon: FeedbackIcon::Rating(5),
                    alpha: 1.0
                })
            );
        }
    }

    #[test]
    fn tiles_run_independently() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        fb.trigger([1], FeedbackIcon::Rating(3), t0);
        fb.trigger([2], FeedbackIcon::Rating(1), t0 + ms(600));
        let now = t0 + ms(900);
        assert!(fb.view(1, now).expect("fading").alpha < 1.0);
        assert_eq!(fb.view(2, now).map(|v| v.alpha), Some(1.0));
        assert_eq!(fb.view(3, now), None);
    }

    #[test]
    fn expire_drops_only_finished_pulses() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        fb.trigger([1], FeedbackIcon::Rating(3), t0);
        fb.trigger([2], FeedbackIcon::Rating(3), t0 + ms(500));
        assert!(!fb.expire(t0 + ms(900)));
        assert!(fb.expire(t0 + ms(1100)));
        assert_eq!(fb.tiles().collect::<Vec<_>>(), vec![2]);
        assert!(fb.expire(t0 + ms(1600)));
        assert!(!fb.is_active());
        assert!(!fb.expire(t0 + ms(2000)));
    }

    #[test]
    fn wakeup_sleeps_through_the_hold_and_ticks_during_the_fade() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        assert_eq!(fb.wakeup(t0), None);
        fb.trigger([1], FeedbackIcon::Rating(3), t0);
        assert_eq!(fb.wakeup(t0), Some(HOLD));
        assert_eq!(fb.wakeup(t0 + ms(250)), Some(ms(500)));
        assert_eq!(fb.wakeup(t0 + ms(800)), Some(FRAME));
        assert_eq!(fb.wakeup(t0 + ms(1000)), Some(Duration::ZERO));
        fb.expire(t0 + ms(1000));
        assert_eq!(fb.wakeup(t0 + ms(1000)), None);
    }

    #[test]
    fn wakeup_follows_the_tile_that_changes_first() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        fb.trigger([1], FeedbackIcon::Rating(3), t0);
        fb.trigger([2], FeedbackIcon::Rating(3), t0 + ms(700));
        // tile 1 starts to fade in 50 ms, tile 2 holds for 750 ms more
        assert_eq!(fb.wakeup(t0 + ms(700)), Some(ms(50)));
    }

    #[test]
    fn tiles_that_left_lose_their_pulse() {
        let t0 = Instant::now();
        let mut fb = MediaFeedback::default();
        fb.trigger([1, 2, 3], FeedbackIcon::Rating(3), t0);
        fb.retain_tiles(|tile| tile != 2);
        let mut left: Vec<_> = fb.tiles().collect();
        left.sort_unstable();
        assert_eq!(left, vec![1, 3]);
        fb.clear();
        assert!(!fb.is_active());
    }
}
