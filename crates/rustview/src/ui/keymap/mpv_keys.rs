//! The names mpv uses for the keys of a chord (`input.conf` syntax, `~/claude/docs/mpv/src/DOCS/man/input.rst`), so a
//! video's window can report the keys of `RustView`'s actions as `script-message rustview-key <chord>`.

use super::chord::Chord;
use slint::SharedString;
use slint::platform::Key;

/// mpv's names of Slint's special keys (the keypad variants are added for digits, `+`, `-` and Enter).
fn special_name(key: &str) -> Option<&'static [&'static str]> {
    let named: [(Key, &'static [&'static str]); 27] = [
        (Key::Backspace, &["BS"]),
        (Key::Tab, &["TAB"]),
        (Key::Return, &["ENTER", "KP_ENTER"]),
        (Key::Escape, &["ESC"]),
        (Key::Delete, &["DEL", "KP_DEL"]),
        (Key::Space, &["SPACE"]),
        (Key::UpArrow, &["UP"]),
        (Key::DownArrow, &["DOWN"]),
        (Key::LeftArrow, &["LEFT"]),
        (Key::RightArrow, &["RIGHT"]),
        (Key::Insert, &["INS", "KP_INS"]),
        (Key::Home, &["HOME"]),
        (Key::End, &["END"]),
        (Key::PageUp, &["PGUP"]),
        (Key::PageDown, &["PGDWN"]),
        (Key::F1, &["F1"]),
        (Key::F2, &["F2"]),
        (Key::F3, &["F3"]),
        (Key::F4, &["F4"]),
        (Key::F5, &["F5"]),
        (Key::F6, &["F6"]),
        (Key::F7, &["F7"]),
        (Key::F8, &["F8"]),
        (Key::F9, &["F9"]),
        (Key::F10, &["F10"]),
        (Key::F11, &["F11"]),
        (Key::F12, &["F12"]),
    ];
    named
        .iter()
        .find(|(k, _)| SharedString::from(*k).as_str() == key)
        .map(|(_, names)| *names)
}

/// Every mpv key name that stands for `chord` (empty for a key mpv cannot name, e.g. a modifier alone).
pub fn names(chord: &Chord) -> Vec<String> {
    let key = chord.key();
    let bases: Vec<String> = if let Some(names) = special_name(key) {
        names.iter().map(|n| (*n).to_owned()).collect()
    } else {
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Vec::new();
        };
        if (0xE000..=0xF8FF).contains(&u32::from(c)) {
            return Vec::new();
        }
        let mut list = vec![c.to_string()];
        match c {
            '0'..='9' => list.push(format!("KP{c}")),
            '+' => list.push("KP_ADD".to_owned()),
            '-' => list.push("KP_SUBTRACT".to_owned()),
            '*' => list.push("KP_MULTIPLY".to_owned()),
            '/' => list.push("KP_DIVIDE".to_owned()),
            _ => {}
        }
        list
    };
    let mut prefix = String::new();
    for (on, name) in [
        (chord.shift(), "Shift+"),
        (chord.control(), "Ctrl+"),
        (chord.alt(), "Alt+"),
        (chord.meta(), "Meta+"),
    ] {
        if on {
            prefix.push_str(name);
        }
    }
    bases.into_iter().map(|base| format!("{prefix}{base}")).collect()
}
