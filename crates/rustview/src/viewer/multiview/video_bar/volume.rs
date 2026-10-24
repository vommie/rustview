//! The volume control of the bar (pure): a speaker button and a volume slider.
//!
//! Mute is a state of its own, as in mpv: the slider sets the level and never touches mute, so a muted video stays muted
//! while its level is raised (the slider shows the level dimmed). The speaker shows "muted" when the video is muted or its
//! level is 0. A click on it unmutes a muted video and mutes an audible one; a video at level 0 gets its last audible level
//! back instead (a muted one at level 0 both), so the click always makes it audible again.

use rustview_core::util::cast::Az;

/// The level a video at 0 gets back when nothing louder is known.
pub const FALLBACK_LEVEL: f64 = 50.0;

/// The highest level of the slider (mpv goes louder with the keys; the slider shows such a level as full).
pub const SLIDER_MAX: f64 = 100.0;

/// A level at or below this counts as silent.
const SILENT_BELOW: f64 = 0.5;

/// The icon of the speaker button. The code is what the window draws by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeIcon {
    Muted,
    Low,
    High,
}

impl VolumeIcon {
    pub fn of(level: f64, muted: bool) -> Self {
        if muted || level <= SILENT_BELOW {
            Self::Muted
        } else if level < SLIDER_MAX / 2.0 {
            Self::Low
        } else {
            Self::High
        }
    }

    pub const fn code(self) -> i32 {
        match self {
            Self::Muted => 0,
            Self::Low => 1,
            Self::High => 2,
        }
    }
}

/// What a click on the speaker does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerClick {
    /// The mute state changes (the user's own choice for the sound rules of the Multiview).
    pub toggle_mute: bool,
    /// The level the video gets (a silent one gets its last audible level back).
    pub level: Option<f64>,
}

/// The click on the speaker of a video at `level` (percent), `muted` or not, whose last audible level was `last_audible`.
pub fn speaker_click(level: f64, muted: bool, last_audible: Option<f64>) -> SpeakerClick {
    let silent = level <= SILENT_BELOW;
    SpeakerClick {
        toggle_mute: muted || !silent,
        level: silent.then(|| last_audible.filter(|l| *l > SILENT_BELOW).unwrap_or(FALLBACK_LEVEL)),
    }
}

/// Where the knob of the slider sits for `level`.
pub fn slider_share(level: f64) -> f32 {
    if level.is_finite() {
        (level / SLIDER_MAX).clamp(0.0, 1.0).az::<f32>()
    } else {
        0.0
    }
}

/// The level at `share` of the slider (whole percent).
pub fn level_at(share: f32) -> f64 {
    (f64::from(share.clamp(0.0, 1.0)) * SLIDER_MAX).round()
}

/// Whether `level` is worth remembering as the level to come back to.
pub fn is_audible(level: f64) -> bool {
    level > SILENT_BELOW
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speaker_shows_muted_for_mute_and_for_level_zero() {
        assert_eq!(VolumeIcon::of(80.0, true), VolumeIcon::Muted);
        assert_eq!(VolumeIcon::of(0.0, false), VolumeIcon::Muted);
        assert_eq!(VolumeIcon::of(30.0, false), VolumeIcon::Low);
        assert_eq!(VolumeIcon::of(100.0, false), VolumeIcon::High);
        assert_eq!(VolumeIcon::of(130.0, false), VolumeIcon::High);
    }

    #[test]
    fn a_click_unmutes_mutes_or_brings_the_level_back() {
        assert_eq!(
            speaker_click(70.0, false, None),
            SpeakerClick {
                toggle_mute: true,
                level: None
            },
            "audible: mute"
        );
        assert_eq!(
            speaker_click(70.0, true, None),
            SpeakerClick {
                toggle_mute: true,
                level: None
            },
            "muted: unmute, the level stays"
        );
        assert_eq!(
            speaker_click(0.0, false, Some(35.0)),
            SpeakerClick {
                toggle_mute: false,
                level: Some(35.0)
            },
            "silent: the last level comes back, mute stays off"
        );
        assert_eq!(
            speaker_click(0.0, true, None),
            SpeakerClick {
                toggle_mute: true,
                level: Some(FALLBACK_LEVEL)
            },
            "muted and silent: both, so it is audible afterwards"
        );
    }

    #[test]
    fn slider_shares_and_levels() {
        assert!((slider_share(50.0) - 0.5).abs() < 1e-6);
        assert!((slider_share(130.0) - 1.0).abs() < 1e-6);
        assert!(slider_share(f64::NAN).abs() < f32::EPSILON);
        assert!((level_at(0.333) - 33.0).abs() < f64::EPSILON);
        assert!((level_at(2.0) - 100.0).abs() < f64::EPSILON);
        assert!(!is_audible(0.2));
    }
}
