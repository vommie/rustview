//! One key combination of the keymap, written with Slint's key names (`Control+S`, `Shift+LeftArrow`, `Home`, `Plus`).
//! Slint parses it ([`slint::Keys::from_parts`]): the same names as the `@keys` macro, layout-dependent keys like `Plus`
//! or `Comma` ignore Shift, and the parsed [`slint::Keys`] gives the label menus and tooltips show. Matching a key press
//! follows Slint's own rule (lowercased text, equal modifiers apart from the ignored ones). Ported from `RustCut`'s
//! `ui/keymap/chord.rs`.

use std::fmt;

use slint::Keys;
use slint::platform::Key;

/// How a chord treats one of the modifiers that can be optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Modifier {
    Off,
    On,
    /// Matches with and without (`Shift?`, and Shift for layout-dependent keys).
    Any,
}

impl Modifier {
    fn accepts(self, pressed: bool) -> bool {
        match self {
            Self::Off => !pressed,
            Self::On => pressed,
            Self::Any => true,
        }
    }

    fn overlaps(self, other: Self) -> bool {
        self == Self::Any || other == Self::Any || self == other
    }
}

/// What a key press reports (the fields of Slint's `KeyEvent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress<'a> {
    pub text: &'a str,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// The key is held and the press is an auto-repeat.
    pub repeat: bool,
}

impl KeyPress<'_> {
    /// The lowercased text the chords compare with. Backtab (Shift+Tab on some backends) is the Tab key with Shift.
    fn key_text(&self) -> String {
        if self.text == slint::SharedString::from(Key::Backtab).as_str() {
            return slint::SharedString::from(Key::Tab).to_string();
        }
        self.text.to_lowercase()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChordError {
    #[error("the key combination is empty")]
    Empty,
    #[error("`{text}` is not a key combination: {reason}")]
    Invalid { text: String, reason: String },
}

/// A parsed key combination.
#[derive(Clone, PartialEq, Eq)]
pub struct Chord {
    /// Canonical text (`Control+Shift+S`): modifiers in a fixed order, the key name as written (aliases resolved).
    text: String,
    keys: Keys,
    /// The lowercase key text Slint compares with the pressed text.
    key: String,
    control: bool,
    meta: bool,
    alt: Modifier,
    shift: Modifier,
}

impl fmt::Debug for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Chord({})", self.text)
    }
}

impl Chord {
    /// Reads `Modifier+...+Key`. Modifiers: `Control` (`Ctrl`), `Alt`, `Shift`, `Meta` (`Super`), `Shift?`, `Alt?`, any
    /// case. The key is a Slint key name (`F9`, `Home`, `Plus`, `LeftArrow`, `A`), a common alias (`Left`, `PgUp`, `Del`,
    /// `Esc`, `Minus`, any case) or a single character; `+` alone or at the end (`Control++`) is the plus key.
    pub fn parse(text: &str) -> Result<Self, ChordError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ChordError::Empty);
        }
        let invalid = |reason: &str| ChordError::Invalid {
            text: text.to_owned(),
            reason: reason.to_owned(),
        };
        let (rest, plus_key) = match text.strip_suffix("++") {
            Some(rest) => (rest, true),
            None if text == "+" => ("", true),
            None => (text, false),
        };
        let mut parts: Vec<&str> = if rest.is_empty() {
            Vec::new()
        } else {
            rest.split('+').map(str::trim).collect()
        };
        if plus_key {
            parts.push("Plus");
        }
        let Some((key_part, modifier_parts)) = parts.split_last() else {
            return Err(invalid("no key"));
        };
        let mut modifiers = Vec::with_capacity(modifier_parts.len());
        for part in modifier_parts {
            let canonical = match part.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => "Control",
                "alt" => "Alt",
                "alt?" => "Alt?",
                "shift" => "Shift",
                "shift?" => "Shift?",
                "meta" | "super" | "win" | "logo" => "Meta",
                "" => return Err(invalid("an empty part")),
                _ => return Err(invalid(&format!("`{part}` is not a modifier"))),
            };
            if modifiers.contains(&canonical) {
                return Err(invalid(&format!("{canonical} is given twice")));
            }
            modifiers.push(canonical);
        }
        let key_name = key_alias(key_part).unwrap_or(key_part);
        // Fixed order, so equal chords have equal text.
        modifiers.sort_by_key(|m| {
            ["Control", "Alt", "Alt?", "Shift", "Shift?", "Meta"]
                .iter()
                .position(|o| o == m)
        });
        let keys =
            Keys::from_parts(modifiers.iter().copied().chain([key_name])).map_err(|e| invalid(&e.to_string()))?;
        let mut chord = Self {
            text: modifiers
                .iter()
                .copied()
                .chain([key_name])
                .collect::<Vec<_>>()
                .join("+"),
            keys,
            key: String::new(),
            control: false,
            meta: false,
            alt: Modifier::Off,
            shift: Modifier::Off,
        };
        for part in chord.keys.to_parts() {
            match part {
                "Control" => chord.control = true,
                "Meta" => chord.meta = true,
                "Alt" => chord.alt = Modifier::On,
                "Alt?" => chord.alt = Modifier::Any,
                "Shift" => chord.shift = Modifier::On,
                "Shift?" => chord.shift = Modifier::Any,
                key => key.clone_into(&mut chord.key),
            }
        }
        if chord.key.is_empty() {
            return Err(invalid("no key"));
        }
        Ok(chord)
    }

    /// The canonical text stored in `config.toml`.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The parsed form for the menus.
    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    /// How the desktop writes it (`Ctrl+S`).
    pub fn label(&self) -> String {
        self.keys.to_string()
    }

    /// Whether `press` triggers this chord (Slint's rule: lowercased text, equal modifiers except the ignored ones).
    pub fn matches(&self, press: &KeyPress<'_>) -> bool {
        press.key_text() == self.key
            && press.control == self.control
            && press.meta == self.meta
            && self.alt.accepts(press.alt)
            && self.shift.accepts(press.shift)
    }

    /// Whether one key press could trigger both chords.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.key == other.key
            && self.control == other.control
            && self.meta == other.meta
            && self.alt.overlaps(other.alt)
            && self.shift.overlaps(other.shift)
    }

    /// The chord needs Control, Alt or Meta: it cannot be typed into a text field.
    pub fn has_command_modifier(&self) -> bool {
        self.control || self.meta || self.alt == Modifier::On
    }

    /// A key that types a character (a letter, a digit, a sign, Space) without Control, Alt or Meta: in the Browse view
    /// it belongs to type-ahead find.
    pub fn is_plain_printable(&self) -> bool {
        if self.has_command_modifier() {
            return false;
        }
        let mut chars = self.key.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_control() && !is_special_key(c))
    }

    /// The lowercase key text (a character, or the private code Slint gives a special key).
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn control(&self) -> bool {
        self.control
    }

    pub fn meta(&self) -> bool {
        self.meta
    }

    /// Alt is required (not only allowed).
    pub fn alt(&self) -> bool {
        self.alt == Modifier::On
    }

    /// Shift is required (not only allowed).
    pub fn shift(&self) -> bool {
        self.shift == Modifier::On
    }

    /// The chord a key press makes, with Slint's key names; `None` for a modifier alone or a key without a name.
    pub fn of_press(press: &KeyPress<'_>) -> Option<Self> {
        const MODIFIER_KEYS: [Key; 9] = [
            Key::Shift,
            Key::ShiftR,
            Key::Control,
            Key::ControlR,
            Key::Alt,
            Key::AltGr,
            Key::Meta,
            Key::MetaR,
            Key::CapsLock,
        ];
        let text = press.key_text();
        if text.is_empty()
            || MODIFIER_KEYS
                .iter()
                .any(|k| text == slint::SharedString::from(*k).as_str())
        {
            return None;
        }
        let mut prefix = String::new();
        for (on, name) in [
            (press.control, "Control+"),
            (press.alt, "Alt+"),
            (press.shift, "Shift+"),
            (press.meta, "Meta+"),
        ] {
            if on {
                prefix.push_str(name);
            }
        }
        let digit = text
            .chars()
            .next()
            .filter(char::is_ascii_digit)
            .map(|d| format!("Digit{d}"));
        let upper = text.to_uppercase();
        NAMED_KEYS
            .iter()
            .map(|k| (*k).to_owned())
            .chain(digit)
            .chain([upper, text.clone()])
            .filter_map(|name| Self::parse(&format!("{prefix}{name}")).ok())
            .find(|chord| chord.matches(press))
            .or_else(|| {
                // A layout-dependent key with Shift (Shift+Comma is "<" on some layouts): the key without the Shift prefix.
                let without_shift = prefix.replace("Shift+", "");
                NAMED_KEYS
                    .iter()
                    .filter_map(|name| Self::parse(&format!("{without_shift}{name}")).ok())
                    .find(|chord| chord.matches(press))
            })
    }
}

/// Slint's names of the keys that are not a plain character.
const NAMED_KEYS: [&str; 50] = [
    "LeftArrow",
    "RightArrow",
    "UpArrow",
    "DownArrow",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Insert",
    "Delete",
    "Backspace",
    "Return",
    "Tab",
    "Space",
    "Escape",
    "Plus",
    "HyphenMinus",
    "Comma",
    "Period",
    "Equals",
    "OpenBracket",
    "CloseBracket",
    "ScrollLock",
    "Pause",
    "SysReq",
    "Menu",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "F13",
    "F14",
    "F15",
    "F16",
    "F17",
    "F18",
    "F19",
    "F20",
    "F21",
    "F22",
    "F23",
    "F24",
];

/// Slint gives special keys (arrows, F-keys, Home, ...) characters of the Unicode private use area.
fn is_special_key(c: char) -> bool {
    matches!(u32::from(c), 0xE000..=0xF8FF)
}

/// Common spellings of Slint's key names (lowercase alias -> Slint name).
fn key_alias(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "left" | "leftarrow" | "arrowleft" => "LeftArrow",
        "right" | "rightarrow" | "arrowright" => "RightArrow",
        "up" | "uparrow" | "arrowup" => "UpArrow",
        "down" | "downarrow" | "arrowdown" => "DownArrow",
        "home" => "Home",
        "end" => "End",
        "pageup" | "pgup" | "prior" => "PageUp",
        "pagedown" | "pgdown" | "pgdn" | "next" => "PageDown",
        "insert" | "ins" => "Insert",
        "delete" | "del" => "Delete",
        "backspace" => "Backspace",
        "escape" | "esc" => "Escape",
        "return" | "enter" => "Return",
        "tab" => "Tab",
        "space" => "Space",
        "plus" => "Plus",
        "minus" | "hyphenminus" => "HyphenMinus",
        "comma" => "Comma",
        "period" | "dot" => "Period",
        "equals" | "equal" => "Equals",
        "scrolllock" | "scroll" => "ScrollLock",
        "pause" => "Pause",
        "sysreq" | "print" => "SysReq",
        _ => return function_key(name),
    })
}

/// `f1` ... `f24` in any case.
fn function_key(name: &str) -> Option<&'static str> {
    const NAMES: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "F16", "F17",
        "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    NAMES.into_iter().find(|n| n.eq_ignore_ascii_case(name))
}
