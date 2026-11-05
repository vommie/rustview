//! The menus of a control bar (pure): the "..." menu and the filter menu share their entries, actions and submenus. A menu
//! has up to three levels (the "..." menu > Filters > Image); the open path is part of the bar's popup (`popups::BarPopup`).
//! Every entry reports its action's code (`MenuAction::code`), a slider also its value.

use crate::icons::Icon;
use crate::viewer::multiview::video_filter::{FilterChange, Knob, Normalize};

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
    RotateRight,
    RotateLeft,
    Rotate180,
    Mirror,
    Deinterlace,
    /// A slider of the picture adjustments.
    Knob(Knob),
    ResetPicture,
    Normalize(Normalize),
    AllFiltersOff,
}

const SUBS: [MenuSub; 5] = [
    MenuSub::AbRepeat,
    MenuSub::Filters,
    MenuSub::FilterImage,
    MenuSub::FilterPicture,
    MenuSub::FilterSound,
];

const PLAIN: [MenuAction; 16] = [
    MenuAction::Milliseconds,
    MenuAction::Volume,
    MenuAction::Mute,
    MenuAction::AbSetA,
    MenuAction::AbSetB,
    MenuAction::AbLoop,
    MenuAction::AbClear,
    MenuAction::AbSave,
    MenuAction::AbLoad,
    MenuAction::RotateRight,
    MenuAction::RotateLeft,
    MenuAction::Rotate180,
    MenuAction::Mirror,
    MenuAction::Deinterlace,
    MenuAction::ResetPicture,
    MenuAction::AllFiltersOff,
];

/// The codes of the actions with a value: submenus from 100, knobs from 200, normalizations from 300.
const SUB_BASE: i32 = 100;
const KNOB_BASE: i32 = 200;
const NORMALIZE_BASE: i32 = 300;

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
            plain => position(&PLAIN, &plain),
        }
    }

    /// The change of the video filters the entry stands for (`None`: no filter, or a slider, which reports its value).
    pub fn filter_change(self) -> Option<FilterChange> {
        match self {
            Self::RotateRight => Some(FilterChange::Rotate(90)),
            Self::RotateLeft => Some(FilterChange::Rotate(-90)),
            Self::Rotate180 => Some(FilterChange::Rotate(180)),
            Self::Mirror => Some(FilterChange::ToggleMirror),
            Self::Deinterlace => Some(FilterChange::ToggleDeinterlace),
            Self::ResetPicture => Some(FilterChange::ResetPicture),
            Self::Normalize(normalize) => Some(FilterChange::Normalize(normalize)),
            Self::AllFiltersOff => Some(FilterChange::AllOff),
            _ => None,
        }
    }

    pub fn from_code(code: i32) -> Option<Self> {
        match code {
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
    /// A slider from -`KNOB_RANGE` to `KNOB_RANGE`.
    Slider(i32),
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

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
        let mut codes = std::collections::HashSet::new();
        for action in all {
            let code = action.code();
            assert!(code >= 0 && codes.insert(code), "{action:?} -> {code}");
            assert_eq!(MenuAction::from_code(code), Some(action));
        }
        assert_eq!(MenuAction::from_code(-1), None);
        assert_eq!(MenuAction::from_code(99), None);
        assert_eq!(MenuAction::from_code(199), None);
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
