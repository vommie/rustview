//! What a control bar shows and what its clicks mean (pure data between the instance and the canvas that draws the bar).

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
    /// Physical pixels relative to the host's rectangle (the canvas).
    pub placement: VideoBarPlacement,
    /// Physical pixels per logical pixel of the host.
    pub scale: f32,
    pub phase: Phase,
    /// The small bar of a tab grid item (play/pause and the progress bar only).
    pub mini: bool,
    pub view: VideoBarView,
}

/// The phases of a drag on the progress bar or the volume slider (the codes the canvas sends).
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

/// What the canvas reports about a bar (`VideoBarEvent` of `ui/multiview_video_bar.slint`, which lists the codes).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoBarEvent {
    pub kind: i32,
    pub share: f32,
    pub phase: i32,
    pub index: i32,
    pub text: String,
    pub delta: f32,
}

impl VideoBarInput {
    /// The input an event of the canvas stands for (`None`: an unknown code).
    pub fn from_event(event: VideoBarEvent) -> Option<Self> {
        let VideoBarEvent {
            kind,
            share,
            phase,
            index,
            text,
            delta,
        } = event;
        let at = usize::try_from(index).ok();
        let ab = |input: AbInput| Some(Self::Ab(input));
        match kind {
            0 => Some(Self::PlayPause),
            1 => Some(Self::ToggleMs),
            2 => Some(Self::Speaker),
            3 => DragPhase::from_code(phase).map(|phase| Self::Seek { share, phase }),
            4 => Some(Self::Hover {
                share: (index == 1).then_some(share),
            }),
            5 => DragPhase::from_code(phase).map(|phase| Self::Volume { share, phase }),
            6 => Some(Self::Wheel { delta_y: delta }),
            10 => AbButton::from_code(index).map(|button| Self::Ab(AbInput::Button(button))),
            11 => ab(AbInput::SaveText(text)),
            12 => ab(AbInput::SaveCommit(text)),
            13 => ab(AbInput::ClosePopup),
            14 => ab(AbInput::MenuQuery(text)),
            15 => ab(AbInput::MenuPage(index)),
            16 => at.map(|i| Self::Ab(AbInput::MenuPick(i))),
            17 => at.map(|i| Self::Ab(AbInput::MenuDelete(i))),
            18 => ab(AbInput::MenuDeleteAll),
            19 => at.map(|i| Self::Ab(AbInput::MenuRename(i))),
            20 => ab(AbInput::MenuRenameText(text)),
            21 => ab(AbInput::MenuRenameCommit(text)),
            22 => ab(AbInput::MenuRenameCancel),
            23 => ab(AbInput::StripHover((index == 1).then_some(share))),
            24 => ab(AbInput::StripClick(share)),
            25 => {
                let point = if index == 0 { AbPoint::A } else { AbPoint::B };
                DragPhase::from_code(phase).map(|phase| Self::Ab(AbInput::Flag { point, share, phase }))
            }
            _ => None,
        }
    }
}

impl AbButton {
    /// The codes of the canvas: 0 = A, 1 = B, 2 = AB clicked, 3 = AB held, 4 = Save, 5 = Load.
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::SetA),
            1 => Some(Self::SetB),
            2 => Some(Self::Loop),
            3 => Some(Self::Clear),
            4 => Some(Self::Save),
            5 => Some(Self::Load),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: i32) -> VideoBarEvent {
        VideoBarEvent {
            kind,
            ..VideoBarEvent::default()
        }
    }

    #[test]
    fn the_events_of_the_canvas_become_inputs() {
        assert_eq!(VideoBarInput::from_event(event(0)), Some(VideoBarInput::PlayPause));
        let seek = VideoBarEvent {
            share: 0.25,
            phase: 1,
            ..event(3)
        };
        assert_eq!(
            VideoBarInput::from_event(seek),
            Some(VideoBarInput::Seek {
                share: 0.25,
                phase: DragPhase::Move
            })
        );
        let left = VideoBarEvent { share: 0.5, ..event(4) };
        assert_eq!(
            VideoBarInput::from_event(left),
            Some(VideoBarInput::Hover { share: None })
        );
        let wheel = VideoBarEvent {
            delta: -120.0,
            ..event(6)
        };
        assert_eq!(
            VideoBarInput::from_event(wheel),
            Some(VideoBarInput::Wheel { delta_y: -120.0 })
        );
        let load = VideoBarEvent { index: 5, ..event(10) };
        assert_eq!(
            VideoBarInput::from_event(load),
            Some(VideoBarInput::Ab(AbInput::Button(AbButton::Load)))
        );
        let save = VideoBarEvent {
            text: "Intro".into(),
            ..event(12)
        };
        assert_eq!(
            VideoBarInput::from_event(save),
            Some(VideoBarInput::Ab(AbInput::SaveCommit("Intro".into())))
        );
        let flag = VideoBarEvent {
            index: 1,
            share: 0.75,
            phase: 2,
            ..event(25)
        };
        assert_eq!(
            VideoBarInput::from_event(flag),
            Some(VideoBarInput::Ab(AbInput::Flag {
                point: AbPoint::B,
                share: 0.75,
                phase: DragPhase::Release
            }))
        );
    }

    #[test]
    fn unknown_codes_and_negative_rows_are_dropped() {
        assert_eq!(VideoBarInput::from_event(event(99)), None);
        assert_eq!(
            VideoBarInput::from_event(VideoBarEvent { index: -1, ..event(16) }),
            None
        );
        assert_eq!(VideoBarInput::from_event(VideoBarEvent { phase: 7, ..event(5) }), None);
        assert_eq!(VideoBarInput::from_event(VideoBarEvent { index: 9, ..event(10) }), None);
    }
}
