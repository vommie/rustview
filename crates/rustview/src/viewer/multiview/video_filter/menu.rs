//! The entries of the filter menus (pure): the menu of the filter button, and the Filters submenu of "..." (grouped into
//! Image and Sound, user decision 2026-11-05). Every filter is a filter row: its icon, its state in the value column (in the
//! filter color while it is on) and a reset button that is enabled only while the filter is on; the columns keep their
//! width whatever the values are, so nothing moves when a filter changes (user request 2026-11-06).

use super::{KNOB_RANGE, Knob, Normalize, VideoFilters};
use crate::icons::Icon;
use crate::viewer::multiview::video_bar::bar_menu::{
    EntryKind, MenuAction, MenuEntry, MenuLevels, MenuPath, MenuSub, ROTATIONS,
};

/// The keys of the filter actions as the menus name them ("" = no key).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterKeys {
    pub rotate_right: String,
    pub rotate_left: String,
    pub mirror: String,
    pub gamma: String,
    pub deinterlace: String,
    pub all_off: String,
}

/// The widest value of a toggled filter, of a group and of a slider.
const ON: &str = "On";
const COUNT_TEMPLATE: &str = "4 on";
const KNOB_TEMPLATE: &str = "-100";

fn count_text(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!("{count} on")
    }
}

/// The widest name of a normalization (the value column of the Normalize entry).
fn normalize_template() -> &'static str {
    Normalize::ALL
        .into_iter()
        .map(Normalize::title)
        .max_by_key(|title| title.len())
        .unwrap_or_default()
}

/// The parts of the image group that are on (rotation, mirror, deinterlacing, picture adjustments).
fn image_count(filters: &VideoFilters) -> usize {
    usize::from(filters.rotation() != 0)
        + usize::from(filters.mirror)
        + usize::from(filters.deinterlace)
        + filters.adjusted_knobs()
}

fn toggle(action: MenuAction, text: &str, icon: Icon, on: bool) -> MenuEntry {
    MenuEntry::new(action, EntryKind::Filter(on), text, Some(icon)).with_value(if on { ON } else { "" }, ON, on)
}

/// The rotations (one of them can be on; a click on the one that is on turns the picture back), mirroring, deinterlacing.
fn image_entries(filters: &VideoFilters, keys: &FilterKeys) -> Vec<MenuEntry> {
    let mut entries: Vec<MenuEntry> = ROTATIONS
        .into_iter()
        .map(|degrees| {
            let (text, icon, key, tip) = match degrees {
                90 => (
                    "Rotate 90\u{b0} Right",
                    Icon::VideoFilterRotateRight,
                    keys.rotate_right.as_str(),
                    "A quarter turn clockwise; a click on the rotation that is on turns the picture back. The key goes round: 90, 180, 270 degrees, back to none",
                ),
                180 => (
                    "Rotate 180\u{b0}",
                    Icon::VideoFilterRotate180,
                    "",
                    "Upside down to the right way up; a click on the rotation that is on turns the picture back",
                ),
                _ => (
                    "Rotate 90\u{b0} Left",
                    Icon::VideoFilterRotateLeft,
                    keys.rotate_left.as_str(),
                    "A quarter turn counterclockwise; a click on the rotation that is on turns the picture back",
                ),
            };
            toggle(MenuAction::Rotation(degrees), text, icon, filters.rotation() == degrees).tip(tip, key)
        })
        .collect();
    entries.push(
        toggle(MenuAction::Mirror, "Mirror", Icon::VideoFilterMirror, filters.mirror)
            .tip("Left and right swapped on screen, on top of the rotation", &keys.mirror),
    );
    entries.push(
        toggle(
            MenuAction::Deinterlace,
            "Deinterlace",
            Icon::VideoFilterDeinterlace,
            filters.deinterlace,
        )
        .tip(
            "Removes the comb lines of interlaced video (old TV and camcorder recordings)",
            &keys.deinterlace,
        ),
    );
    entries
}

/// The sliders of the picture adjustments, each with its reset.
fn picture_entries(filters: &VideoFilters, keys: &FilterKeys) -> Vec<MenuEntry> {
    Knob::ALL
        .into_iter()
        .map(|knob| {
            let value = filters.knob(knob);
            let shortcut = if knob == Knob::Gamma { keys.gamma.as_str() } else { "" };
            let text = if value > 0 { format!("+{value}") } else { value.to_string() };
            MenuEntry::new(MenuAction::Knob(knob), EntryKind::Slider(value), knob.title(), None)
                .with_value(text, KNOB_TEMPLATE, value != 0)
                .tip(
                    &format!(
                        "From -{KNOB_RANGE} to {KNOB_RANGE}, 0 = as in the file. Drag or use the wheel; a right click or the reset button sets 0"
                    ),
                    shortcut,
                )
        })
        .collect()
}

/// The kinds of volume normalization (one of them plays; the default of the settings is named).
fn sound_entries(filters: &VideoFilters, default: Normalize) -> Vec<MenuEntry> {
    let playing = filters.normalize_or(default);
    Normalize::ALL
        .into_iter()
        .map(|normalize| {
            let text = if normalize == default {
                format!("{} (default)", normalize.title())
            } else {
                normalize.title().to_owned()
            };
            MenuEntry::new(
                MenuAction::Normalize(normalize),
                EntryKind::Radio(playing == normalize),
                text,
                None,
            )
            .tip(normalize.tip(), "")
        })
        .collect()
}

/// The entry that opens the normalizations: the one that plays as its value, in the filter color while this video overrides
/// the default.
fn sound_entry(filters: &VideoFilters, default: Normalize, place: (MenuPath, usize), text: &str) -> MenuEntry {
    let overridden = filters.normalize_overridden(default);
    MenuEntry::filter_submenu(
        MenuSub::FilterSound,
        text,
        Some(Icon::VideoFilterSound),
        place,
        overridden,
    )
    .with_value(filters.normalize_or(default).title(), normalize_template(), overridden)
    .tip(
        &format!(
            "Every video starts with the normalization of Settings > Multiview > Player (now: {}). A choice here overrides it for this video in this view only, and a duplicated tile takes it over. The reset button goes back to the default",
            default.title()
        ),
        "",
    )
}

fn reset_all(filters: &VideoFilters, default: Normalize, keys: &FilterKeys) -> MenuEntry {
    MenuEntry::new(
        MenuAction::AllFiltersOff,
        EntryKind::Action,
        "Reset All Filters",
        Some(Icon::VideoFilterOff),
    )
    .enabled(!filters.is_plain(default))
    .tip(
        "The video plays as it is in the file again (with the default normalization)",
        &keys.all_off,
    )
}

/// The menu of the filter button: the image filters, the picture adjustments and the sound in submenus, reset all.
pub fn button_menu(filters: &VideoFilters, keys: &FilterKeys, default: Normalize, path: MenuPath) -> MenuLevels {
    let mut entries = vec![MenuEntry::header("Image")];
    entries.extend(image_entries(filters, keys));
    let adjusted = filters.adjusted_knobs();
    entries.push(
        MenuEntry::filter_submenu(
            MenuSub::FilterPicture,
            "Adjust Picture",
            Some(Icon::VideoFilterPicture),
            (path, 0),
            adjusted > 0,
        )
        .with_value(count_text(adjusted), COUNT_TEMPLATE, adjusted > 0)
        .tip(
            "Gamma, brightness, contrast and saturation; the reset button sets all four to 0",
            "",
        ),
    );
    entries.push(MenuEntry::separator());
    entries.push(MenuEntry::header("Sound"));
    entries.push(sound_entry(filters, default, (path, 0), "Normalize Volume"));
    entries.push(MenuEntry::separator());
    entries.push(reset_all(filters, default, keys));
    let sub = match path.at(0) {
        Some(MenuSub::FilterPicture) => picture_entries(filters, keys),
        Some(MenuSub::FilterSound) => sound_entries(filters, default),
        _ => Vec::new(),
    };
    MenuLevels {
        entries,
        sub,
        sub2: Vec::new(),
    }
}

/// The Filters entry of "...": its number of filters that are on as its value; its reset button resets them all.
pub fn more_entry(filters: &VideoFilters, default: Normalize, path: MenuPath) -> MenuEntry {
    let count = filters.active_count(default);
    MenuEntry::filter_submenu(
        MenuSub::Filters,
        "Filters",
        Some(Icon::VideoFilter),
        (path, 0),
        count > 0,
    )
    .with_value(count_text(count), COUNT_TEMPLATE, count > 0)
    .tip("The video filters; the reset button switches every filter off", "")
}

/// The Filters submenu of "..." (its groups) and the open group.
pub fn more_levels(
    filters: &VideoFilters,
    keys: &FilterKeys,
    default: Normalize,
    path: MenuPath,
) -> (Vec<MenuEntry>, Vec<MenuEntry>) {
    let image = image_count(filters);
    let sub = vec![
        MenuEntry::filter_submenu(
            MenuSub::FilterImage,
            "Image",
            Some(Icon::VideoFilterPicture),
            (path, 1),
            image > 0,
        )
        .with_value(count_text(image), COUNT_TEMPLATE, image > 0)
        .tip(
            "Rotation, mirror, deinterlacing and the picture adjustments; the reset button switches them off",
            "",
        ),
        sound_entry(filters, default, (path, 1), "Sound"),
        MenuEntry::separator(),
        reset_all(filters, default, keys),
    ];
    let sub2 = match path.at(1) {
        Some(MenuSub::FilterImage) => {
            let mut entries = image_entries(filters, keys);
            entries.push(MenuEntry::separator());
            entries.extend(picture_entries(filters, keys));
            entries
        }
        Some(MenuSub::FilterSound) => sound_entries(filters, default),
        _ => Vec::new(),
    };
    (sub, sub2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> FilterKeys {
        FilterKeys {
            rotate_right: "R".into(),
            ..FilterKeys::default()
        }
    }

    fn entry(entries: &[MenuEntry], action: MenuAction) -> &MenuEntry {
        entries
            .iter()
            .find(|e| e.action == Some(action))
            .expect("the entry is there")
    }

    #[test]
    fn the_button_menu_shows_which_filter_is_on_with_a_reset_only_then() {
        let mut filters = VideoFilters::default();
        let menu = button_menu(&filters, &keys(), Normalize::Off, MenuPath::default());
        let right = entry(&menu.entries, MenuAction::Rotation(90));
        assert_eq!(right.shortcut, "R");
        assert_eq!(
            (right.kind, right.value.as_str(), right.reset),
            (EntryKind::Filter(false), "", Some(false))
        );
        assert!(!menu.entries.last().is_some_and(|e| e.enabled), "nothing to reset");
        filters.set_rotation(180);
        filters.mirror = true;
        let menu = button_menu(&filters, &keys(), Normalize::Off, MenuPath::default());
        let half = entry(&menu.entries, MenuAction::Rotation(180));
        assert_eq!(
            (half.kind, half.value.as_str(), half.reset),
            (EntryKind::Filter(true), "On", Some(true))
        );
        assert_eq!(
            entry(&menu.entries, MenuAction::Rotation(90)).kind,
            EntryKind::Filter(false)
        );
        assert_eq!(entry(&menu.entries, MenuAction::Mirror).reset, Some(true));
        assert_eq!(entry(&menu.entries, MenuAction::Deinterlace).reset, Some(false));
        assert!(menu.entries.last().is_some_and(|e| e.enabled));
    }

    #[test]
    fn every_value_column_has_a_fixed_template() {
        let mut filters = VideoFilters::default();
        filters.set_knob(Knob::Contrast, 30);
        let menu = button_menu(
            &filters,
            &keys(),
            Normalize::Off,
            MenuPath([Some(MenuSub::FilterPicture), None]),
        );
        let contrast = entry(&menu.sub, MenuAction::Knob(Knob::Contrast));
        assert_eq!((contrast.kind, contrast.value.as_str()), (EntryKind::Slider(30), "+30"));
        assert_eq!(contrast.value_template, "-100");
        assert_eq!(entry(&menu.sub, MenuAction::Knob(Knob::Gamma)).reset, Some(false));
        let picture = entry(&menu.entries, MenuAction::Open(MenuSub::FilterPicture));
        assert_eq!(
            (picture.value.as_str(), picture.value_template.as_str()),
            ("1 on", "4 on")
        );
        let sound = entry(&menu.entries, MenuAction::Open(MenuSub::FilterSound));
        assert_eq!(sound.value_template, "Compressor");
    }

    #[test]
    fn the_normalization_starts_from_the_default_and_a_choice_overrides_it() {
        let mut filters = VideoFilters::default();
        let menu = button_menu(
            &filters,
            &keys(),
            Normalize::Loudness,
            MenuPath([Some(MenuSub::FilterSound), None]),
        );
        let sound = entry(&menu.entries, MenuAction::Open(MenuSub::FilterSound));
        assert_eq!(
            (sound.kind, sound.value.as_str(), sound.reset),
            (EntryKind::Filter(false), "Loudness", Some(false))
        );
        assert!(sound.tooltip.contains("now: Loudness"));
        let loud = entry(&menu.sub, MenuAction::Normalize(Normalize::Loudness));
        assert_eq!(
            (loud.kind, loud.text.as_str()),
            (EntryKind::Radio(true), "Loudness (default)")
        );
        filters.normalize = Some(Normalize::Speech);
        let menu = button_menu(&filters, &keys(), Normalize::Loudness, MenuPath::default());
        let sound = entry(&menu.entries, MenuAction::Open(MenuSub::FilterSound));
        assert_eq!(
            (sound.kind, sound.value.as_str(), sound.reset),
            (EntryKind::Filter(true), "Speech", Some(true))
        );
    }

    #[test]
    fn the_more_menu_groups_the_filters() {
        let filters = VideoFilters::default();
        let (sub, sub2) = more_levels(
            &filters,
            &keys(),
            Normalize::Off,
            MenuPath([Some(MenuSub::Filters), None]),
        );
        assert_eq!(sub.len(), 4);
        assert_eq!(sub2, []);
        let (_, image) = more_levels(
            &filters,
            &keys(),
            Normalize::Off,
            MenuPath([Some(MenuSub::Filters), Some(MenuSub::FilterImage)]),
        );
        assert!(image.iter().any(|e| e.action == Some(MenuAction::Rotation(270))));
        assert!(
            image
                .iter()
                .any(|e| e.action == Some(MenuAction::Knob(Knob::Saturation)))
        );
        assert_eq!(
            more_entry(&filters, Normalize::Off, MenuPath::default()).reset,
            Some(false)
        );
    }
}
