//! Settings > Fullscreen > Selected Medium and the switch that hides its marks with the mouse cursor: the dialog shows the
//! steps by index, the settings store them by name (`MULTIVIEW_FOCUS_BRIGHTEN_LEVELS`, `MULTIVIEW_FOCUS_CORNER_LEVELS`).

use crate::ui::SettingsWindow;
use rustview_core::settings::schema::{
    DEFAULT_FULLSCREEN_SELECTION_BRIGHTEN, DEFAULT_FULLSCREEN_SELECTION_CORNER_MARKS, FullscreenSettings,
    MULTIVIEW_FOCUS_BRIGHTEN_LEVELS, MULTIVIEW_FOCUS_CORNER_LEVELS,
};
use rustview_core::util::cast::SaturatingAs;

/// The index of the step `name` in `levels` (the default's index for an unknown name).
fn index_of<T>(levels: &[(&str, T)], name: &str, default: &str) -> i32 {
    let position = |wanted: &str| levels.iter().position(|(n, _)| *n == wanted);
    position(name)
        .or_else(|| position(default))
        .unwrap_or(0)
        .saturating_as::<i32>()
}

/// The name of the step at `index` in `levels` (`default` for an index out of range).
fn name_at<T>(levels: &[(&str, T)], index: i32, default: &str) -> String {
    usize::try_from(index)
        .ok()
        .and_then(|i| levels.get(i))
        .map_or(default, |(name, _)| *name)
        .to_string()
}

pub(super) fn populate(window: &SettingsWindow, fullscreen: &FullscreenSettings) {
    let selection = &fullscreen.selection;
    window.set_fullscreen_selection_brighten_idx(index_of(
        &MULTIVIEW_FOCUS_BRIGHTEN_LEVELS,
        &selection.brighten,
        DEFAULT_FULLSCREEN_SELECTION_BRIGHTEN,
    ));
    window.set_fullscreen_selection_corner_idx(index_of(
        &MULTIVIEW_FOCUS_CORNER_LEVELS,
        &selection.corner_marks,
        DEFAULT_FULLSCREEN_SELECTION_CORNER_MARKS,
    ));
    window.set_fullscreen_hide_selection_with_cursor(fullscreen.primary_bar.hide_selection_with_cursor);
}

pub(super) fn save(window: &SettingsWindow, fullscreen: &mut FullscreenSettings) {
    fullscreen.selection.brighten = name_at(
        &MULTIVIEW_FOCUS_BRIGHTEN_LEVELS,
        window.get_fullscreen_selection_brighten_idx(),
        DEFAULT_FULLSCREEN_SELECTION_BRIGHTEN,
    );
    fullscreen.selection.corner_marks = name_at(
        &MULTIVIEW_FOCUS_CORNER_LEVELS,
        window.get_fullscreen_selection_corner_idx(),
        DEFAULT_FULLSCREEN_SELECTION_CORNER_MARKS,
    );
    fullscreen.primary_bar.hide_selection_with_cursor = window.get_fullscreen_hide_selection_with_cursor();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_indexes_of_the_steps_round_trip() {
        for (index, (name, _)) in MULTIVIEW_FOCUS_BRIGHTEN_LEVELS.iter().enumerate() {
            let index = index.saturating_as::<i32>();
            assert_eq!(index_of(&MULTIVIEW_FOCUS_BRIGHTEN_LEVELS, name, "medium"), index);
            assert_eq!(name_at(&MULTIVIEW_FOCUS_BRIGHTEN_LEVELS, index, "medium"), *name);
        }
        assert_eq!(index_of(&MULTIVIEW_FOCUS_CORNER_LEVELS, "huge", "medium"), 1);
        assert_eq!(name_at(&MULTIVIEW_FOCUS_CORNER_LEVELS, 7, "medium"), "medium");
        assert_eq!(name_at(&MULTIVIEW_FOCUS_CORNER_LEVELS, -1, "medium"), "medium");
    }
}
