//! The entries of the filter menu (pure): the menu of the filter button, and the Filters submenu of "..." (grouped into
//! Image and Sound, user decision 2026-11-05). Rotating and mirroring combine; the picture adjustments are sliders.

use super::{KNOB_RANGE, Knob, Normalize, VideoFilters};
use crate::icons::Icon;
use crate::viewer::multiview::video_bar::bar_menu::{EntryKind, MenuAction, MenuEntry, MenuLevels, MenuPath, MenuSub};

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

/// "Filters", with the number of filters that are on.
pub fn title_with_count(filters: &VideoFilters) -> String {
    match filters.active_count() {
        0 => "Filters".to_owned(),
        n => format!("Filters ({n})"),
    }
}

fn image_header(filters: &VideoFilters) -> MenuEntry {
    match filters.rotation() {
        0 => MenuEntry::header("Image"),
        degrees => MenuEntry::header(format!("Image: turned {degrees} degrees")),
    }
}

/// Rotating, mirroring and deinterlacing.
fn image_entries(filters: &VideoFilters, keys: &FilterKeys) -> Vec<MenuEntry> {
    vec![
        image_header(filters),
        MenuEntry::new(
            MenuAction::RotateRight,
            EntryKind::Action,
            "Rotate Right",
            Some(Icon::VideoFilterRotateRight),
        )
        .tip(
            "A quarter turn clockwise; again and again: 90, 180, 270, back to 0 degrees",
            &keys.rotate_right,
        ),
        MenuEntry::new(
            MenuAction::RotateLeft,
            EntryKind::Action,
            "Rotate Left",
            Some(Icon::VideoFilterRotateLeft),
        )
        .tip("A quarter turn counterclockwise", &keys.rotate_left),
        MenuEntry::new(
            MenuAction::Rotate180,
            EntryKind::Action,
            "Rotate 180 Degrees",
            Some(Icon::VideoFilterRotate180),
        )
        .tip("Upside down to the right way up", ""),
        MenuEntry::new(MenuAction::Mirror, EntryKind::Check(filters.mirror), "Mirror", None)
            .tip("Left and right swapped, on top of the rotation", &keys.mirror),
        MenuEntry::new(
            MenuAction::Deinterlace,
            EntryKind::Check(filters.deinterlace),
            "Deinterlace",
            None,
        )
        .tip(
            "Removes the comb lines of interlaced video (old TV and camcorder recordings)",
            &keys.deinterlace,
        ),
    ]
}

/// The sliders of the picture adjustments and their reset.
fn picture_entries(filters: &VideoFilters, keys: &FilterKeys) -> Vec<MenuEntry> {
    let mut entries: Vec<MenuEntry> = Knob::ALL
        .into_iter()
        .map(|knob| {
            let shortcut = if knob == Knob::Gamma { keys.gamma.as_str() } else { "" };
            MenuEntry::new(
                MenuAction::Knob(knob),
                EntryKind::Slider(filters.knob(knob)),
                knob.title(),
                None,
            )
            .tip(
                &format!(
                    "From -{KNOB_RANGE} to {KNOB_RANGE}, 0 = as in the file. Drag or use the wheel; a right click sets 0"
                ),
                shortcut,
            )
        })
        .collect();
    let adjusted = Knob::ALL.iter().any(|&knob| filters.knob(knob) != 0);
    entries.push(MenuEntry::separator());
    entries.push(
        MenuEntry::new(MenuAction::ResetPicture, EntryKind::Action, "Reset Picture", None)
            .enabled(adjusted)
            .tip("Gamma, brightness, contrast and saturation back to 0", ""),
    );
    entries
}

/// The kinds of volume normalization (one of them is on).
fn sound_entries(filters: &VideoFilters) -> Vec<MenuEntry> {
    Normalize::ALL
        .into_iter()
        .map(|normalize| {
            MenuEntry::new(
                MenuAction::Normalize(normalize),
                EntryKind::Radio(filters.normalize == normalize),
                normalize.title(),
                None,
            )
            .tip(normalize.tip(), "")
        })
        .collect()
}

fn all_off(filters: &VideoFilters, keys: &FilterKeys) -> MenuEntry {
    MenuEntry::new(
        MenuAction::AllFiltersOff,
        EntryKind::Action,
        "All Filters Off",
        Some(Icon::VideoFilterOff),
    )
    .enabled(!filters.is_default())
    .tip("The video plays as it is in the file again", &keys.all_off)
}

/// The menu of the filter button: the image filters, the picture adjustments and the sound in submenus, all filters off.
pub fn button_menu(filters: &VideoFilters, keys: &FilterKeys, path: MenuPath) -> MenuLevels {
    let mut entries = image_entries(filters, keys);
    entries.push(MenuEntry::submenu(
        MenuSub::FilterPicture,
        "Adjust Picture",
        Some(Icon::VideoFilterPicture),
        path,
        0,
    ));
    entries.push(MenuEntry::separator());
    entries.push(MenuEntry::header("Sound"));
    entries.push(MenuEntry {
        shortcut: filters.normalize.title().to_owned(),
        ..MenuEntry::submenu(
            MenuSub::FilterSound,
            "Normalize Volume",
            Some(Icon::VideoFilterSound),
            path,
            0,
        )
    });
    entries.push(MenuEntry::separator());
    entries.push(all_off(filters, keys));
    let sub = match path.at(0) {
        Some(MenuSub::FilterPicture) => picture_entries(filters, keys),
        Some(MenuSub::FilterSound) => sound_entries(filters),
        _ => Vec::new(),
    };
    MenuLevels {
        entries,
        sub,
        sub2: Vec::new(),
    }
}

/// The Filters submenu of "..." (its groups) and the open group.
pub fn more_levels(filters: &VideoFilters, keys: &FilterKeys, path: MenuPath) -> (Vec<MenuEntry>, Vec<MenuEntry>) {
    let sub = vec![
        MenuEntry::submenu(MenuSub::FilterImage, "Image", Some(Icon::VideoFilterPicture), path, 1),
        MenuEntry::submenu(MenuSub::FilterSound, "Sound", Some(Icon::VideoFilterSound), path, 1),
        MenuEntry::separator(),
        all_off(filters, keys),
    ];
    let sub2 = match path.at(1) {
        Some(MenuSub::FilterImage) => {
            let mut entries = image_entries(filters, keys);
            entries.push(MenuEntry::separator());
            entries.extend(picture_entries(filters, keys));
            entries
        }
        Some(MenuSub::FilterSound) => sound_entries(filters),
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

    #[test]
    fn the_button_menu_shows_what_is_on() {
        let mut filters = VideoFilters::default();
        let menu = button_menu(&filters, &keys(), MenuPath::default());
        assert_eq!(menu.entries[0].text, "Image");
        assert_eq!(menu.entries[1].shortcut, "R");
        assert!(!menu.entries.last().is_some_and(|e| e.enabled), "nothing to switch off");
        assert_eq!(menu.sub, []);
        filters.rotate(90);
        filters.mirror = true;
        filters.normalize = Normalize::Dynamic;
        let menu = button_menu(&filters, &keys(), MenuPath([Some(MenuSub::FilterSound), None]));
        assert_eq!(menu.entries[0].text, "Image: turned 90 degrees");
        assert!(
            menu.entries
                .iter()
                .any(|e| e.action == Some(MenuAction::Mirror) && e.kind == EntryKind::Check(true))
        );
        assert!(menu.entries.iter().any(|e| e.open && e.shortcut == "Dynamic"));
        assert_eq!(menu.sub.len(), 5);
        assert_eq!(menu.sub[2].kind, EntryKind::Radio(true));
        assert!(menu.entries.last().is_some_and(|e| e.enabled));
    }

    #[test]
    fn the_picture_submenu_has_a_slider_per_knob_and_a_reset() {
        let mut filters = VideoFilters::default();
        filters.set_knob(Knob::Contrast, 30);
        let menu = button_menu(&filters, &keys(), MenuPath([Some(MenuSub::FilterPicture), None]));
        let sliders: Vec<EntryKind> = menu
            .sub
            .iter()
            .map(|e| e.kind)
            .filter(|k| matches!(k, EntryKind::Slider(_)))
            .collect();
        assert_eq!(
            sliders,
            [
                EntryKind::Slider(0),
                EntryKind::Slider(0),
                EntryKind::Slider(30),
                EntryKind::Slider(0)
            ]
        );
        assert!(
            menu.sub
                .last()
                .is_some_and(|e| e.action == Some(MenuAction::ResetPicture) && e.enabled)
        );
    }

    #[test]
    fn the_more_menu_groups_the_filters() {
        let filters = VideoFilters::default();
        let (sub, sub2) = more_levels(&filters, &keys(), MenuPath([Some(MenuSub::Filters), None]));
        assert_eq!(sub.len(), 4);
        assert_eq!(sub2, []);
        let (_, image) = more_levels(
            &filters,
            &keys(),
            MenuPath([Some(MenuSub::Filters), Some(MenuSub::FilterImage)]),
        );
        assert!(image.iter().any(|e| e.action == Some(MenuAction::RotateRight)));
        assert!(
            image
                .iter()
                .any(|e| e.action == Some(MenuAction::Knob(Knob::Saturation)))
        );
        assert_eq!(title_with_count(&filters), "Filters");
    }
}
