//! The page Hotkeys (pure): every action of the keymap with its keys, grouped by area, the filter and "Only changed", and
//! recording a key. A key another action of an overlapping area has is not taken at once: the row names that action, and
//! pressing the same key again moves it. Browse refuses keys that type a character (they belong to type-ahead find) and
//! nothing takes a fixed key. What is stored keeps only the actions whose keys differ from their defaults (`docs/hotkeys.md`).

use crate::ui::keymap::{Action, Area, Chord, KeyPress, Keymap, KeymapTable};

/// One action of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub action: Action,
    /// (stored text, label) per key; label actions (type-ahead find) have their labels as both.
    pub keys: Vec<(String, String)>,
    pub changed: bool,
    /// The default keys' labels, joined.
    pub defaults: String,
}

/// One area with the rows that pass the filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub area: Area,
    /// The section shows its rows (collapsed sections open while a filter or "Only changed" is on).
    pub open: bool,
    pub rows: Vec<Row>,
}

/// A line under one row: what happened to the last key pressed for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub action: Action,
    pub text: String,
    /// An error (the key cannot be used) rather than a hint.
    pub error: bool,
}

/// What a key press while recording did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// A modifier alone, or a key without a name: still waiting.
    Waiting,
    /// Esc: recording ended without a key.
    Cancelled,
    Bound(String),
    /// The key belongs to `other`; the same key again moves it.
    Taken {
        chord: String,
        other: Action,
    },
    Moved {
        chord: String,
        from: Action,
    },
    /// The key cannot be used here (a fixed key, or a typing key in Browse); still waiting for another one.
    Refused(String),
}

#[derive(Debug, Clone, Default)]
pub struct HotkeysPage {
    pub filter: String,
    pub only_changed: bool,
    /// The ids of the collapsed areas.
    pub collapsed: Vec<String>,
    /// The action waiting for a key.
    pub recording: Option<Action>,
    /// The key that was refused for being another action's: pressing it again moves it.
    pending_move: Option<String>,
    pub note: Option<Note>,
}

/// The parsed default keys of `action`.
fn default_chords(action: Action) -> Vec<Chord> {
    if action.has_label_keys() {
        return Vec::new();
    }
    action
        .default_keys()
        .iter()
        .filter_map(|k| Chord::parse(k).ok())
        .collect()
}

fn texts(chords: &[Chord]) -> Vec<&str> {
    chords.iter().map(Chord::text).collect()
}

fn is_changed(keymap: &Keymap, action: Action) -> bool {
    !action.is_fixed() && texts(keymap.chords(action)) != texts(&default_chords(action))
}

/// The row of `action`.
pub fn row(keymap: &Keymap, action: Action) -> Row {
    let keys = if action.has_label_keys() {
        keymap.labels(action).into_iter().map(|l| (l.clone(), l)).collect()
    } else {
        keymap
            .chords(action)
            .iter()
            .map(|c| (c.text().to_owned(), c.label()))
            .collect()
    };
    Row {
        action,
        keys,
        changed: is_changed(keymap, action),
        defaults: default_chords(action)
            .iter()
            .map(Chord::label)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The table for `settings.json`: only the actions whose keys differ from their defaults.
pub fn table(keymap: &Keymap) -> KeymapTable {
    keymap
        .to_table()
        .into_iter()
        .filter(|(id, _)| Action::from_id(id).is_some_and(|a| is_changed(keymap, a)))
        .collect()
}

/// Puts `action`'s default keys back; a default another action uses now stays with that action. Returns those keys.
pub fn reset(keymap: &mut Keymap, action: Action) -> Vec<(String, Action)> {
    let current: Vec<String> = keymap.chords(action).iter().map(|c| c.text().to_owned()).collect();
    for text in &current {
        keymap.unbind(action, text);
    }
    default_chords(action)
        .into_iter()
        .filter_map(|chord| {
            let label = chord.label();
            keymap.bind(action, chord).err().map(|conflict| (label, conflict.other))
        })
        .collect()
}

/// The name of an action with its area, as the notes write it.
fn named(action: Action) -> String {
    format!("\"{}\" ({})", action.title(), action.area().title())
}

impl HotkeysPage {
    pub fn new(filter: String, only_changed: bool, collapsed: Vec<String>) -> Self {
        Self {
            filter,
            only_changed,
            collapsed,
            ..Self::default()
        }
    }

    /// The sections with the rows that match the filter (title, area or a key; case-insensitive) and "Only changed".
    pub fn sections(&self, keymap: &Keymap) -> Vec<Section> {
        let needle = self.filter.trim().to_lowercase();
        let narrowing = !needle.is_empty() || self.only_changed;
        Area::ALL
            .into_iter()
            .filter_map(|area| {
                let rows: Vec<Row> = Action::ALL
                    .iter()
                    .filter(|a| a.area() == area)
                    .map(|a| row(keymap, *a))
                    .filter(|row| !self.only_changed || row.changed)
                    .filter(|row| {
                        needle.is_empty()
                            || row.action.title().to_lowercase().contains(&needle)
                            || area.title().to_lowercase().contains(&needle)
                            || row.keys.iter().any(|(_, label)| label.to_lowercase().contains(&needle))
                    })
                    .collect();
                (!rows.is_empty() || !narrowing).then(|| Section {
                    area,
                    open: narrowing || !self.collapsed.iter().any(|id| id == area.id()),
                    rows,
                })
            })
            .collect()
    }

    pub fn toggle_section(&mut self, area: Area) {
        if let Some(i) = self.collapsed.iter().position(|id| id == area.id()) {
            self.collapsed.remove(i);
        } else {
            self.collapsed.push(area.id().to_owned());
        }
    }

    pub fn start(&mut self, action: Action) {
        if action.is_fixed() {
            return;
        }
        self.recording = Some(action);
        self.pending_move = None;
        self.note = None;
    }

    pub fn cancel(&mut self) {
        self.recording = None;
        self.pending_move = None;
    }

    fn refuse(&mut self, action: Action, text: String) -> Recorded {
        self.pending_move = None;
        self.note = Some(Note {
            action,
            text: text.clone(),
            error: true,
        });
        Recorded::Refused(text)
    }

    /// A key press while recording: binds it, names the action that has it, moves it (pressed again) or refuses it.
    pub fn press(&mut self, keymap: &mut Keymap, press: &KeyPress<'_>) -> Recorded {
        let Some(action) = self.recording else {
            return Recorded::Waiting;
        };
        let escape = slint::SharedString::from(slint::platform::Key::Escape);
        if press.text == escape.as_str() && !press.control && !press.alt && !press.meta {
            self.cancel();
            return Recorded::Cancelled;
        }
        let Some(chord) = Chord::of_press(press) else {
            return Recorded::Waiting;
        };
        let label = chord.label();
        if action.area() == Area::Browse && chord.is_plain_printable() {
            return self.refuse(
                action,
                format!(
                    "{label} cannot be used in Browse: keys that type a character belong to type-ahead find. Use a key with \
                     Ctrl, Alt or Meta, or a function key."
                ),
            );
        }
        let text = chord.text().to_owned();
        match keymap.conflict(&chord, action) {
            Some(other) if other.is_fixed() => self.refuse(
                action,
                format!("{label} cannot be used: it is a fixed key of {}.", named(other)),
            ),
            None => {
                // A key the action has already is not added twice; recording ends either way.
                let _ = keymap.bind(action, chord);
                self.cancel();
                self.note = None;
                Recorded::Bound(text)
            }
            Some(other) if self.pending_move.as_deref() == Some(text.as_str()) => {
                let taken: Vec<String> = keymap
                    .chords(other)
                    .iter()
                    .filter(|c| c.overlaps(&chord))
                    .map(|c| c.text().to_owned())
                    .collect();
                for t in &taken {
                    keymap.unbind(other, t);
                }
                let _ = keymap.bind(action, chord);
                self.cancel();
                self.note = Some(Note {
                    action,
                    text: format!("{label} moved here from {}.", named(other)),
                    error: false,
                });
                Recorded::Moved {
                    chord: text,
                    from: other,
                }
            }
            Some(other) => {
                self.pending_move = Some(text.clone());
                self.note = Some(Note {
                    action,
                    text: format!(
                        "{label} is used by {}. Press {label} again to move it here.",
                        named(other)
                    ),
                    error: false,
                });
                Recorded::Taken { chord: text, other }
            }
        }
    }

    /// The reset button of a row: the defaults come back; a note names the defaults another action keeps.
    pub fn reset(&mut self, keymap: &mut Keymap, action: Action) {
        self.cancel();
        let kept = reset(keymap, action);
        self.note = (!kept.is_empty()).then(|| Note {
            action,
            text: kept
                .iter()
                .map(|(label, other)| {
                    format!(
                        "{label} stays with {}; it comes back once that action lets it go.",
                        named(*other)
                    )
                })
                .collect::<Vec<_>>()
                .join(" "),
            error: true,
        });
    }

    pub fn reset_all(&mut self, keymap: &mut Keymap) {
        self.cancel();
        self.note = None;
        *keymap = Keymap::default();
    }

    pub fn unbind(&mut self, keymap: &mut Keymap, action: Action, text: &str) {
        self.cancel();
        self.note = None;
        keymap.unbind(action, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::Key;

    fn press(text: &str, control: bool) -> KeyPress<'_> {
        KeyPress {
            text,
            control,
            alt: false,
            shift: false,
            meta: false,
            repeat: false,
        }
    }

    fn special(key: Key) -> String {
        char::from(key).to_string()
    }

    #[test]
    fn a_taken_key_moves_on_the_second_press_and_only_changes_are_stored() {
        let mut keymap = Keymap::default();
        let mut page = HotkeysPage::default();
        assert!(table(&keymap).is_empty());

        let f6 = special(Key::F6);
        page.start(Action::TabRename);
        assert_eq!(
            page.press(&mut keymap, &press(&f6, false)),
            Recorded::Bound("F6".into())
        );
        assert_eq!(page.recording, None);

        let f2 = special(Key::F2);
        page.start(Action::TabRefresh);
        assert_eq!(
            page.press(&mut keymap, &press(&f2, false)),
            Recorded::Taken {
                chord: "F2".into(),
                other: Action::TabRename
            }
        );
        assert_eq!(page.recording, Some(Action::TabRefresh));
        assert!(page.note.as_ref().is_some_and(|n| n.text.contains("Press F2 again")));
        assert_eq!(
            page.press(&mut keymap, &press(&f2, false)),
            Recorded::Moved {
                chord: "F2".into(),
                from: Action::TabRename
            }
        );
        assert_eq!(keymap.labels(Action::TabRename), vec!["F6".to_owned()]);
        assert_eq!(
            keymap.labels(Action::TabRefresh),
            vec!["F5".to_owned(), "F2".to_owned()]
        );
        // Browse keeps its own F2: another area.
        assert_eq!(keymap.labels(Action::BrowseRename), vec!["F2".to_owned()]);

        let stored = table(&keymap);
        assert_eq!(stored.len(), 2, "{stored:?}");
        let (again, notes) = Keymap::from_table(&stored);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(again, keymap);

        // F2 is the refresh key now: Rename cannot get it back until Refresh lets it go.
        page.reset(&mut keymap, Action::TabRename);
        assert!(page.note.as_ref().is_some_and(|n| n.error));
        page.reset(&mut keymap, Action::TabRefresh);
        page.reset(&mut keymap, Action::TabRename);
        assert_eq!(page.note, None);
        assert!(table(&keymap).is_empty());
    }

    #[test]
    fn browse_refuses_typing_keys_and_nobody_takes_a_fixed_key() {
        let mut keymap = Keymap::default();
        let mut page = HotkeysPage::default();
        page.start(Action::BrowseRefresh);
        assert!(matches!(
            page.press(&mut keymap, &press("r", false)),
            Recorded::Refused(_)
        ));
        assert_eq!(page.recording, Some(Action::BrowseRefresh), "still waiting");
        let up = special(Key::UpArrow);
        assert!(matches!(
            page.press(&mut keymap, &press(&up, false)),
            Recorded::Refused(_)
        ));
        assert!(
            matches!(page.press(&mut keymap, &press(&up, false)), Recorded::Refused(_)),
            "pressing again does not move it"
        );
        assert_eq!(
            page.press(&mut keymap, &press("r", true)),
            Recorded::Bound("Control+R".into())
        );
        // A plain letter is fine outside Browse.
        page.start(Action::TabSave);
        assert_eq!(page.press(&mut keymap, &press("s", false)), Recorded::Bound("S".into()));
        // Esc cancels.
        page.start(Action::TabSave);
        assert_eq!(
            page.press(&mut keymap, &press(&special(Key::Escape), false)),
            Recorded::Cancelled
        );
        assert_eq!(page.recording, None);
        // Fixed rows cannot record.
        page.start(Action::BrowseCursor);
        assert_eq!(page.recording, None);
    }

    #[test]
    fn the_filter_and_only_changed_narrow_the_sections() {
        let mut keymap = Keymap::default();
        let mut page = HotkeysPage::new(String::new(), false, vec!["player".into()]);
        let all = page.sections(&keymap);
        assert_eq!(all.len(), Area::ALL.len());
        assert!(!all.iter().find(|s| s.area == Area::Player).unwrap().open, "collapsed");

        page.filter = "f2".into();
        let found = page.sections(&keymap);
        let areas: Vec<Area> = found.iter().map(|s| s.area).collect();
        assert_eq!(areas, vec![Area::Browse, Area::Tab, Area::Fullscreen]);
        assert!(found.iter().all(|s| s.open && s.rows.len() == 1));

        page.filter = "video player".into();
        assert_eq!(
            page.sections(&keymap).first().map(|s| s.open),
            Some(true),
            "a filter opens collapsed sections"
        );

        page.filter.clear();
        page.only_changed = true;
        assert_eq!(page.sections(&keymap), Vec::new());
        keymap.unbind(Action::FullscreenClose, "F");
        let changed = page.sections(&keymap);
        assert_eq!(changed.len(), 1);
        assert_eq!(
            changed.first().unwrap().rows.first().unwrap().action,
            Action::FullscreenClose
        );
        assert!(changed.first().unwrap().rows.first().unwrap().changed);

        page.toggle_section(Area::Player);
        assert_eq!(page.collapsed, Vec::<String>::new());
    }
}
