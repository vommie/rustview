//! Key presses -> actions of the keymap (`ui::keymap`, `docs/hotkeys.md`).
//!
//! The windows keep their fixed handling in Slint (dialog locks, popups, Esc, type-ahead find) and hand every other key press
//! to Rust: the main window through [`main_window`], the fullscreen window through `ui::multiview::session_keys`. mpv has no window
//! of its own (videos are drawn by the Slint windows), so a key pressed over a video reaches the window like any other. This module keeps
//! the one keymap of the application: it is built from `settings.json` at startup and when the settings change.

pub mod main_window;

use crate::ui::keymap::{Action, Area, Chord, KeyPress, Keymap, table_from_json};
use crate::viewer::multiview::key_labels::{self, AbKeys, EditKeys};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

static KEYMAP: LazyLock<RwLock<Arc<Keymap>>> = LazyLock::new(|| RwLock::new(Arc::new(Keymap::default())));

/// The keymap in use.
pub fn keymap() -> Arc<Keymap> {
    KEYMAP.read().clone()
}

/// Builds the keymap from the stored `hotkeys` table (repairs are logged) and gives the labels of the media bar their keys.
pub fn apply(stored: &BTreeMap<String, serde_json::Value>) {
    let (keymap, notes) = Keymap::from_table(&table_from_json(stored));
    for note in &notes {
        tracing::warn!(note, "[Hotkeys] A stored key was repaired");
    }
    let edit_keys = |undo, redo, save, apply_crop| EditKeys {
        undo: keymap.first_label(undo),
        redo: keymap.first_label(redo),
        save: keymap.first_label(save),
        apply_crop: keymap.first_label(apply_crop),
    };
    key_labels::set(
        edit_keys(Action::TabUndo, Action::TabRedo, Action::TabSave, Action::TabApplyCrop),
        edit_keys(
            Action::FullscreenUndo,
            Action::FullscreenRedo,
            Action::FullscreenSave,
            Action::FullscreenApplyCrop,
        ),
    );
    key_labels::set_ab(AbKeys {
        set_a: keymap.first_label(Action::AbSetA),
        set_b: keymap.first_label(Action::AbSetB),
        clear: keymap.first_label(Action::AbClear),
    });
    tracing::info!(
        changed = stored.len(),
        repaired = notes.len(),
        "[Hotkeys] Keymap in use"
    );
    *KEYMAP.write() = Arc::new(keymap);
}

/// The first key of `action` as menus and messages show it ("" when it has none).
pub fn label(action: Action) -> String {
    keymap().first_label(action)
}

/// The shortcut column of a menu entry for `action` (`None` when it has no key).
pub fn shortcut(action: Action) -> Option<String> {
    Some(label(action)).filter(|l| !l.is_empty())
}

/// `text` followed by the first key of `action` in parentheses, or `text` alone when the action has no key.
pub fn with_key(text: &str, action: Action) -> String {
    match shortcut(action) {
        Some(key) => format!("{text} ({key})"),
        None => text.to_owned(),
    }
}

/// The rating keys of the fullscreen as a description names them: "Ctrl+0 (no rating) to Ctrl+5", or a pointer to the
/// Hotkeys page when either end has no key.
pub fn rating_keys_text() -> String {
    let (none, five) = (label(Action::FullscreenRate0), label(Action::FullscreenRate5));
    if none.is_empty() || five.is_empty() {
        "the rating keys (Settings > Hotkeys)".to_owned()
    } else {
        format!("{none} (no rating) to {five}")
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn printable_keys_are_never_actions_of_the_main_window() {
        let press = |text| {
            Trigger::Press(KeyPress {
                text,
                control: false,
                alt: false,
                shift: false,
                meta: false,
                repeat: false,
            })
        };
        let printable = ["f", "m", "o", "2", " ", "ä"];
        // While a text field has the keyboard a printable key is text, also where it is a key of the view (F, Space, M).
        for areas in [
            &[Area::General, Area::Browse][..],
            &[Area::General, Area::Tab],
            &[Area::General, Area::Player, Area::Tab],
        ] {
            for text in printable {
                assert_eq!(
                    resolve(&press(text), areas, true),
                    Resolved::Pass,
                    "typing {text:?} in {areas:?}"
                );
            }
        }
        // In Browse they belong to type-ahead find.
        for text in printable {
            assert_eq!(
                resolve(&press(text), &[Area::General, Area::Browse], false),
                Resolved::Pass,
                "{text:?}"
            );
        }
    }
}
