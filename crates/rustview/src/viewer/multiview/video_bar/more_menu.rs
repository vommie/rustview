//! The "..." menu of a narrow control bar (pure): the controls the bar has no room for, as entries of a menu with submenus.
//! Which entries there are follows the bar's step (`collapse::BarFit`): the sound once the speaker left the bar, the A-B
//! repeat (a submenu) once its button did, the times once the current time did. A new kind of entry (the video filters of a
//! later version) is one more submenu here. Every entry does what its control in the bar does (`MoreAction::input`).

use super::ab_view::{AbBarView, LoopLook};
use super::collapse::BarFit;
use super::popups::MoreSub;
use super::time_text::TimeTexts;
use super::view::{AbButton, AbInput, VideoBarInput};
use super::volume::VolumeIcon;
use crate::icons::Icon;

/// What an entry does. Its code is the id the canvas reports back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoreAction {
    /// The times are shown with milliseconds or without.
    Milliseconds,
    /// The vertical volume bar opens above "...".
    Volume,
    Mute,
    AbSubmenu,
    AbSetA,
    AbSetB,
    AbLoop,
    AbClear,
    AbSave,
    AbLoad,
}

impl MoreAction {
    const ALL: [Self; 10] = [
        Self::Milliseconds,
        Self::Volume,
        Self::Mute,
        Self::AbSubmenu,
        Self::AbSetA,
        Self::AbSetB,
        Self::AbLoop,
        Self::AbClear,
        Self::AbSave,
        Self::AbLoad,
    ];

    pub fn code(self) -> i32 {
        Self::ALL
            .iter()
            .position(|&action| action == self)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(-1)
    }

    pub fn from_code(code: i32) -> Option<Self> {
        usize::try_from(code).ok().and_then(|i| Self::ALL.get(i).copied())
    }

    /// The input of the bar the entry stands for (`None`: it opens a popup or a submenu, which the bar's state does).
    pub fn input(self) -> Option<VideoBarInput> {
        let ab = |button| Some(VideoBarInput::Ab(AbInput::Button(button)));
        match self {
            Self::Milliseconds => Some(VideoBarInput::ToggleMs),
            Self::Mute => Some(VideoBarInput::Speaker),
            Self::AbSetA => ab(AbButton::SetA),
            Self::AbSetB => ab(AbButton::SetB),
            Self::AbLoop => ab(AbButton::Loop),
            Self::AbClear => ab(AbButton::Clear),
            Self::AbSave => ab(AbButton::Save),
            Self::AbLoad => ab(AbButton::Load),
            Self::Volume | Self::AbSubmenu => None,
        }
    }
}

/// How an entry looks (the kinds of `ContextMenuItemModel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Action,
    Check(bool),
    Separator,
    /// A line that only tells something (the times).
    Header,
    Submenu,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoreEntry {
    pub action: Option<MoreAction>,
    pub kind: EntryKind,
    pub text: String,
    pub shortcut: String,
    pub icon: Option<Icon>,
    pub enabled: bool,
    pub tooltip: String,
    /// The submenu of this entry is open (it stays highlighted).
    pub open: bool,
}

impl MoreEntry {
    fn new(action: MoreAction, kind: EntryKind, text: impl Into<String>, icon: Option<Icon>) -> Self {
        Self {
            action: Some(action),
            kind,
            text: text.into(),
            shortcut: String::new(),
            icon,
            enabled: true,
            tooltip: String::new(),
            open: false,
        }
    }

    fn separator() -> Self {
        Self {
            action: None,
            kind: EntryKind::Separator,
            text: String::new(),
            shortcut: String::new(),
            icon: None,
            enabled: false,
            tooltip: String::new(),
            open: false,
        }
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn tip(mut self, tooltip: &str, shortcut: &str) -> Self {
        tooltip.clone_into(&mut self.tooltip);
        shortcut.clone_into(&mut self.shortcut);
        self
    }
}

/// What the menu is made from.
pub struct MoreInput<'a> {
    pub fit: BarFit,
    pub times: &'a TimeTexts,
    pub ms: bool,
    /// The volume in percent.
    pub volume: f64,
    pub volume_icon: VolumeIcon,
    pub muted: bool,
    pub ab: Option<&'a AbBarView>,
    pub sub: Option<MoreSub>,
}

/// The entries of the menu and of its open submenu (empty: no "..." in a bar of this step).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MoreMenu {
    pub entries: Vec<MoreEntry>,
    pub sub: Vec<MoreEntry>,
}

pub fn build(input: &MoreInput) -> MoreMenu {
    if !input.fit.has_more() {
        return MoreMenu::default();
    }
    let mut entries = Vec::new();
    if input.fit == BarFit::NoTime {
        entries.push(MoreEntry {
            action: None,
            kind: EntryKind::Header,
            text: format!("{}  /  {}", input.times.current, input.times.remaining),
            ..MoreEntry::separator()
        });
        entries.push(MoreEntry::new(
            MoreAction::Milliseconds,
            EntryKind::Check(input.ms),
            "Milliseconds",
            None,
        ));
        entries.push(MoreEntry::separator());
    }
    let speaker = match input.volume_icon {
        VolumeIcon::Muted => Icon::VolumeOff,
        VolumeIcon::Low => Icon::VolumeLow,
        VolumeIcon::High => Icon::VolumeOn,
    };
    entries.push(MoreEntry::new(
        MoreAction::Volume,
        EntryKind::Action,
        format!("Volume {:.0} %", input.volume.max(0.0)),
        Some(speaker),
    ));
    entries.push(MoreEntry::new(
        MoreAction::Mute,
        EntryKind::Check(input.muted),
        "Mute",
        None,
    ));
    let mut sub = Vec::new();
    if let Some(ab) = input.ab {
        entries.push(MoreEntry::separator());
        let open = input.sub == Some(MoreSub::AbRepeat);
        entries.push(MoreEntry {
            open,
            ..MoreEntry::new(
                MoreAction::AbSubmenu,
                EntryKind::Submenu,
                "A-B Repeat",
                Some(Icon::AbLoop),
            )
        });
        if open {
            sub = ab_entries(ab);
        }
    }
    MoreMenu { entries, sub }
}

/// The A-B submenu: the five controls of the group, plus clearing A and B (the right click on AB in the bar).
fn ab_entries(ab: &AbBarView) -> Vec<MoreEntry> {
    let looping = ab.loop_look == LoopLook::Looping;
    vec![
        MoreEntry::new(MoreAction::AbSetA, EntryKind::Action, "Set A", Some(Icon::AbSetA)).tip(&ab.a_tip, &ab.a_key),
        MoreEntry::new(MoreAction::AbSetB, EntryKind::Action, "Set B", Some(Icon::AbSetB)).tip(&ab.b_tip, &ab.b_key),
        MoreEntry::new(MoreAction::AbLoop, EntryKind::Check(looping), "Loop A-B", None)
            .enabled(ab.loop_look != LoopLook::Disabled)
            .tip(&ab.loop_tip, ""),
        MoreEntry::new(
            MoreAction::AbClear,
            EntryKind::Action,
            "Clear A and B",
            Some(Icon::AbLoopOff),
        )
        .enabled(ab.clearable)
        .tip("", &ab.loop_key),
        MoreEntry::separator(),
        MoreEntry::new(MoreAction::AbSave, EntryKind::Action, "Save...", Some(Icon::AbSave))
            .enabled(ab.save_enabled)
            .tip(&ab.save_tip, ""),
        MoreEntry::new(
            MoreAction::AbLoad,
            EntryKind::Action,
            format!("Load... ({})", ab.load_count),
            Some(Icon::AbLoad),
        )
        .enabled(ab.load_count > 0)
        .tip(&ab.load_tip, ""),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::video_bar::ab_view::PointLook;

    fn times() -> TimeTexts {
        TimeTexts {
            current: "0:12".into(),
            remaining: "-1:18".into(),
            current_template: "0:00".into(),
            remaining_template: "-0:00".into(),
        }
    }

    fn ab() -> AbBarView {
        AbBarView {
            a: PointLook::Idle,
            b: PointLook::Unset,
            loop_look: LoopLook::Disabled,
            a_tip: "Set A".into(),
            b_tip: "Set B".into(),
            loop_tip: "Loop".into(),
            a_key: "Home".into(),
            b_key: "End".into(),
            loop_key: String::new(),
            clearable: true,
            save_enabled: false,
            save_tip: String::new(),
            load_count: 3,
            load_tip: String::new(),
            strip: Vec::new(),
            span: None,
            dim: false,
            a_flag: None,
            b_flag: None,
            hover: None,
            save: None,
            menu: None,
        }
    }

    fn menu(fit: BarFit, ab: Option<&AbBarView>, sub: Option<MoreSub>) -> MoreMenu {
        let times = times();
        build(&MoreInput {
            fit,
            times: &times,
            ms: false,
            volume: 70.0,
            volume_icon: VolumeIcon::High,
            muted: false,
            ab,
            sub,
        })
    }

    fn actions(entries: &[MoreEntry]) -> Vec<Option<MoreAction>> {
        entries.iter().map(|e| e.action).collect()
    }

    #[test]
    fn no_menu_while_everything_fits_in_the_bar() {
        assert_eq!(menu(BarFit::AbButton, Some(&ab()), None), MoreMenu::default());
    }

    #[test]
    fn the_sound_and_the_a_b_submenu_once_they_left_the_bar() {
        let ab = ab();
        let closed = menu(BarFit::More, Some(&ab), None);
        assert_eq!(
            actions(&closed.entries),
            [
                Some(MoreAction::Volume),
                Some(MoreAction::Mute),
                None,
                Some(MoreAction::AbSubmenu)
            ]
        );
        assert_eq!(closed.entries[0].text, "Volume 70 %");
        assert_eq!(closed.sub, []);
        let open = menu(BarFit::More, Some(&ab), Some(MoreSub::AbRepeat));
        assert!(open.entries[3].open);
        assert_eq!(open.sub.len(), 7);
        assert_eq!(open.sub[0].shortcut, "Home");
        assert!(!open.sub[2].enabled, "no valid A-B: no loop");
        assert!(open.sub[3].enabled, "A is set: it can be cleared");
        assert!(!open.sub[5].enabled, "nothing to save");
        assert_eq!(open.sub[6].text, "Load... (3)");
        // Without the script there is no A-B submenu.
        assert_eq!(actions(&menu(BarFit::More, None, None).entries).len(), 2);
    }

    #[test]
    fn the_narrowest_bar_names_the_times() {
        let narrow = menu(BarFit::NoTime, None, None);
        assert_eq!(narrow.entries[0].kind, EntryKind::Header);
        assert_eq!(narrow.entries[0].text, "0:12  /  -1:18");
        assert_eq!(narrow.entries[1].kind, EntryKind::Check(false));
        assert_eq!(narrow.entries[1].action, Some(MoreAction::Milliseconds));
    }

    #[test]
    fn codes_go_back_and_forth_and_entries_do_what_their_control_does() {
        for action in MoreAction::ALL {
            assert_eq!(MoreAction::from_code(action.code()), Some(action));
        }
        assert_eq!(MoreAction::from_code(-1), None);
        assert_eq!(MoreAction::from_code(99), None);
        assert_eq!(MoreAction::Mute.input(), Some(VideoBarInput::Speaker));
        assert_eq!(
            MoreAction::AbLoad.input(),
            Some(VideoBarInput::Ab(AbInput::Button(AbButton::Load)))
        );
        assert_eq!(MoreAction::Volume.input(), None);
    }
}
