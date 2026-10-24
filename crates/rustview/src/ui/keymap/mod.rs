//! The configurable keymap (`docs/hotkeys.md`): which keys trigger which [`Action`], read from the `hotkeys` table of
//! `settings.json` (the core stores it as plain JSON values; this module validates and repairs it), with conflict detection
//! between areas that are active at the same time. Modeled on `RustCut`'s `ui/keymap/`.
//!
//! The windows send every key press that their fixed handling (dialog locks, Esc, type-ahead find) leaves to
//! `ui::hotkeys`, which looks it up here for the areas of the view that has the keyboard. The editing UI is the Settings page
//! Hotkeys (`ui::settings_dialog::hotkeys_page`).

mod action;
mod area;
mod chord;
pub mod mpv_keys;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

pub use action::Action;
pub use area::Area;
pub use chord::{Chord, ChordError, KeyPress};

/// The raw table: action id -> key texts (`None` = a value that was not a list of strings).
pub type KeymapTable = BTreeMap<String, Option<Vec<String>>>;

/// A chord that is already bound to another action of an area that is active at the same time.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{chord} is already used by \"{}\" ({})", other.title(), other.area().title())]
pub struct Conflict {
    pub chord: String,
    pub other: Action,
}

/// The bindings of every action, in [`Action::ALL`] order. No two chords of actions that exclude each other overlap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<(Action, Vec<Chord>)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_table(&KeymapTable::new()).0
    }
}

impl Keymap {
    /// Builds the keymap from the stored table and returns a note for every repair (logged at startup): unknown actions,
    /// fixed actions and keys that do not parse are dropped, an unreadable value keeps the default, and a key bound twice
    /// stays with the action the user configured (fixed keys first, then configured actions, then defaults, each in
    /// [`Action::ALL`] order).
    pub fn from_table(table: &KeymapTable) -> (Self, Vec<String>) {
        let mut notes: Vec<String> = Vec::new();
        for id in table.keys() {
            match Action::from_id(id) {
                None => notes.push(format!("unknown action `{id}` ignored")),
                Some(action) if action.is_fixed() => notes.push(format!("`{id}` cannot be changed; ignored")),
                Some(_) => {}
            }
        }
        // (action, keys, rank): 0 = fixed, 1 = configured, 2 = default.
        let mut wanted: Vec<(Action, Vec<String>, u8)> = Vec::with_capacity(Action::ALL.len());
        for &action in Action::ALL {
            if action.has_label_keys() {
                continue;
            }
            let defaults = || action.default_keys().iter().map(|k| (*k).to_owned()).collect();
            if action.is_fixed() {
                wanted.push((action, defaults(), 0));
                continue;
            }
            match table.get(action.id()) {
                Some(Some(texts)) => wanted.push((action, texts.clone(), 1)),
                Some(None) => {
                    notes.push(format!(
                        "{}: the value is not a list of keys; defaults used",
                        action.id()
                    ));
                    wanted.push((action, defaults(), 2));
                }
                None => wanted.push((action, defaults(), 2)),
            }
        }
        let mut keymap = Self {
            bindings: Action::ALL.iter().map(|a| (*a, Vec::new())).collect(),
        };
        for rank in 0..=2 {
            for (action, texts, _) in wanted.iter().filter(|(_, _, r)| *r == rank) {
                for text in texts {
                    let chord = match Chord::parse(text) {
                        Ok(chord) => chord,
                        Err(e) => {
                            notes.push(format!("{}: {e}", action.id()));
                            continue;
                        }
                    };
                    if let Err(conflict) = keymap.bind(*action, chord) {
                        notes.push(format!("{}: {conflict}; dropped", action.id()));
                    }
                }
            }
        }
        (keymap, notes)
    }

    /// The table to store: every action that can be changed, with its keys (an empty list = unbound on purpose).
    pub fn to_table(&self) -> KeymapTable {
        self.bindings
            .iter()
            .filter(|(action, _)| !action.is_fixed() && !action.has_label_keys())
            .map(|(action, chords)| {
                (
                    action.id().to_owned(),
                    Some(chords.iter().map(|c| c.text().to_owned()).collect()),
                )
            })
            .collect()
    }

    pub fn chords(&self, action: Action) -> &[Chord] {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, chords)| chords.as_slice())
    }

    /// The action of the first of `areas` that `press` triggers.
    pub fn lookup(&self, press: &KeyPress<'_>, areas: &[Area]) -> Option<Action> {
        self.find_in(areas, |chord| chord.matches(press))
    }

    /// The action of the first of `areas` that has a chord overlapping `chord` (a key reported by a video's window).
    pub fn lookup_chord(&self, chord: &Chord, areas: &[Area]) -> Option<Action> {
        self.find_in(areas, |c| c.overlaps(chord))
    }

    fn find_in(&self, areas: &[Area], hit: impl Fn(&Chord) -> bool) -> Option<Action> {
        areas.iter().find_map(|area| {
            self.bindings
                .iter()
                .filter(|(action, _)| action.area() == *area)
                .find(|(_, chords)| chords.iter().any(&hit))
                .map(|(action, _)| *action)
        })
    }

    /// The action that excludes `except` and already has a chord overlapping `chord`.
    pub fn conflict(&self, chord: &Chord, except: Action) -> Option<Action> {
        self.bindings
            .iter()
            .filter(|(a, _)| except.excludes(*a))
            .find(|(_, chords)| chords.iter().any(|c| c.overlaps(chord)))
            .map(|(action, _)| *action)
    }

    /// Adds `chord` to `action` (a chord it already has is not added twice); refused when an action it excludes has it.
    pub fn bind(&mut self, action: Action, chord: Chord) -> Result<(), Conflict> {
        if let Some(other) = self.conflict(&chord, action) {
            return Err(Conflict {
                chord: chord.label(),
                other,
            });
        }
        if let Some((_, chords)) = self.bindings.iter_mut().find(|(a, _)| *a == action)
            && !chords.iter().any(|c| c.overlaps(&chord))
        {
            chords.push(chord);
        }
        Ok(())
    }

    /// Removes the chord with `text` from `action`; `false` when it had none (or the action is fixed).
    pub fn unbind(&mut self, action: Action, text: &str) -> bool {
        if action.is_fixed() {
            return false;
        }
        let Some((_, chords)) = self.bindings.iter_mut().find(|(a, _)| *a == action) else {
            return false;
        };
        let before = chords.len();
        chords.retain(|c| c.text() != text);
        chords.len() != before
    }

    /// Every pair of actions that exclude each other but share a key (empty for a keymap built by
    /// [`Keymap::from_table`] and [`Keymap::bind`]).
    pub fn conflicts(&self) -> Vec<(Action, Action)> {
        let mut pairs = Vec::new();
        for (i, (a, chords_a)) in self.bindings.iter().enumerate() {
            for (b, chords_b) in self.bindings.iter().skip(i + 1) {
                if a.excludes(*b) && chords_a.iter().any(|x| chords_b.iter().any(|y| x.overlaps(y))) {
                    pairs.push((*a, *b));
                }
            }
        }
        pairs
    }

    /// The keys of `action` as the page and the tooltips show them (the labels of label actions).
    pub fn labels(&self, action: Action) -> Vec<String> {
        if action.has_label_keys() {
            return action.default_keys().iter().map(|k| (*k).to_owned()).collect();
        }
        self.chords(action).iter().map(Chord::label).collect()
    }

    /// The first key of `action` as a menu row or a message shows it ("" when unbound).
    pub fn first_label(&self, action: Action) -> String {
        self.labels(action).into_iter().next().unwrap_or_default()
    }

    /// Every key of `action` joined by " or " (tooltips; "" when unbound).
    pub fn tooltip_keys(&self, action: Action) -> String {
        self.labels(action).join(" or ")
    }
}

/// The table as `settings.json` keeps it (`hotkeys`: id -> any JSON value, so a hand-edited file never breaks loading).
pub fn table_from_json(stored: &BTreeMap<String, serde_json::Value>) -> KeymapTable {
    stored
        .iter()
        .map(|(id, value)| {
            let keys = value.as_array().and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            });
            (id.clone(), keys)
        })
        .collect()
}

/// The table for `settings.json`.
pub fn table_to_json(table: &KeymapTable) -> BTreeMap<String, serde_json::Value> {
    table
        .iter()
        .filter_map(|(id, keys)| {
            keys.as_ref()
                .map(|keys| (id.clone(), serde_json::Value::from(keys.clone())))
        })
        .collect()
}
