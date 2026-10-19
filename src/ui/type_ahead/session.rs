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
            && self.last_input.is_some_and(|last| now.saturating_duration_since(last) <= config.timeout)
    }

    pub fn key(&mut self, class: KeyClass<'_>, now: Instant, cursor: Cursor, names: &dyn NameSource, config: &TypeAheadConfig) -> Reaction {
        match class {
            KeyClass::Modifier => Reaction::NotHandled,
            KeyClass::Other => {
                self.reset();
                Reaction::NotHandled
            }
            KeyClass::Escape => {
                let running = self.is_running(now, cursor.item, config);
                self.reset();
                if running { Reaction::Consumed } else { Reaction::NotHandled }
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

    fn text(&mut self, text: &str, now: Instant, cursor: Cursor, names: &dyn NameSource, config: &TypeAheadConfig) -> Reaction {
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
        let start = if running { start_at(cursor.position, count, false) } else { next };
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
    (start..count).chain(0..start).find(|&position| names.starts_with(position, needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::models::MediaId;
    use std::time::Duration;

    struct Names(Vec<&'static str>);

    impl NameSource for Names {
        fn count(&self) -> usize {
            self.0.len()
        }
        fn starts_with(&self, position: usize, needle: &str) -> bool {
            self.0[position].to_lowercase().starts_with(needle)
        }
    }

    fn item(position: usize) -> BrowseItem {
        BrowseItem::Media(position as MediaId)
    }

    /// Types `keys` with `gap_ms` between them, moving the cursor as the reactions say; returns the final cursor position.
    struct Typist {
        search: TypeAhead,
        now: Instant,
        cursor: Option<usize>,
        config: TypeAheadConfig,
    }

    impl Typist {
        fn new(cursor: Option<usize>) -> Self {
            Self { search: TypeAhead::default(), now: Instant::now(), cursor, config: TypeAheadConfig::default() }
        }

        fn press(&mut self, class: KeyClass<'_>, gap_ms: u64, names: &Names) -> Reaction {
            self.now += Duration::from_millis(gap_ms);
            let cursor = Cursor { item: self.cursor.map(item), position: self.cursor };
            let reaction = self.search.key(class, self.now, cursor, names, &self.config);
            if let Reaction::Jump(position) = reaction {
                self.cursor = Some(position);
            }
            self.search.settle(self.cursor.map(item));
            reaction
        }

        fn type_str(&mut self, text: &str, gap_ms: u64, names: &Names) {
            for c in text.chars() {
                let s = c.to_string();
                self.press(classify(&s, false), gap_ms, names);
            }
        }
    }

    fn files() -> Names {
        Names(vec!["alpha.jpg", "Apple.png", "avocado.png", "hallo 1.jpg", "hallo 2.jpg", "hallo 123.jpg", "zebra.jpg", "Ärger.jpg", "44a", "444b", "5"])
    }

    #[test]
    fn typing_a_word_jumps_to_its_first_name() {
        let names = files();
        let mut t = Typist::new(Some(0));
        t.type_str("hallo 12", 120, &names);
        assert_eq!(t.cursor, Some(5));
    }

    #[test]
    fn refining_keeps_the_current_item_while_it_fits() {
        let names = files();
        let mut t = Typist::new(Some(0));
        t.type_str("hallo", 100, &names);
        assert_eq!(t.cursor, Some(3));
        t.type_str(" 2", 100, &names);
        assert_eq!(t.cursor, Some(4));
    }

    #[test]
    fn a_new_search_starts_after_the_cursor_and_wraps() {
        let names = files();
        let mut t = Typist::new(Some(1));
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(2));
        t.type_str("a", 1500, &names);
        assert_eq!(t.cursor, Some(0), "wraps around to alpha");
    }

    #[test]
    fn same_letter_cycles() {
        let names = files();
        let mut t = Typist::new(None);
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(0));
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(1));
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(2));
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(0), "cycles around");
    }

    #[test]
    fn runs_of_digits_keep_their_prefix() {
        let names = files();
        let mut t = Typist::new(None);
        t.type_str("44", 100, &names);
        assert_eq!(t.cursor, Some(8));
        t.type_str("4", 100, &names);
        assert_eq!(t.cursor, Some(9));
        t.type_str("4", 100, &names);
        assert_eq!(t.cursor, Some(9), "keeps the longest prefix that matched instead of falling back to 4");
    }

    #[test]
    fn pause_starts_a_new_search_but_a_fast_typo_does_not() {
        let names = files();
        let mut t = Typist::new(None);
        t.type_str("ha", 100, &names);
        assert_eq!(t.cursor, Some(3));
        t.type_str("z", 700, &names);
        assert_eq!(t.cursor, Some(6), "slow key that fits nothing starts a new search");

        let mut t = Typist::new(None);
        t.type_str("ha", 100, &names);
        t.type_str("z", 100, &names);
        assert_eq!(t.cursor, Some(3), "fast key that fits nothing is a typo");
        t.press(KeyClass::Backspace, 100, &names);
        t.type_str("l", 100, &names);
        assert_eq!(t.cursor, Some(3));
    }

    #[test]
    fn timeout_and_moved_cursor_end_the_search() {
        let names = files();
        let mut t = Typist::new(None);
        t.type_str("ha", 100, &names);
        t.type_str("l", 1200, &names);
        assert_eq!(t.cursor, Some(3), "new search 'l' finds nothing, nothing moves");

        let mut t = Typist::new(None);
        t.type_str("h", 100, &names);
        t.cursor = Some(10);
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(0), "cursor moved by someone else: 'a' is a new search");
    }

    #[test]
    fn backspace_and_escape_only_while_a_search_runs() {
        let names = files();
        let mut t = Typist::new(None);
        assert_eq!(t.press(KeyClass::Backspace, 100, &names), Reaction::NotHandled);
        assert_eq!(t.press(KeyClass::Escape, 100, &names), Reaction::NotHandled);
        t.type_str("hal", 100, &names);
        assert!(matches!(t.press(KeyClass::Backspace, 100, &names), Reaction::Jump(3)));
        assert_eq!(t.press(KeyClass::Escape, 100, &names), Reaction::Consumed);
        assert_eq!(t.press(KeyClass::Escape, 100, &names), Reaction::NotHandled);
        t.type_str("h", 100, &names);
        assert_eq!(t.press(KeyClass::Backspace, 100, &names), Reaction::Consumed, "empties and ends the search");
        assert_eq!(t.press(KeyClass::Backspace, 100, &names), Reaction::NotHandled, "now it goes to the parent folder");
    }

    #[test]
    fn space_never_starts_a_search_and_modifiers_do_not_break_one() {
        let names = files();
        let mut t = Typist::new(None);
        assert_eq!(t.press(classify(" ", false), 100, &names), Reaction::NotHandled);
        t.type_str("h", 100, &names);
        assert_eq!(t.press(classify("\u{10}", false), 100, &names), Reaction::NotHandled);
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(3));
        assert!(t.search.len() == 2);
    }

    #[test]
    fn capitals_umlauts_and_unicode() {
        let names = files();
        let mut t = Typist::new(None);
        t.type_str("Ä", 100, &names);
        assert_eq!(t.cursor, Some(7));
    }

    #[test]
    fn options_change_the_timing_and_the_cycling() {
        let names = files();
        let mut t = Typist::new(None);
        t.config.timeout = Duration::from_millis(300);
        t.type_str("ha", 100, &names);
        t.type_str("l", 400, &names);
        assert_eq!(t.cursor, Some(3), "400 ms is past the 300 ms timeout: 'l' is a new search and finds nothing");
        assert_eq!(t.search.len(), 1);

        let mut t = Typist::new(None);
        t.config.new_search_gap = None;
        t.type_str("ha", 100, &names);
        t.type_str("z", 900, &names);
        assert_eq!(t.cursor, Some(3), "without a new-search gap a slow key that fits nothing stays a typo");

        let mut t = Typist::new(None);
        t.config.cycle_repeated_keys = false;
        t.type_str("a", 100, &names);
        t.type_str("a", 100, &names);
        assert_eq!(t.cursor, Some(0), "'aa' matches nothing and does not cycle on");
    }

    #[test]
    fn nothing_found_is_reported_with_the_search_string() {
        let names = files();
        let mut t = Typist::new(None);
        t.type_str("ha", 100, &names);
        assert_eq!(t.press(classify("z", false), 100, &names), Reaction::NoMatch);
        assert_eq!(t.search.query(), "haz");
        let mut t = Typist::new(None);
        assert_eq!(t.press(classify("q", false), 100, &names), Reaction::NoMatch);
        assert_eq!(t.press(KeyClass::Backspace, 100, &names), Reaction::Consumed, "empty search string ends the search");
    }

    #[test]
    fn classification() {
        assert_eq!(classify("a", false), KeyClass::Text("a"));
        assert_eq!(classify("A", false), KeyClass::Text("A"));
        assert_eq!(classify("a", true), KeyClass::Other);
        assert_eq!(classify("\u{1}", false), KeyClass::Other, "Ctrl+A");
        assert_eq!(classify("\n", false), KeyClass::Other);
        assert_eq!(classify("\t", false), KeyClass::Other);
        assert_eq!(classify("\u{7f}", false), KeyClass::Other);
        assert_eq!(classify("\u{F700}", false), KeyClass::Other, "arrow up");
        assert_eq!(classify("\u{F704}", false), KeyClass::Other, "F1");
        assert_eq!(classify("\u{8}", false), KeyClass::Backspace);
        assert_eq!(classify("\u{1b}", false), KeyClass::Escape);
        assert_eq!(classify("\u{14}", false), KeyClass::Modifier);
        assert_eq!(classify("\u{15}", true), KeyClass::Modifier);
        assert_eq!(classify("", false), KeyClass::Other);
        assert_eq!(classify("€", false), KeyClass::Text("€"));
        assert_eq!(classify(" ", false), KeyClass::Text(" "));
    }

    #[test]
    fn empty_list_and_single_item() {
        let empty = Names(vec![]);
        let mut t = Typist::new(None);
        assert_eq!(t.press(classify("a", false), 100, &empty), Reaction::Consumed);
        let one = Names(vec!["abc"]);
        let mut t = Typist::new(Some(0));
        assert_eq!(t.press(classify("a", false), 100, &one), Reaction::Jump(0));
    }
}
