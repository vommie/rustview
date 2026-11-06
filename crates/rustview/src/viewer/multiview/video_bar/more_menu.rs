//! The "..." menu of a narrow control bar (pure): the controls the bar has no room for, as entries of a menu with submenus.
//! Which entries there are follows the bar's step (`collapse::BarFit`): the filters (a submenu with the groups Image and
//! Sound) once the filter button left the bar, the sound and the A-B repeat (a submenu) once the speaker and the A-B button
//! did, the times once the current time did. Every entry does what its control in the bar does (`input`).

use super::ab_view::{AbBarView, LoopLook};
use super::bar_menu::{EntryKind, MenuAction, MenuEntry, MenuLevels, MenuPath, MenuSub};
use super::collapse::BarFit;
use super::time_text::TimeTexts;
use super::view::{AbButton, AbInput, VideoBarInput};
use super::volume::VolumeIcon;
use crate::icons::Icon;
use crate::viewer::multiview::video_filter::menu::{self as filter_menu, FilterKeys};
use crate::viewer::multiview::video_filter::{Normalize, VideoFilters};

/// The input of the bar an entry of the "..." menu stands for (`None`: it opens a popup or a submenu, or it is a filter).
pub fn input(action: MenuAction) -> Option<VideoBarInput> {
    let ab = |button| Some(VideoBarInput::Ab(AbInput::Button(button)));
    match action {
        MenuAction::Milliseconds => Some(VideoBarInput::ToggleMs),
        MenuAction::Mute => Some(VideoBarInput::Speaker),
        MenuAction::AbSetA => ab(AbButton::SetA),
        MenuAction::AbSetB => ab(AbButton::SetB),
        MenuAction::AbLoop => ab(AbButton::Loop),
        MenuAction::AbClear => ab(AbButton::Clear),
        MenuAction::AbSave => ab(AbButton::Save),
        MenuAction::AbLoad => ab(AbButton::Load),
        _ => None,
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
    pub filters: &'a VideoFilters,
    /// The normalization of Settings > Multiview > Player.
    pub default_normalize: Normalize,
    pub keys: &'a FilterKeys,
    pub path: MenuPath,
}

/// The menu with its open submenus (empty: no "..." in a bar of this step).
pub fn build(input: &MoreInput) -> MenuLevels {
    if !input.fit.has_more() {
        return MenuLevels::default();
    }
    let mut entries = Vec::new();
    if input.fit == BarFit::NoTime {
        entries.push(MenuEntry::header(format!(
            "{}  /  {}",
            input.times.current, input.times.remaining
        )));
        entries.push(MenuEntry::new(
            MenuAction::Milliseconds,
            EntryKind::Check(input.ms),
            "Milliseconds",
            None,
        ));
        entries.push(MenuEntry::separator());
    }
    if input.fit.sound_in_more() {
        let speaker = match input.volume_icon {
            VolumeIcon::Muted => Icon::VolumeOff,
            VolumeIcon::Low => Icon::VolumeLow,
            VolumeIcon::High => Icon::VolumeOn,
        };
        entries.push(MenuEntry::new(
            MenuAction::Volume,
            EntryKind::Action,
            format!("Volume {:.0} %", input.volume.max(0.0)),
            Some(speaker),
        ));
        entries.push(MenuEntry::new(
            MenuAction::Mute,
            EntryKind::Check(input.muted),
            "Mute",
            None,
        ));
        if input.ab.is_some() {
            entries.push(MenuEntry::separator());
            entries.push(MenuEntry::submenu(
                MenuSub::AbRepeat,
                "A-B Repeat",
                Some(Icon::AbLoop),
                input.path,
                0,
            ));
        }
        entries.push(MenuEntry::separator());
    }
    entries.push(filter_menu::more_entry(
        input.filters,
        input.default_normalize,
        input.path,
    ));
    let (sub, sub2) = match input.path.at(0) {
        Some(MenuSub::AbRepeat) => (input.ab.map(ab_entries).unwrap_or_default(), Vec::new()),
        Some(MenuSub::Filters) => {
            filter_menu::more_levels(input.filters, input.keys, input.default_normalize, input.path)
        }
        _ => (Vec::new(), Vec::new()),
    };
    MenuLevels { entries, sub, sub2 }
}

/// The A-B submenu: the five controls of the group, plus clearing A and B (the right click on AB in the bar).
fn ab_entries(ab: &AbBarView) -> Vec<MenuEntry> {
    let looping = ab.loop_look == LoopLook::Looping;
    vec![
        MenuEntry::new(MenuAction::AbSetA, EntryKind::Action, "Set A", Some(Icon::AbSetA)).tip(&ab.a_tip, &ab.a_key),
        MenuEntry::new(MenuAction::AbSetB, EntryKind::Action, "Set B", Some(Icon::AbSetB)).tip(&ab.b_tip, &ab.b_key),
        MenuEntry::new(MenuAction::AbLoop, EntryKind::Check(looping), "Loop A-B", None)
            .enabled(ab.loop_look != LoopLook::Disabled)
            .tip(&ab.loop_tip, ""),
        MenuEntry::new(
            MenuAction::AbClear,
            EntryKind::Action,
            "Clear A and B",
            Some(Icon::AbLoopOff),
        )
        .enabled(ab.clearable)
        .tip("", &ab.loop_key),
        MenuEntry::separator(),
        MenuEntry::new(MenuAction::AbSave, EntryKind::Action, "Save...", Some(Icon::AbSave))
            .enabled(ab.save_enabled)
            .tip(&ab.save_tip, ""),
        MenuEntry::new(
            MenuAction::AbLoad,
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

    fn menu(fit: BarFit, ab: Option<&AbBarView>, path: MenuPath) -> MenuLevels {
        let times = times();
        let mut filters = VideoFilters::default();
        filters.mirror = true;
        build(&MoreInput {
            fit,
            times: &times,
            ms: false,
            volume: 70.0,
            volume_icon: VolumeIcon::High,
            muted: false,
            ab,
            filters: &filters,
            default_normalize: Normalize::Off,
            keys: &FilterKeys::default(),
            path,
        })
    }

    fn actions(entries: &[MenuEntry]) -> Vec<Option<MenuAction>> {
        entries.iter().map(|e| e.action).collect()
    }

    #[test]
    fn no_menu_while_everything_fits_in_the_bar() {
        assert_eq!(
            menu(BarFit::AbButton, Some(&ab()), MenuPath::default()),
            MenuLevels::default()
        );
    }

    #[test]
    fn first_the_filters_then_the_sound_and_the_a_b_submenu() {
        let filters_only = menu(BarFit::FilterMore, Some(&ab()), MenuPath::default());
        assert_eq!(
            actions(&filters_only.entries),
            [Some(MenuAction::Open(MenuSub::Filters))]
        );
        assert_eq!(
            (
                filters_only.entries[0].text.as_str(),
                filters_only.entries[0].value.as_str()
            ),
            ("Filters", "1 on")
        );
        let ab = ab();
        let closed = menu(BarFit::More, Some(&ab), MenuPath::default());
        assert_eq!(
            actions(&closed.entries),
            [
                Some(MenuAction::Volume),
                Some(MenuAction::Mute),
                None,
                Some(MenuAction::Open(MenuSub::AbRepeat)),
                None,
                Some(MenuAction::Open(MenuSub::Filters)),
            ]
        );
        assert_eq!(closed.entries[0].text, "Volume 70 %");
        assert_eq!(closed.sub, []);
        let open = menu(BarFit::More, Some(&ab), MenuPath([Some(MenuSub::AbRepeat), None]));
        assert!(open.entries[3].open);
        assert_eq!(open.open_code(0), MenuAction::Open(MenuSub::AbRepeat).code());
        assert_eq!(open.sub.len(), 7);
        assert_eq!(open.sub[0].shortcut, "Home");
        assert!(!open.sub[2].enabled, "no valid A-B: no loop");
        assert!(open.sub[3].enabled, "A is set: it can be cleared");
        assert!(!open.sub[5].enabled, "nothing to save");
        assert_eq!(open.sub[6].text, "Load... (3)");
        // Without the script there is no A-B submenu.
        assert_eq!(actions(&menu(BarFit::More, None, MenuPath::default()).entries).len(), 4);
    }

    #[test]
    fn the_filters_open_their_groups_in_two_levels() {
        let path = MenuPath([Some(MenuSub::Filters), Some(MenuSub::FilterImage)]);
        let levels = menu(BarFit::More, None, path);
        assert!(
            levels
                .sub
                .iter()
                .any(|e| e.action == Some(MenuAction::Open(MenuSub::FilterImage)) && e.open)
        );
        assert!(
            levels
                .sub2
                .iter()
                .any(|e| e.action == Some(MenuAction::Mirror) && e.kind == EntryKind::Filter(true))
        );
        assert!(levels.sub2.iter().any(|e| matches!(e.kind, EntryKind::Slider(0))));
    }

    #[test]
    fn the_narrowest_bar_names_the_times() {
        let narrow = menu(BarFit::NoTime, None, MenuPath::default());
        assert_eq!(narrow.entries[0].kind, EntryKind::Header);
        assert_eq!(narrow.entries[0].text, "0:12  /  -1:18");
        assert_eq!(narrow.entries[1].kind, EntryKind::Check(false));
        assert_eq!(narrow.entries[1].action, Some(MenuAction::Milliseconds));
    }

    #[test]
    fn entries_do_what_their_control_does() {
        assert_eq!(input(MenuAction::Mute), Some(VideoBarInput::Speaker));
        assert_eq!(
            input(MenuAction::AbLoad),
            Some(VideoBarInput::Ab(AbInput::Button(AbButton::Load)))
        );
        assert_eq!(input(MenuAction::Volume), None);
        assert_eq!(input(MenuAction::Mirror), None);
    }
}
