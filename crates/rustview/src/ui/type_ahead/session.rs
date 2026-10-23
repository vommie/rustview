//! The type-ahead search state machine (pure: no Slint, no list, no clock of its own).

use super::config::TypeAheadConfig;
use crate::ui::browse_folders::BrowseItem;
use std::time::Instant;

/// The names of the Browse list by list position.
pub trait NameSource {
    /// Number of list positions.
    fn count(&self) -> usize;
    /// Whether the item at `position` has a name that starts with `needle` (`needle` is lower case; "..." never matches).
    fn starts_with(&self, position: usize, needle: &str) -> bool;
}

/// What a key event is to the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClass<'a> {
    /// A character that can be part of a name.
    Text(&'a str),
    Backspace,
    Escape,
    /// A modifier key on its own (Shift, Ctrl, Alt, AltGr, Meta, Caps Lock): typing a capital letter presses Shift first, which
    /// must neither end nor extend the search.
    Modifier,
    /// Anything else (arrows, Enter, Tab, F-keys, shortcuts with Ctrl/Alt/Meta, ...): the search is over.
    Other,
}

/// `command` = Ctrl, Alt or Meta is held: the key is a shortcut and never part of a name (Shift is just a capital letter).
pub fn classify(text: &str, command: bool) -> KeyClass<'_> {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return KeyClass::Other;
    };
    if chars.next().is_none() {
        match first {
            '\u{10}'..='\u{18}' => return KeyClass::Modifier,
            _ if command => return KeyClass::Other,
            '\u{8}' => return KeyClass::Backspace,
            '\u{1b}' => return KeyClass::Escape,
            _ => {}
        }
    }
    if command || !text.chars().all(is_name_char) {
        return KeyClass::Other;
    }
    KeyClass::Text(text)
}

/// Control characters (Enter, Tab, Delete, Ctrl+letter) and the private-use code points Slint assigns to the arrow, function and
/// editing keys are no text.
fn is_name_char(c: char) -> bool {
    !c.is_control() && !matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFF}')
}

/// What a key did to the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// The key is none of the search's business: normal key handling goes on.
    NotHandled,
    /// The search took the key but nothing moves (search cancelled, empty list).
    Consumed,
    /// The search took the key and no name starts with the search string (see `TypeAhead::query`).
    NoMatch,
    /// Move the cursor to this list position (it may be the one it is on).
    Jump(usize),
}

/// Where the cursor is when a key arrives.
#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub item: Option<BrowseItem>,
    pub position: Option<usize>,
}

#[derive(Debug, Default)]
pub struct TypeAhead {
    /// The search string, lower case.
    buffer: String,
    /// The longest prefix of `buffer` that matched something (for runs of one key, see `cycle`).
    last_success: String,
    last_input: Option<Instant>,
    /// The item the cursor was left on by the last key. A different cursor now (click, arrow key, new folder, ...) means the user
    /// moved on and the next key starts a new search: no hook into the other code paths is needed.
    landed: Option<BrowseItem>,
}

impl TypeAhead {
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.last_success.clear();
        self.last_input = None;
        self.landed = None;
    }

    /// Tells the search where the cursor ended up after a key it handled.
    pub fn settle(&mut self, cursor: Option<BrowseItem>) {
        self.landed = cursor;
    }

    /// Length of the search string in characters (for logging; the text itself is a file name fragment and never logged).
    pub fn len(&self) -> usize {
        self.buffer.chars().count()
    }

    /// The search string (lower case).
    pub fn query(&self) -> &str {
        &self.buffer
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn is_running(&self, now: Instant, cursor: Option<BrowseItem>, config: &TypeAheadConfig) -> bool {
        !self.buffer.is_empty()
            && self.landed == cursor
            && self
                .last_input
                .is_some_and(|last| now.saturating_duration_since(last) <= config.timeout)
    }

    pub fn key(
        &mut self,
        class: KeyClass<'_>,
        now: Instant,
        cursor: Cursor,
        names: &dyn NameSource,
        config: &TypeAheadConfig,
    ) -> Reaction {
        match class {
            KeyClass::Modifier => Reaction::NotHandled,
            KeyClass::Other => {
                self.reset();
                Reaction::NotHandled
            }
            KeyClass::Escape => {
                let running = self.is_running(now, cursor.item, config);
                self.reset();
                if running {
                    Reaction::Consumed
                } else {
                    Reaction::NotHandled
                }
            }
            KeyClass::Backspace => {
                if !self.is_running(now, cursor.item, config) {
                    self.reset();
                    return Reaction::NotHandled;
                }
                self.buffer.pop();
                self.last_success.clear();
                if self.buffer.is_empty() {
                    self.reset();
                    return Reaction::Consumed;
                }
                self.last_input = Some(now);
                match find(names, &self.buffer, start_at(cursor.position, names.count(), false)) {
                    Some(position) => {
                        self.last_success.clone_from(&self.buffer);
                        Reaction::Jump(position)
                    }
                    None => Reaction::NoMatch,
                }
            }
            KeyClass::Text(text) => self.text(text, now, cursor, names, config),
        }
    }

    fn text(
        &mut self,
        text: &str,
        now: Instant,
        cursor: Cursor,
        names: &dyn NameSource,
        config: &TypeAheadConfig,
    ) -> Reaction {
        let running = self.is_running(now, cursor.item, config);
        let gap = self.last_input.map(|last| now.saturating_duration_since(last));
        if !running {
            self.reset();
        }
        // A space only belongs to a search that is running ("hallo 1"); it never starts one.
        if self.buffer.is_empty() && text.trim().is_empty() {
            return Reaction::NotHandled;
        }
        let key: String = text.chars().flat_map(char::to_lowercase).collect();
        self.buffer.push_str(&key);
        self.last_input = Some(now);

        let count = names.count();
        if count == 0 {
            return Reaction::Consumed;
        }
        let next = start_at(cursor.position, count, true);

        // 1. The whole string. A new search starts after the cursor (typing the same letter again moves on to the next item of
        //    that letter); a running one starts at the cursor, so refining the string keeps the item it is on while it still fits.
        let start = if running {
            start_at(cursor.position, count, false)
        } else {
            next
        };
        if let Some(position) = find(names, &self.buffer, start) {
            self.last_success.clone_from(&self.buffer);
            return Reaction::Jump(position);
        }

        if running {
            // 2. A run of one key ("aaa") that matches no name cycles through the names that start with it.
            if config.cycle_repeated_keys
                && let Some(position) = self.cycle(names, next)
            {
                return Reaction::Jump(position);
            }
            // 3. After a real pause the key starts a new search; typed quickly it is a typo inside a word and moves nothing.
            if gap.zip(config.new_search_gap).is_some_and(|(gap, limit)| gap > limit) {
                self.buffer = key;
                self.last_success.clear();
                if let Some(position) = find(names, &self.buffer, next) {
                    self.last_success.clone_from(&self.buffer);
                    return Reaction::Jump(position);
                }
            }
        }
        Reaction::NoMatch
    }

    fn cycle(&self, names: &dyn NameSource, next: usize) -> Option<usize> {
        let mut chars = self.buffer.chars();
        let first = chars.next()?;
        if self.buffer.chars().count() < 2 || !chars.all(|c| c == first) {
            return None;
        }
        // "444" after "44" matched: keep cycling the names that start with "44" instead of falling back to "4".
        let needle = if !self.last_success.is_empty() && self.buffer.starts_with(&self.last_success) {
            self.last_success.clone()
        } else {
            first.to_string()
        };
        find(names, &needle, next)
    }
}

/// The first position to look at: after the cursor (`after`) or on it; the start of the list without a cursor.
fn start_at(cursor: Option<usize>, count: usize, after: bool) -> usize {
    match cursor {
        Some(position) if count > 0 => (position + usize::from(after)) % count,
        _ => 0,
    }
}

/// The first position from `start` on (wrapping around, every position once) whose name starts with `needle`.
fn find(names: &dyn NameSource, needle: &str, start: usize) -> Option<usize> {
    let count = names.count();
    if count == 0 || needle.is_empty() {
        return None;
    }
    let start = start.min(count - 1);
    (start..count)
        .chain(0..start)
        .find(|&position| names.starts_with(position, needle))
}
