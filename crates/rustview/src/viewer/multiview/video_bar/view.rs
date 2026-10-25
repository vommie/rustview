//! What the window of a control bar shows and what its clicks mean (pure data between the instance and the window).

use super::ab_state::AbPoint;
use super::ab_view::AbBarView;
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
    /// The A-B repeat group (only while the mpv script `ab_repeat.lua` is active in the player).
    pub ab: Option<AbBarView>,
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

/// A button of the A-B repeat group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbButton {
    SetA,
    SetB,
    /// A click on AB: the loop of a valid A-B on or off.
    Loop,
    /// AB held: A and B are cleared.
    Clear,
    /// The save bubble opens or closes.
    Save,
    /// The load menu opens or closes.
    Load,
}

/// What the user did in the A-B repeat group.
#[derive(Debug, Clone, PartialEq)]
pub enum AbInput {
    Button(AbButton),
    /// The text of the save bubble changed.
    SaveText(String),
    /// Enter in the save bubble or its button: saved with this name (empty: the script's default name).
    SaveCommit(String),
    /// Esc, a click outside or a second click on the opener.
    ClosePopup,
    MenuQuery(String),
    /// One page back (-1) or on (+1).
    MenuPage(i32),
    /// An entry of the menu (its position in the saved list).
    MenuPick(usize),
    MenuDelete(usize),
    MenuDeleteAll,
    MenuRename(usize),
    MenuRenameText(String),
    MenuRenameCommit(String),
    MenuRenameCancel,
    /// The pointer over the strip of saved repeats (`None`: it left).
    StripHover(Option<f32>),
    StripClick(f32),
    /// The flag of A or B is pressed, dragged or let go at a share of the progress bar.
    Flag {
        point: AbPoint,
        share: f32,
        phase: DragPhase,
    },
}

/// What the user did on a bar.
#[derive(Debug, Clone, PartialEq)]
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
    Ab(AbInput),
}

impl AbInput {
    /// A press of the user (it selects the tile of the bar, like a click on its media bar).
    pub const fn is_press(&self) -> bool {
        matches!(
            self,
            Self::Button(_)
                | Self::MenuPick(_)
                | Self::MenuDelete(_)
                | Self::MenuDeleteAll
                | Self::MenuRename(_)
                | Self::StripClick(_)
                | Self::Flag {
                    phase: DragPhase::Press,
                    ..
                }
        )
    }
}

impl VideoBarInput {
    /// The kind of input, for the log.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::PlayPause => "play/pause",
            Self::ToggleMs => "milliseconds",
            Self::Seek { .. } => "seek",
            Self::Hover { .. } => "hover",
            Self::Speaker => "speaker",
            Self::Volume { .. } => "volume",
            Self::Wheel { .. } => "wheel",
            Self::Ab(_) => "A-B repeat",
        }
    }
}
