//! The keys of a Multiview that act on one medium (pure): what each of them asks for.
//!
//! Every key source (the fullscreen window, the main window for the tab view, the windows of the videos) turns its key into a
//! `MultiviewCommand`; the instance resolves the target, which is always the selected tile (`focus`), and refuses with a reason
//! the hint shows (`instance_commands`). Esc is not a command: it cancels and closes, whatever is selected. The hotkey system of
//! a later milestone only replaces the keys that lead here (`docs/multiview_focus.md`).

use super::media_actions::MediaActionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomKey {
    /// Ctrl+Backspace: back to the fitted picture.
    Reset,
    /// Ctrl++.
    In,
    /// Ctrl+-.
    Out,
}

impl ZoomKey {
    /// The code the windows report (`zoom-key`): 0 = reset, 1 = in, 2 = out.
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Reset),
            1 => Some(Self::In),
            2 => Some(Self::Out),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiviewCommand {
    /// Ctrl+0..5 (in the tab view also 0..5).
    Rate(u8),
    /// F2.
    Rename,
    /// DEL.
    Delete,
    Zoom(ZoomKey),
    /// Enter: the crop tool of the selected picture is applied.
    ApplyCrop,
    /// Ctrl+S.
    Save,
    /// Ctrl+Shift+S.
    SaveAs,
    /// Ctrl+Z.
    Undo,
    /// Ctrl+Shift+Z or Ctrl+Y.
    Redo,
    /// F5 in the tab view: the medium is read from the drive again.
    Refresh,
    /// A button of the media bar of the selected medium, by key (Settings > Hotkeys; no key by default).
    Bar(MediaActionId),
    /// A key of the video player on the selected video (mpv takes no keys itself).
    Player(PlayerCommand),
}

/// What a video player key asks the selected video for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCommand {
    PlayPause,
    Mute,
    /// Settings > Multiview > Jump Keys says how far (`long`: the long jump).
    Jump {
        forward: bool,
        long: bool,
    },
    Frame {
        forward: bool,
    },
    Volume {
        louder: bool,
    },
    Speed {
        faster: bool,
    },
    /// A key of the area "MPV `ab_repeat.lua`" (only while the script is active in the selected video's player).
    Ab(AbKey),
}

/// The keys of the mpv script `ab_repeat.lua` (Settings > Hotkeys > MPV `ab_repeat.lua`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbKey {
    SetA,
    SetB,
    Clear,
    /// Opens the save bubble of the control bar.
    Save,
    /// Opens the load menu of the control bar.
    LoadMenu,
    JumpToA,
    NextRange,
    PreviousRange,
}

impl MultiviewCommand {
    /// The command of a key of the image editor as the windows report it (`edit-key`): 1 = Enter, 2 = Ctrl+S, 3 = Ctrl+Shift+S,
    /// 4 = Ctrl+Z, 5 = Ctrl+Shift+Z / Ctrl+Y. Code 0 is Esc, which is no command.
    pub const fn from_edit_code(code: i32) -> Option<Self> {
        match code {
            1 => Some(Self::ApplyCrop),
            2 => Some(Self::Save),
            3 => Some(Self::SaveAs),
            4 => Some(Self::Undo),
            5 => Some(Self::Redo),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_codes_map_to_commands_and_esc_is_none() {
        assert_eq!(MultiviewCommand::from_edit_code(0), None);
        assert_eq!(MultiviewCommand::from_edit_code(1), Some(MultiviewCommand::ApplyCrop));
        assert_eq!(MultiviewCommand::from_edit_code(3), Some(MultiviewCommand::SaveAs));
        assert_eq!(MultiviewCommand::from_edit_code(5), Some(MultiviewCommand::Redo));
        assert_eq!(MultiviewCommand::from_edit_code(6), None);
    }

    #[test]
    fn the_zoom_codes_map_to_keys() {
        assert_eq!(ZoomKey::from_code(0), Some(ZoomKey::Reset));
        assert_eq!(ZoomKey::from_code(2), Some(ZoomKey::Out));
        assert_eq!(ZoomKey::from_code(3), None);
    }
}
