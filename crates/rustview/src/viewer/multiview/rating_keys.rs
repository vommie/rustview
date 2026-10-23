//! Which key press is a rating (pure).
//!
//! The hotkeys are Ctrl+0 (no rating) to Ctrl+5 on the main row and on the numpad, because every plain printable key belongs
//! to type-ahead find in the Browse view. Alt, Meta and Shift variants are other shortcuts and never rate. A viewer tab may
//! also take the plain digits (they were its rating keys before the hotkeys existed; nothing else uses them there).

/// The rating a key press stands for, `None` when it is no rating key. `text` is what the key produced (Slint's `event.text`).
pub fn rating_of_key(text: &str, ctrl: bool, alt: bool, meta: bool, plain_digits_ok: bool) -> Option<u8> {
    if alt || meta || (!ctrl && !plain_digits_ok) {
        return None;
    }
    let mut chars = text.chars();
    let (Some(first), None) = (chars.next(), chars.next()) else {
        return None;
    };
    match first {
        '0'..='5' => u8::try_from(first as u32 - '0' as u32).ok(),
        _ => None,
    }
}

/// The key names mpv gets bindings for, with the rating each stands for: main row and numpad.
pub fn mpv_key_names() -> Vec<(String, u8)> {
    (0..=5u8)
        .flat_map(|n| [(format!("ctrl+{n}"), n), (format!("ctrl+KP{n}"), n)])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_digits_zero_to_five_rate() {
        for (n, text) in ["0", "1", "2", "3", "4", "5"].into_iter().enumerate() {
            assert_eq!(
                rating_of_key(text, true, false, false, false),
                Some(u8::try_from(n).unwrap())
            );
        }
    }

    #[test]
    fn other_digits_and_texts_do_not_rate() {
        for text in ["6", "7", "8", "9", "a", "", "10", "-", "\u{1b}", "\u{0}"] {
            assert_eq!(rating_of_key(text, true, false, false, true), None, "{text:?}");
        }
    }

    #[test]
    fn plain_digits_only_where_allowed() {
        assert_eq!(rating_of_key("3", false, false, false, false), None);
        assert_eq!(rating_of_key("3", false, false, false, true), Some(3));
    }

    #[test]
    fn alt_and_meta_variants_are_other_shortcuts() {
        assert_eq!(rating_of_key("3", true, true, false, true), None);
        assert_eq!(rating_of_key("3", true, false, true, true), None);
        assert_eq!(rating_of_key("3", false, true, false, true), None);
    }

    #[test]
    fn mpv_gets_twelve_bindings() {
        let names = mpv_key_names();
        assert_eq!(names.len(), 12);
        assert!(names.contains(&("ctrl+0".to_string(), 0)));
        assert!(names.contains(&("ctrl+KP5".to_string(), 5)));
    }
}
