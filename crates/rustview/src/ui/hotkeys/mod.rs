//! Key presses -> actions of the keymap (`ui::keymap`, `docs/hotkeys.md`).
//!
//! The windows keep their fixed handling in Slint (dialog locks, popups, Esc, type-ahead find) and hand every other key press
//! to Rust: the main window through [`main_window`], the fullscreen window through `ui::multiview::session_keys`, and the
//! windows of the videos through the `rustview-key` message of mpv (resolved for the host of that player). This module keeps
//! the one keymap of the application: it is built from `settings.json` at startup and when the settings change.

pub mod main_window;

use crate::ui::keymap::{Action, Area, Chord, KeyPress, Keymap, mpv_keys, table_from_json};
use crate::viewer::mpv_engine::keys as mpv_engine_keys;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

static KEYMAP: LazyLock<RwLock<Arc<Keymap>>> = LazyLock::new(|| RwLock::new(Arc::new(Keymap::default())));

/// The keymap in use.
pub fn keymap() -> Arc<Keymap> {
    KEYMAP.read().clone()
}

/// Builds the keymap from the stored `hotkeys` table (repairs are logged) and gives the video windows their keys.
pub fn apply(stored: &BTreeMap<String, serde_json::Value>) {
    let (keymap, notes) = Keymap::from_table(&table_from_json(stored));
    for note in &notes {
        tracing::warn!(note, "[Hotkeys] A stored key was repaired");
    }
    mpv_engine_keys::set_bindings(&video_bindings(&keymap));
    tracing::info!(
        changed = stored.len(),
        repaired = notes.len(),
        "[Hotkeys] Keymap in use"
    );
    *KEYMAP.write() = Arc::new(keymap);
}

/// What reached a window: a key press of Slint, or a chord a video's window reported.
pub enum Trigger<'a> {
    Press(KeyPress<'a>),
    Chord(&'a Chord),
}

impl Trigger<'_> {
    fn lookup(&self, keymap: &Keymap, areas: &[Area]) -> Option<Action> {
        match self {
            Self::Press(press) => keymap.lookup(press, areas),
            Self::Chord(chord) => keymap.lookup_chord(chord, areas),
        }
    }

    /// The press is an auto-repeat of a held key (a video's window never reports repeats as such).
    pub fn repeat(&self) -> bool {
        matches!(self, Self::Press(press) if press.repeat)
    }

    /// The key text (Slint's code of a special key).
    pub fn key_text(&self) -> String {
        match self {
            Self::Press(press) => press.text.to_lowercase(),
            Self::Chord(chord) => chord.key().to_owned(),
        }
    }

    pub fn shift(&self) -> bool {
        match self {
            Self::Press(press) => press.shift,
            Self::Chord(chord) => chord.shift(),
        }
    }
}

/// What a window does with a key: nothing (let it go on), swallow it, or run an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    Pass,
    Swallow,
    Run(Action),
}

/// The action of `trigger` in the first of `areas`. While a text field has the keyboard (`typing`) only the actions that
/// work while typing act; a held key repeats only the actions that repeat (the others swallow the repeats).
pub fn resolve(trigger: &Trigger<'_>, areas: &[Area], typing: bool) -> Resolved {
    let Some(action) = trigger.lookup(&keymap(), areas) else {
        return Resolved::Pass;
    };
    if typing && !action.works_while_typing() {
        return Resolved::Pass;
    }
    if trigger.repeat() && !action.repeats() {
        return Resolved::Swallow;
    }
    Resolved::Run(action)
}

/// The keys every video window reports to `RustView` (mpv key name, chord text): those of the actions of the areas a video can
/// be in, except Esc (reported on its own), the fixed keys, the paging keys (paging never applies to a video) and every key
/// the video player area has (mpv keeps them until `RustView` controls the videos).
pub fn video_bindings(keymap: &Keymap) -> Vec<(String, String)> {
    let player: Vec<&Chord> = Action::ALL
        .iter()
        .filter(|a| a.area() == Area::Player)
        .flat_map(|a| keymap.chords(*a))
        .collect();
    let mut bindings: Vec<(String, String)> = Vec::new();
    for &action in Action::ALL {
        let reported =
            matches!(action.area(), Area::General | Area::Tab | Area::Fullscreen) || action == Action::BrowseRename;
        if !reported || action.is_fixed() || matches!(action, Action::FullscreenPrevious | Action::FullscreenNext) {
            continue;
        }
        for chord in keymap.chords(action) {
            if player.iter().any(|p| p.overlaps(chord)) {
                continue;
            }
            for name in mpv_keys::names(chord) {
                if !bindings.iter().any(|(n, _)| *n == name) {
                    bindings.push((name, chord.text().to_owned()));
                }
            }
        }
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_windows_report_the_keys_of_the_multiview_but_leave_the_player_keys_to_mpv() {
        let bindings = video_bindings(&Keymap::default());
        let has = |name: &str| bindings.iter().any(|(n, _)| n == name);
        for name in [
            "F2",
            "DEL",
            "Ctrl+3",
            "Ctrl+KP3",
            "Ctrl+s",
            "Shift+Ctrl+s",
            "Ctrl+w",
            "F11",
            "f",
            "BS",
            "Ctrl+BS",
        ] {
            assert!(has(name), "{name} missing: {bindings:?}");
        }
        for name in [
            "ESC",
            "SPACE",
            "m",
            "LEFT",
            "RIGHT",
            "UP",
            "DOWN",
            "PGUP",
            "Shift+LEFT",
            ",",
        ] {
            assert!(!has(name), "{name} must stay with mpv");
        }
        let chord_of = |name: &str| bindings.iter().find(|(n, _)| n == name).map(|(_, c)| c.clone());
        assert_eq!(chord_of("Ctrl+KP3").as_deref(), Some("Control+Digit3"));
    }

    #[test]
    fn typing_and_repeats() {
        let press = |text, control, repeat| {
            Trigger::Press(KeyPress {
                text,
                control,
                alt: false,
                shift: false,
                meta: false,
                repeat,
            })
        };
        let areas = [Area::General, Area::Tab];
        assert_eq!(
            resolve(&press("s", true, false), &areas, false),
            Resolved::Run(Action::TabSave)
        );
        assert_eq!(
            resolve(&press("s", true, false), &areas, true),
            Resolved::Pass,
            "typing"
        );
        assert_eq!(
            resolve(&press("w", true, false), &areas, true),
            Resolved::Run(Action::TabClose)
        );
        assert_eq!(
            resolve(&press("s", true, true), &areas, false),
            Resolved::Swallow,
            "no repeat"
        );
        assert_eq!(
            resolve(&press("z", true, true), &areas, false),
            Resolved::Run(Action::TabUndo)
        );
        assert_eq!(resolve(&press("q", true, false), &areas, false), Resolved::Pass);
    }
}
