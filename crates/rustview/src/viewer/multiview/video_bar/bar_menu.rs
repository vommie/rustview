//! The menus of a control bar (pure): the "..." menu and the filter menu share their entries, actions and submenus. A menu
//! has up to three levels (the "..." menu > Filters > Image); the open path is part of the bar's popup (`popups::BarPopup`).
//! Every entry reports its action's code (`MenuAction::code`), a slider also its value.

use crate::icons::Icon;
use crate::viewer::multiview::video_filter::{FilterChange, FilterPart, Knob, Normalize};

/// The submenus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuSub {
    AbRepeat,
    /// The filters inside the "..." menu (their groups).
    Filters,
    /// The image filters of the "..." menu's Filters submenu.
    FilterImage,
    /// The picture adjustments of the filter menu.
    FilterPicture,
    /// The volume normalization.
    FilterSound,
}

/// The open submenus of a menu: the one of its first level and the one of the second.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MenuPath(pub [Option<MenuSub>; 2]);

impl MenuPath {
    /// A submenu entry of level `level` (0 = the menu itself) was clicked: its submenu opens, or closes when it is open;
    /// deeper submenus close.
    pub fn toggle(&mut self, level: usize, sub: MenuSub) {
        let Some(slot) = self.0.get(level).copied() else {
            return;
        };
        self.0[level] = if slot == Some(sub) { None } else { Some(sub) };
        for deeper in self.0.iter_mut().skip(level + 1) {
            *deeper = None;
        }
    }

    /// The open submenu of level `level`.
    pub fn at(&self, level: usize) -> Option<MenuSub> {
        self.0.get(level).copied().flatten()
    }

    /// The level whose entry `sub` is (0, 1), if `sub` can be opened from the menu `top`'s level.
    pub fn level_of(sub: MenuSub, filter_menu: bool) -> usize {
        match (sub, filter_menu) {
            (MenuSub::FilterImage | MenuSub::FilterSound, false) => 1,
            _ => 0,
        }
    }
}

/// What an entry does. Its code is the id the canvas reports back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAction {
    /// The times are shown with milliseconds or without.
    Milliseconds,
    /// The vertical volume bar opens above "...".
    Volume,
    Mute,
    Open(MenuSub),
    AbSetA,
    AbSetB,
    AbLoop,
    AbClear,
    AbSave,
    AbLoad,
    /// A rotation entry: this many degrees clockwise (90, 180, 270), or none again when it is the one shown.
    Rotation(u16),
    Mirror,
    Deinterlace,
    /// A slider of the picture adjustments.
    Knob(Knob),
    Normalize(Normalize),
    AllFiltersOff,
}

/// The rotation entries of the filter menus.
pub const ROTATIONS: [u16; 3] = [90, 180, 270];

const SUBS: [MenuSub; 5] = [
    MenuSub::AbRepeat,
    MenuSub::Filters,
    MenuSub::FilterImage,
    MenuSub::FilterPicture,
    MenuSub::FilterSound,
];

const PLAIN: [MenuAction; 12] = [
    MenuAction::Milliseconds,
    MenuAction::Volume,
    MenuAction::Mute,
    MenuAction::AbSetA,
    MenuAction::AbSetB,
    MenuAction::AbLoop,
    MenuAction::AbClear,
    MenuAction::AbSave,
    MenuAction::AbLoad,
    MenuAction::Mirror,
    MenuAction::Deinterlace,
    MenuAction::AllFiltersOff,
];

/// The codes of the actions with a value: submenus from 100, knobs from 200, normalizations from 300, rotations from 400.
const SUB_BASE: i32 = 100;
const KNOB_BASE: i32 = 200;
const NORMALIZE_BASE: i32 = 300;
const ROTATION_BASE: i32 = 400;

fn position<T: PartialEq>(list: &[T], item: &T) -> i32 {
    list.iter()
        .position(|x| x == item)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(-1)
}

fn at<T: Copy>(list: &[T], code: i32, base: i32) -> Option<T> {
    usize::try_from(code - base).ok().and_then(|i| list.get(i).copied())
}

impl MenuAction {
    pub fn code(self) -> i32 {
        match self {
            Self::Open(sub) => SUB_BASE + position(&SUBS, &sub),
            Self::Knob(knob) => KNOB_BASE + position(&Knob::ALL, &knob),
            Self::Normalize(normalize) => NORMALIZE_BASE + position(&Normalize::ALL, &normalize),
            Self::Rotation(degrees) => ROTATION_BASE + position(&ROTATIONS, &degrees),
            plain => position(&PLAIN, &plain),
        }
    }

    /// What the reset button of the entry puts back (`None`: the entry has none).
    pub fn reset_part(self) -> Option<FilterPart> {
        match self {
            Self::Rotation(_) => Some(FilterPart::Rotation),
            Self::Mirror => Some(FilterPart::Mirror),
            Self::Deinterlace => Some(FilterPart::Deinterlace),
            Self::Knob(knob) => Some(FilterPart::Knob(knob)),
            Self::Open(MenuSub::FilterPicture) => Some(FilterPart::Picture),
            Self::Open(MenuSub::FilterSound) => Some(FilterPart::Normalize),
            Self::Open(MenuSub::FilterImage) => Some(FilterPart::Image),
            Self::Open(MenuSub::Filters) | Self::AllFiltersOff => Some(FilterPart::All),
            _ => None,
        }
    }

    /// The change of the video filters the entry stands for (`None`: no filter, or a slider, which reports its value).
    pub fn filter_change(self) -> Option<FilterChange> {
        match self {
            Self::Rotation(degrees) => Some(FilterChange::ToggleRotation(degrees)),
            Self::Mirror => Some(FilterChange::ToggleMirror),
            Self::Deinterlace => Some(FilterChange::ToggleDeinterlace),
            Self::Normalize(normalize) => Some(FilterChange::Normalize(normalize)),
            Self::AllFiltersOff => Some(FilterChange::Reset(FilterPart::All)),
            _ => None,
        }
    }

    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            c if c >= ROTATION_BASE => at(&ROTATIONS, c, ROTATION_BASE).map(Self::Rotation),
            c if c >= NORMALIZE_BASE => at(&Normalize::ALL, c, NORMALIZE_BASE).map(Self::Normalize),
            c if c >= KNOB_BASE => at(&Knob::ALL, c, KNOB_BASE).map(Self::Knob),
            c if c >= SUB_BASE => at(&SUBS, c, SUB_BASE).map(Self::Open),
            c => at(&PLAIN, c, 0),
        }
    }
}

/// How an entry looks (the kinds of `ContextMenuItemModel`, plus the slider).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Action,
    Check(bool),
    Radio(bool),
    Separator,
    /// A line that only tells something (the times, a group's name).
    Header,
    Submenu,
    /// A slider from -`KNOB_RANGE` to `KNOB_RANGE` (with a reset button).
    Slider(i32),
    /// A filter: its icon stays, while it is on (`true`) the icon and the value are in the filter color; a value column and a
    /// reset button follow (`MenuEntry::value`, `reset`).
    Filter(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuEntry {
    pub action: Option<MenuAction>,
    pub kind: EntryKind,
    pub text: String,
    pub shortcut: String,
    pub icon: Option<Icon>,
    pub enabled: bool,
    pub tooltip: String,
    /// The submenu of this entry is open (it stays highlighted).
    pub open: bool,
    /// The value column of a filter or slider (the text, and the widest text it can have, so the column never changes
    /// width while the value changes).
    pub value: String,
    pub value_template: String,
    /// The reset button of a filter or slider: `Some(enabled)` (enabled only while the filter is on).
    pub reset: Option<bool>,
}

impl MenuEntry {
    pub fn new(action: MenuAction, kind: EntryKind, text: impl Into<String>, icon: Option<Icon>) -> Self {
        Self {
            action: Some(action),
            kind,
            text: text.into(),
            shortcut: String::new(),
            icon,
            enabled: true,
            tooltip: String::new(),
            open: false,
            value: String::new(),
            value_template: String::new(),
            reset: None,
        }
    }

    pub fn separator() -> Self {
        Self {
            action: None,
            kind: EntryKind::Separator,
            text: String::new(),
            shortcut: String::new(),
            icon: None,
            enabled: false,
            tooltip: String::new(),
            open: false,
            value: String::new(),
            value_template: String::new(),
            reset: None,
        }
    }

    pub fn header(text: impl Into<String>) -> Self {
        Self {
            kind: EntryKind::Header,
            text: text.into(),
            ..Self::separator()
        }
    }

    /// The entry of submenu `sub`, highlighted while it is open in `path` at `level`.
    pub fn submenu(sub: MenuSub, text: impl Into<String>, icon: Option<Icon>, path: MenuPath, level: usize) -> Self {
        Self {
            open: path.at(level) == Some(sub),
            ..Self::new(MenuAction::Open(sub), EntryKind::Submenu, text, icon)
        }
    }

    /// The entry of a filter group's submenu: a filter row (`active`: a filter of the group is on) that opens it.
    pub fn filter_submenu(
        sub: MenuSub,
        text: impl Into<String>,
        icon: Option<Icon>,
        (path, level): (MenuPath, usize),
        active: bool,
    ) -> Self {
        Self {
            open: path.at(level) == Some(sub),
            ..Self::new(MenuAction::Open(sub), EntryKind::Filter(active), text, icon)
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The value column and the reset button of a filter or slider (`template`: the widest value it can show).
    pub fn with_value(mut self, value: impl Into<String>, template: &str, resettable: bool) -> Self {
        self.value = value.into();
        template.clone_into(&mut self.value_template);
        self.reset = Some(resettable);
        self
    }

    pub fn tip(mut self, tooltip: &str, shortcut: &str) -> Self {
        tooltip.clone_into(&mut self.tooltip);
        shortcut.clone_into(&mut self.shortcut);
        self
    }
}

/// A menu with its open submenus (each empty when closed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuLevels {
    pub entries: Vec<MenuEntry>,
    pub sub: Vec<MenuEntry>,
    pub sub2: Vec<MenuEntry>,
}

impl MenuLevels {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The code of the open entry of level `level` (-1: none).
    pub fn open_code(&self, level: usize) -> i32 {
        let entries = match level {
            0 => &self.entries,
            _ => &self.sub,
        };
        entries
            .iter()
            .find(|entry| entry.open)
            .and_then(|entry| entry.action)
            .map_or(-1, MenuAction::code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_code_that_comes_back() {
        let mut all: Vec<MenuAction> = PLAIN.to_vec();
        all.extend(SUBS.map(MenuAction::Open));
        all.extend(Knob::ALL.map(MenuAction::Knob));
        all.extend(Normalize::ALL.map(MenuAction::Normalize));
        all.extend(ROTATIONS.map(MenuAction::Rotation));
        let mut codes = std::collections::HashSet::new();
        for action in all {
            let code = action.code();
            assert!(code >= 0 && codes.insert(code), "{action:?} -> {code}");
            assert_eq!(MenuAction::from_code(code), Some(action));
        }
        assert_eq!(MenuAction::from_code(-1), None);
        assert_eq!(MenuAction::from_code(99), None);
        assert_eq!(MenuAction::from_code(199), None);
        assert_eq!(MenuAction::Rotation(90).reset_part(), Some(FilterPart::Rotation));
        assert_eq!(
            MenuAction::Open(MenuSub::FilterSound).reset_part(),
            Some(FilterPart::Normalize)
        );
        assert_eq!(MenuAction::Open(MenuSub::AbRepeat).reset_part(), None);
    }

    #[test]
    fn a_submenu_opens_closes_and_takes_its_deeper_levels_along() {
        let mut path = MenuPath::default();
        path.toggle(0, MenuSub::Filters);
        path.toggle(1, MenuSub::FilterImage);
        assert_eq!(path, MenuPath([Some(MenuSub::Filters), Some(MenuSub::FilterImage)]));
        path.toggle(1, MenuSub::FilterSound);
        assert_eq!(path.at(1), Some(MenuSub::FilterSound));
        path.toggle(0, MenuSub::AbRepeat);
        assert_eq!(path, MenuPath([Some(MenuSub::AbRepeat), None]));
        path.toggle(0, MenuSub::AbRepeat);
        assert_eq!(path, MenuPath::default());
        path.toggle(5, MenuSub::AbRepeat);
        assert_eq!(path, MenuPath::default());
        assert_eq!(MenuPath::level_of(MenuSub::FilterImage, false), 1);
        assert_eq!(MenuPath::level_of(MenuSub::FilterSound, true), 0);
    }
}
