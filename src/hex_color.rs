//! `#rrggbb` color strings: the form colors are stored in settings and typed into the color picker.

/// Parses `rgb` or `rrggbb` (the leading `#` is optional, case does not matter). `None` for anything else.
pub fn parse_rgb(text: &str) -> Option<[u8; 3]> {
    let digits = digits(text).filter(|d| matches!(d.len(), 3 | 6))?;
    let [r, g, b, _] = parse_digits(digits)?;
    Some([r, g, b])
}

/// Parses `rgb`, `rgba`, `rrggbb` or `rrggbbaa` (the leading `#` is optional, case does not matter). A missing alpha is
/// opaque.
pub fn parse_rgba(text: &str) -> Option<[u8; 4]> {
    parse_digits(digits(text)?)
}

fn parse_digits(digits: &[u8]) -> Option<[u8; 4]> {
    let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let pair = |hi: u8, lo: u8| Some(nibble(hi)? * 16 + nibble(lo)?);
    match *digits {
        [r, g, b] => Some([pair(r, r)?, pair(g, g)?, pair(b, b)?, 255]),
        [r, g, b, a] => Some([pair(r, r)?, pair(g, g)?, pair(b, b)?, pair(a, a)?]),
        [r1, r0, g1, g0, b1, b0] => Some([pair(r1, r0)?, pair(g1, g0)?, pair(b1, b0)?, 255]),
        [r1, r0, g1, g0, b1, b0, a1, a0] => {
            Some([pair(r1, r0)?, pair(g1, g0)?, pair(b1, b0)?, pair(a1, a0)?])
        }
        _ => None,
    }
}

/// The hex digits of `text` without `#` (only if all of them are hex digits).
fn digits(text: &str) -> Option<&[u8]> {
    let bytes = text.trim().trim_start_matches('#').as_bytes();
    bytes.iter().all(u8::is_ascii_hexdigit).then_some(bytes)
}

pub fn format_rgb([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn format_rgba([r, g, b, a]: [u8; 4]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_in_every_accepted_form() {
        assert_eq!(parse_rgb("#11111b"), Some([0x11, 0x11, 0x1b]));
        assert_eq!(parse_rgb(" ABC "), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(parse_rgba("#11223344"), Some([0x11, 0x22, 0x33, 0x44]));
        assert_eq!(parse_rgba("f80"), Some([0xff, 0x88, 0x00, 255]));
        assert_eq!(parse_rgba("#f808"), Some([0xff, 0x88, 0x00, 0x88]));
    }

    #[test]
    fn invalid_colors_are_refused() {
        for text in ["", "#", "#12", "#12345", "#gggggg", "#11111b00", "red", "#11 11 1b"] {
            assert_eq!(parse_rgb(text), None, "{text:?}");
        }
    }

    #[test]
    fn formatting_round_trips() {
        assert_eq!(format_rgb([1, 2, 255]), "#0102ff");
        assert_eq!(format_rgba([1, 2, 255, 0]), "#0102ff00");
        assert_eq!(parse_rgb(&format_rgb([9, 99, 199])), Some([9, 99, 199]));
    }
}
