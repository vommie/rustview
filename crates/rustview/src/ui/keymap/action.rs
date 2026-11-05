//! The actions a key can trigger: their ids in `settings.json`, areas, titles and default keys (today's keys plus the new
//! ones the user chose on 2026-10-24, `docs/hotkeys.md`). The order is the order of the Settings page and decides which
//! binding wins when a stored key collides with a default.

use super::area::Area;
use crate::ui::tabs::grid::mark::{GridKey, Step};
use crate::viewer::multiview::command::{AbKey, PlayerCommand};
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::primary_actions::PrimaryActionId;
use crate::viewer::multiview::video_filter::FilterChange;

/// The keys cannot be changed (Esc, the cursor keys of Browse, type-ahead find); the page shows them with a lock.
pub const FIXED: u8 = 1;
/// The action also works while a text field has the keyboard.
pub const TYPING: u8 = 2;
/// A held key repeats the action (steps); toggles and edits act once per press.
pub const REPEAT: u8 = 4;
/// The keys are labels for the page, not key combinations (type-ahead find: "A-Z").
pub const LABELS: u8 = 8;
/// The action pages through media and may share keys with the video player (paging only works for an image).
pub const BESIDE_PLAYER: u8 = 16;

/// What the keymap knows about an action.
#[derive(Debug)]
pub struct Spec {
    pub area: Area,
    /// The key in the `hotkeys` table of `settings.json`.
    pub id: &'static str,
    pub title: &'static str,
    /// The keys a fresh profile has, in Slint key names (`Control+S`, `Home`, `Plus`).
    pub keys: &'static [&'static str],
    pub flags: u8,
}

macro_rules! actions {
    ($($variant:ident: $area:ident, $id:literal, $title:literal, [$($key:literal),*] $(, $flag:ident)*;)*) => {
        /// Everything the keymap can bind.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action {
            $($variant),*
        }

        impl Action {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            pub const fn spec(self) -> &'static Spec {
                match self {
                    $(Self::$variant => &Spec {
                        area: Area::$area,
                        id: $id,
                        title: $title,
                        keys: &[$($key),*],
                        flags: 0 $(| $flag)*,
                    }),*
                }
            }
        }
    };
}

actions! {
    NextTab: General, "general.next_tab", "Next tab", ["Control+Tab", "Control+PageDown"], TYPING, REPEAT;
    PreviousTab: General, "general.previous_tab", "Previous tab", ["Control+Shift+Tab", "Control+PageUp"], TYPING, REPEAT;
    ShowBrowse: General, "general.show_browse", "Show Browse", ["Alt+Digit1"], TYPING;
    ShowTabGrid: General, "general.show_tab_grid", "Show the tab grid", ["Alt+Digit2"], TYPING;
    OpenSettings: General, "general.open_settings", "Open the settings", ["Control+Comma"], TYPING;
    ShowHotkeys: General, "general.show_hotkeys", "Show the keys (Settings, Hotkeys)", ["F1"], TYPING;

    BrowseOpen: Browse, "browse.open", "Open the selection in a tab", ["Return"];
    BrowseOpenFullscreen: Browse, "browse.open_fullscreen", "Open the selection in the fullscreen", ["F11"];
    BrowseOpenBackground: Browse, "browse.open_background_tab", "Open the selection in a background tab", ["Control+Return"];
    BrowseRename: Browse, "browse.rename", "Rename", ["F2"];
    BrowseDelete: Browse, "browse.delete", "Delete", ["Delete"];
    BrowseRefresh: Browse, "browse.refresh", "Refresh the selected files", ["F5"];
    BrowseRate5: Browse, "browse.rate_5", "Rate: 5 stars", ["Control+Digit5"];
    BrowseRate4: Browse, "browse.rate_4", "Rate: 4 stars", ["Control+Digit4"];
    BrowseRate3: Browse, "browse.rate_3", "Rate: 3 stars", ["Control+Digit3"];
    BrowseRate2: Browse, "browse.rate_2", "Rate: 2 stars", ["Control+Digit2"];
    BrowseRate1: Browse, "browse.rate_1", "Rate: 1 star", ["Control+Digit1"];
    BrowseRate0: Browse, "browse.rate_0", "Clear the rating", ["Control+Digit0"];
    BrowseSelectAll: Browse, "browse.select_all", "Select all", ["Control+A"];
    BrowseClearSelection: Browse, "browse.clear_selection", "Clear the selection", ["Control+Shift+A"];
    BrowseParent: Browse, "browse.parent_folder", "Go to the parent folder", ["Backspace", "Alt+UpArrow"];
    BrowseBack: Browse, "browse.back", "Back to the previous folder", ["Alt+LeftArrow"];
    BrowseForward: Browse, "browse.forward", "Forward to the next folder", ["Alt+RightArrow"];
    BrowseNewFolder: Browse, "browse.new_folder", "New folder", ["Control+Shift+N"];
    BrowseFilter: Browse, "browse.focus_filter", "Put the cursor into the filter field", ["Control+F", "F3"], TYPING;
    BrowseToggleTable: Browse, "browse.toggle_table", "Switch between thumbnails and the table", ["Control+T"];
    BrowseCursor: Browse, "browse.move_cursor", "Move the cursor (Shift extends the selection)",
        ["Shift?+UpArrow", "Shift?+DownArrow", "Shift?+LeftArrow", "Shift?+RightArrow"], FIXED, REPEAT;
    BrowsePage: Browse, "browse.page", "One page up or down (Shift extends the selection)",
        ["Shift?+PageUp", "Shift?+PageDown"], FIXED, REPEAT;
    BrowseFirstLast: Browse, "browse.first_last", "First or last item (Shift extends the selection)",
        ["Shift?+Home", "Shift?+End"], FIXED;
    BrowseEscape: Browse, "browse.escape", "End a search, clear the selection", ["Escape"], FIXED;
    BrowseTypeAhead: Browse, "browse.type_ahead", "Type a name to jump to it", ["A-Z", "0-9"], FIXED, LABELS;

    GridMarkLeft: Grid, "grid.mark_left", "Mark the item to the left", ["LeftArrow"], REPEAT;
    GridMarkRight: Grid, "grid.mark_right", "Mark the item to the right", ["RightArrow"], REPEAT;
    GridMarkUp: Grid, "grid.mark_up", "Mark the item above", ["UpArrow"], REPEAT;
    GridMarkDown: Grid, "grid.mark_down", "Mark the item below", ["DownArrow"], REPEAT;
    GridOpen: Grid, "grid.open", "Open the marked item", ["Return"];
    GridClose: Grid, "grid.close", "Close the tab of the marked item", ["Delete", "Control+W"];
    GridFullscreen: Grid, "grid.show_fullscreen", "Show the marked item in the fullscreen", ["F11"];

    TabRename: Tab, "tab.rename", "Rename", ["F2"];
    TabDelete: Tab, "tab.delete", "Delete", ["Delete"];
    TabRefresh: Tab, "tab.refresh", "Read the medium again from the drive", ["F5"];
    TabRate5: Tab, "tab.rate_5", "Rate: 5 stars", ["Control+Digit5"];
    TabRate4: Tab, "tab.rate_4", "Rate: 4 stars", ["Control+Digit4"];
    TabRate3: Tab, "tab.rate_3", "Rate: 3 stars", ["Control+Digit3"];
    TabRate2: Tab, "tab.rate_2", "Rate: 2 stars", ["Control+Digit2"];
    TabRate1: Tab, "tab.rate_1", "Rate: 1 star", ["Control+Digit1"];
    TabRate0: Tab, "tab.rate_0", "Clear the rating", ["Control+Digit0"];
    TabZoomIn: Tab, "tab.zoom_in", "Zoom in", ["Control+Plus", "Control+Equals"], REPEAT;
    TabZoomOut: Tab, "tab.zoom_out", "Zoom out", ["Control+HyphenMinus"], REPEAT;
    TabZoomReset: Tab, "tab.zoom_reset", "Fit the picture again (zoom reset)", ["Control+Backspace", "Backspace"];
    TabApplyCrop: Tab, "tab.apply_crop", "Apply the crop", ["Return"];
    TabSave: Tab, "tab.save", "Save the changes", ["Control+S"];
    TabSaveAs: Tab, "tab.save_as", "Save as", ["Control+Shift+S"];
    TabUndo: Tab, "tab.undo", "Undo", ["Control+Z"], REPEAT;
    TabRedo: Tab, "tab.redo", "Redo", ["Control+Shift+Z", "Control+Y"], REPEAT;
    TabFullscreen: Tab, "tab.show_fullscreen", "Show in the fullscreen", ["F11", "F"];
    TabClose: Tab, "tab.close", "Close the tab", ["Control+W"], TYPING;
    TabDuplicate: Tab, "tab.duplicate", "Media bar: show the medium a second time (Duplicate)", [];
    TabCrop: Tab, "tab.crop", "Media bar: crop tool on or off", [];
    TabRemove: Tab, "tab.remove", "Media bar: remove the medium from the view", [];
    TabMuteAll: Tab, "tab.mute_all", "Primary bar: mute or unmute all videos", [];
    TabPlayPauseAll: Tab, "tab.play_pause_all", "Primary bar: play or pause all videos", [];
    TabResetLayout: Tab, "tab.reset_layout", "Primary bar: reset the arrangement", [];
    TabHideCursor: Tab, "tab.hide_cursor", "Primary bar: hide the resting mouse cursor (on or off)", [];
    TabEscape: Tab, "tab.escape", "Cancel the crop, end the selection of tabs", ["Escape"], FIXED;

    FullscreenRename: Fullscreen, "fullscreen.rename", "Rename", ["F2"];
    FullscreenDelete: Fullscreen, "fullscreen.delete", "Delete", ["Delete"];
    FullscreenRefresh: Fullscreen, "fullscreen.refresh", "Read the medium again from the drive", ["F5"];
    FullscreenRate5: Fullscreen, "fullscreen.rate_5", "Rate: 5 stars", ["Control+Digit5"];
    FullscreenRate4: Fullscreen, "fullscreen.rate_4", "Rate: 4 stars", ["Control+Digit4"];
    FullscreenRate3: Fullscreen, "fullscreen.rate_3", "Rate: 3 stars", ["Control+Digit3"];
    FullscreenRate2: Fullscreen, "fullscreen.rate_2", "Rate: 2 stars", ["Control+Digit2"];
    FullscreenRate1: Fullscreen, "fullscreen.rate_1", "Rate: 1 star", ["Control+Digit1"];
    FullscreenRate0: Fullscreen, "fullscreen.rate_0", "Clear the rating", ["Control+Digit0"];
    FullscreenZoomIn: Fullscreen, "fullscreen.zoom_in", "Zoom in", ["Control+Plus", "Control+Equals"], REPEAT;
    FullscreenZoomOut: Fullscreen, "fullscreen.zoom_out", "Zoom out", ["Control+HyphenMinus"], REPEAT;
    FullscreenZoomReset: Fullscreen, "fullscreen.zoom_reset", "Fit the picture again (zoom reset)",
        ["Control+Backspace", "Backspace"];
    FullscreenApplyCrop: Fullscreen, "fullscreen.apply_crop", "Apply the crop", ["Return"];
    FullscreenSave: Fullscreen, "fullscreen.save", "Save the changes", ["Control+S"];
    FullscreenSaveAs: Fullscreen, "fullscreen.save_as", "Save as", ["Control+Shift+S"];
    FullscreenUndo: Fullscreen, "fullscreen.undo", "Undo", ["Control+Z"], REPEAT;
    FullscreenRedo: Fullscreen, "fullscreen.redo", "Redo", ["Control+Shift+Z", "Control+Y"], REPEAT;
    FullscreenPrevious: Fullscreen, "fullscreen.previous", "Previous medium (a view opened from Browse)",
        ["LeftArrow", "UpArrow", "PageUp"], REPEAT, BESIDE_PLAYER;
    FullscreenNext: Fullscreen, "fullscreen.next", "Next medium (a view opened from Browse)",
        ["RightArrow", "DownArrow", "PageDown"], REPEAT, BESIDE_PLAYER;
    FullscreenClose: Fullscreen, "fullscreen.close", "Close the fullscreen", ["Control+W", "F11", "F"];
    FullscreenSticky: Fullscreen, "fullscreen.sticky", "Media bar: keep the medium when the selection changes (Sticky)", [];
    FullscreenDuplicate: Fullscreen, "fullscreen.duplicate", "Media bar: show the medium a second time (Duplicate)", [];
    FullscreenOpenAsTab: Fullscreen, "fullscreen.open_as_tab", "Media bar: open the medium in a background tab", [];
    FullscreenCrop: Fullscreen, "fullscreen.crop", "Media bar: crop tool on or off", [];
    FullscreenRemove: Fullscreen, "fullscreen.remove", "Media bar: remove the medium from the view", [];
    FullscreenSendToTab: Fullscreen, "fullscreen.send_to_tab", "Primary bar: keep the view as a tab (Send to Tab)", [];
    FullscreenMuteAll: Fullscreen, "fullscreen.mute_all", "Primary bar: mute or unmute all videos", [];
    FullscreenPlayPauseAll: Fullscreen, "fullscreen.play_pause_all", "Primary bar: play or pause all videos", [];
    FullscreenResetLayout: Fullscreen, "fullscreen.reset_layout", "Primary bar: reset the arrangement", [];
    FullscreenHideCursor: Fullscreen, "fullscreen.hide_cursor", "Primary bar: hide the resting mouse cursor (on or off)", [];
    FullscreenPin: Fullscreen, "fullscreen.pin", "Primary bar: pin the fullscreen (on or off)", [];
    FullscreenEscape: Fullscreen, "fullscreen.escape", "Cancel the crop, else close the fullscreen", ["Escape"], FIXED;

    PlayerPlayPause: Player, "player.play_pause", "Play or pause", ["Space"];
    PlayerMute: Player, "player.mute", "Mute or unmute", ["M"];
    PlayerJumpBack: Player, "player.jump_back", "Jump back", ["LeftArrow"], REPEAT;
    PlayerJumpForward: Player, "player.jump_forward", "Jump forward", ["RightArrow"], REPEAT;
    PlayerJumpBackLong: Player, "player.jump_back_long", "Jump back (long step)", ["Shift+LeftArrow"], REPEAT;
    PlayerJumpForwardLong: Player, "player.jump_forward_long", "Jump forward (long step)", ["Shift+RightArrow"], REPEAT;
    PlayerFrameBack: Player, "player.frame_back", "Previous frame", ["Comma"], REPEAT;
    PlayerFrameForward: Player, "player.frame_forward", "Next frame", ["Period"], REPEAT;
    PlayerVolumeUp: Player, "player.volume_up", "Louder", ["Control+UpArrow"], REPEAT;
    PlayerVolumeDown: Player, "player.volume_down", "Quieter", ["Control+DownArrow"], REPEAT;
    PlayerSlower: Player, "player.slower", "Slower", ["OpenBracket"], REPEAT;
    PlayerFaster: Player, "player.faster", "Faster", ["CloseBracket"], REPEAT;
    PlayerRotate: Player, "player.filter_rotate", "Filters: rotate right (90, 180, 270, back to 0 degrees)", ["R"];
    PlayerRotateLeft: Player, "player.filter_rotate_left", "Filters: rotate left", [];
    PlayerMirror: Player, "player.filter_mirror", "Filters: mirror (on top of the rotation)", [];
    PlayerGamma: Player, "player.filter_gamma", "Filters: gamma up by 10 (after 100 back to 0)", [];
    PlayerDeinterlace: Player, "player.filter_deinterlace", "Filters: deinterlace (on or off)", [];
    PlayerFiltersOff: Player, "player.filters_off", "Filters: all filters off", [];
    AbSetA: AbRepeat, "ab_repeat.set_a", "Set A (the start of the repeat)", ["Home"];
    AbSetB: AbRepeat, "ab_repeat.set_b", "Set B (the end of the repeat)", ["End"];
    AbClear: AbRepeat, "ab_repeat.clear", "Clear A and B", ["Control+Delete"];
    AbSave: AbRepeat, "ab_repeat.save", "Save the repeat (the name bubble of the control bar)", [];
    AbLoadMenu: AbRepeat, "ab_repeat.load_menu", "Saved repeats (the menu of the control bar)", [];
    AbJumpToA: AbRepeat, "ab_repeat.jump_a", "Jump to A", [];
    AbNext: AbRepeat, "ab_repeat.next", "Next saved repeat", [];
    AbPrevious: AbRepeat, "ab_repeat.previous", "Previous saved repeat", [];
}

impl Action {
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.id() == id)
    }

    pub const fn id(self) -> &'static str {
        self.spec().id
    }

    pub const fn area(self) -> Area {
        self.spec().area
    }

    pub const fn title(self) -> &'static str {
        self.spec().title
    }

    pub const fn default_keys(self) -> &'static [&'static str] {
        self.spec().keys
    }

    const fn has(self, flag: u8) -> bool {
        self.spec().flags & flag != 0
    }

    /// The keys cannot be changed.
    pub const fn is_fixed(self) -> bool {
        self.has(FIXED)
    }

    /// The default keys are labels (type-ahead find), not key combinations.
    pub const fn has_label_keys(self) -> bool {
        self.has(LABELS)
    }

    pub const fn works_while_typing(self) -> bool {
        self.has(TYPING)
    }

    pub const fn repeats(self) -> bool {
        self.has(REPEAT)
    }

    /// Whether `self` and `other` may never share a key: different actions in areas that are active together. Paging in
    /// the fullscreen and the video player may share one (paging only works for an image, the player only for a video).
    pub fn excludes(self, other: Self) -> bool {
        if self == other || !self.area().overlaps(other.area()) {
            return false;
        }
        let beside = |a: Self, b: Self| a.has(BESIDE_PLAYER) && b.area() == Area::Player;
        !(beside(self, other) || beside(other, self))
    }

    /// The rating a rating action gives (0 = none).
    /// The media bar button a key action stands for.
    pub const fn media_bar(self) -> Option<MediaActionId> {
        Some(match self {
            Self::FullscreenSticky => MediaActionId::Sticky,
            Self::TabDuplicate | Self::FullscreenDuplicate => MediaActionId::Duplicate,
            Self::FullscreenOpenAsTab => MediaActionId::OpenAsTab,
            Self::TabCrop | Self::FullscreenCrop => MediaActionId::Crop,
            Self::TabRemove | Self::FullscreenRemove => MediaActionId::Remove,
            _ => return None,
        })
    }

    /// The primary bar button a key action stands for.
    pub const fn primary_bar(self) -> Option<PrimaryActionId> {
        Some(match self {
            Self::FullscreenSendToTab => PrimaryActionId::SendToTab,
            Self::TabMuteAll | Self::FullscreenMuteAll => PrimaryActionId::MuteAll,
            Self::TabPlayPauseAll | Self::FullscreenPlayPauseAll => PrimaryActionId::PlayPauseAll,
            Self::TabResetLayout | Self::FullscreenResetLayout => PrimaryActionId::ResetLayout,
            Self::TabHideCursor | Self::FullscreenHideCursor => PrimaryActionId::HideIdleCursor,
            Self::FullscreenPin => PrimaryActionId::Pin,
            _ => return None,
        })
    }

    /// The video player command a key action stands for.
    pub const fn player(self) -> Option<PlayerCommand> {
        Some(match self {
            Self::PlayerPlayPause => PlayerCommand::PlayPause,
            Self::PlayerMute => PlayerCommand::Mute,
            Self::PlayerJumpBack => PlayerCommand::Jump {
                forward: false,
                long: false,
            },
            Self::PlayerJumpForward => PlayerCommand::Jump {
                forward: true,
                long: false,
            },
            Self::PlayerJumpBackLong => PlayerCommand::Jump {
                forward: false,
                long: true,
            },
            Self::PlayerJumpForwardLong => PlayerCommand::Jump {
                forward: true,
                long: true,
            },
            Self::PlayerFrameBack => PlayerCommand::Frame { forward: false },
            Self::PlayerFrameForward => PlayerCommand::Frame { forward: true },
            Self::PlayerVolumeUp => PlayerCommand::Volume { louder: true },
            Self::PlayerVolumeDown => PlayerCommand::Volume { louder: false },
            Self::PlayerSlower => PlayerCommand::Speed { faster: false },
            Self::PlayerFaster => PlayerCommand::Speed { faster: true },
            Self::PlayerRotate => PlayerCommand::Filter(FilterChange::Rotate(90)),
            Self::PlayerRotateLeft => PlayerCommand::Filter(FilterChange::Rotate(-90)),
            Self::PlayerMirror => PlayerCommand::Filter(FilterChange::ToggleMirror),
            Self::PlayerGamma => PlayerCommand::Filter(FilterChange::CycleGamma),
            Self::PlayerDeinterlace => PlayerCommand::Filter(FilterChange::ToggleDeinterlace),
            Self::PlayerFiltersOff => PlayerCommand::Filter(FilterChange::AllOff),
            Self::AbSetA => PlayerCommand::Ab(AbKey::SetA),
            Self::AbSetB => PlayerCommand::Ab(AbKey::SetB),
            Self::AbClear => PlayerCommand::Ab(AbKey::Clear),
            Self::AbSave => PlayerCommand::Ab(AbKey::Save),
            Self::AbLoadMenu => PlayerCommand::Ab(AbKey::LoadMenu),
            Self::AbJumpToA => PlayerCommand::Ab(AbKey::JumpToA),
            Self::AbNext => PlayerCommand::Ab(AbKey::NextRange),
            Self::AbPrevious => PlayerCommand::Ab(AbKey::PreviousRange),
            _ => return None,
        })
    }

    pub const fn grid(self) -> Option<GridKey> {
        Some(match self {
            Self::GridMarkLeft => GridKey::Move(Step::Left),
            Self::GridMarkRight => GridKey::Move(Step::Right),
            Self::GridMarkUp => GridKey::Move(Step::Up),
            Self::GridMarkDown => GridKey::Move(Step::Down),
            Self::GridOpen => GridKey::Open,
            Self::GridClose => GridKey::Close,
            Self::GridFullscreen => GridKey::Fullscreen,
            _ => return None,
        })
    }

    pub const fn rating(self) -> Option<u8> {
        match self {
            Self::BrowseRate0 | Self::TabRate0 | Self::FullscreenRate0 => Some(0),
            Self::BrowseRate1 | Self::TabRate1 | Self::FullscreenRate1 => Some(1),
            Self::BrowseRate2 | Self::TabRate2 | Self::FullscreenRate2 => Some(2),
            Self::BrowseRate3 | Self::TabRate3 | Self::FullscreenRate3 => Some(3),
            Self::BrowseRate4 | Self::TabRate4 | Self::FullscreenRate4 => Some(4),
            Self::BrowseRate5 | Self::TabRate5 | Self::FullscreenRate5 => Some(5),
            _ => None,
        }
    }
}
