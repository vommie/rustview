//! What the window of a control bar shows and what its clicks mean (pure data between the instance and the window).

use super::placement::VideoBarPlacement;
use super::progress::SpanShare;
use super::reveal::Phase;
use super::time_text::TimeTexts;
use super::volume::VolumeIcon;
use crate::viewer::multiview::follow::TileKey;

/// The content of a bar.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoBarView {
    pub playing: bool,
    pub times: TimeTexts,
    /// The position as a share of the progress bar.
    pub fraction: f32,
    pub spans: Vec<SpanShare>,
    pub volume_share: f32,
    pub volume_icon: VolumeIcon,
    pub muted: bool,
    /// The pointer over the progress bar: where (share of the bar) and the time there.
    pub hover: Option<(f32, String)>,
}

/// A bar a host shows (or lets fade out).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoBarTarget {
    pub key: TileKey,
    pub placement: VideoBarPlacement,
    pub phase: Phase,
    /// A drag on the bar runs: it stays even while the pointer is over another window.
    pub held: bool,
    /// The small bar of a tab grid item (play/pause and the progress bar only).
    pub mini: bool,
    pub view: VideoBarView,
}

/// The phases of a drag on the progress bar or the volume slider (the codes the window sends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragPhase {
    Press,
    Move,
    Release,
}

impl DragPhase {
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Press),
            1 => Some(Self::Move),
            2 => Some(Self::Release),
            _ => None,
        }
    }
}

/// What the user did on a bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoBarInput {
    PlayPause,
    /// A click on a time: milliseconds on or off.
    ToggleMs,
    /// The progress bar is pressed, dragged or let go at `share` of its width.
    Seek {
        share: f32,
        phase: DragPhase,
    },
    /// The pointer moved over the progress bar (`None`: it left).
    Hover {
        share: Option<f32>,
    },
    /// A click on the speaker.
    Speaker,
    /// The volume slider is pressed, dragged or let go at `share` of its width.
    Volume {
        share: f32,
        phase: DragPhase,
    },
    /// The mouse wheel over the bar (`delta_y` as the window reports it): louder or quieter.
    Wheel {
        delta_y: f32,
    },
}
