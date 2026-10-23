//! Optional per-badge color overrides.
//!
//! Badges normally take their colors from a semantic `ThumbnailBadgeTone` (resolved against the
//! theme in Slint). `BadgeColor` is the escape hatch for a badge that must deviate from the theme,
//! e.g. a user-defined tag color given as hex string.

use slint::Color;

/// Dark text color used on light badge backgrounds (matches `Theme.badge-text-on-light`).
const DARK_TEXT: (u8, u8, u8) = (0x11, 0x11, 0x1b);
const LIGHT_TEXT: (u8, u8, u8) = (0xff, 0xff, 0xff);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BadgeColorError {
    #[error("badge color '{0}' is not a valid hex color (expected #rgb, #rgba, #rrggbb or #rrggbbaa)")]
    InvalidHex(String),
}

/// An explicit RGBA color for a single badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadgeColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl BadgeColor {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parses `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` (the leading `#` is optional).
    pub fn from_hex(input: &str) -> Result<Self, BadgeColorError> {
        let digits = input.trim().trim_start_matches('#');
        let nibbles: Option<Vec<u8>> = digits
            .chars()
            .map(|c| {
                c.to_digit(16)
                    .map(rustview_core::util::cast::SaturatingAs::saturating_as::<u8>)
            })
            .collect();

        match nibbles.as_deref() {
            Some(&[r, g, b]) => Ok(Self::rgba(r * 17, g * 17, b * 17, 255)),
            Some(&[r, g, b, a]) => Ok(Self::rgba(r * 17, g * 17, b * 17, a * 17)),
            Some(&[r1, r2, g1, g2, b1, b2]) => Ok(Self::rgba(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2, 255)),
            Some(&[r1, r2, g1, g2, b1, b2, a1, a2]) => {
                Ok(Self::rgba(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2, a1 * 16 + a2))
            }
            _ => Err(BadgeColorError::InvalidHex(input.to_string())),
        }
    }

    pub fn to_slint(self) -> Color {
        Color::from_argb_u8(self.a, self.r, self.g, self.b)
    }

    /// Picks the text color (dark or light) with the higher WCAG contrast against this color.
    pub fn contrast_text(self) -> Color {
        let background = relative_luminance(self.r, self.g, self.b);
        let contrast_with = |(r, g, b): (u8, u8, u8)| {
            let other = relative_luminance(r, g, b);
            let (hi, lo) = if background > other {
                (background, other)
            } else {
                (other, background)
            };
            (hi + 0.05) / (lo + 0.05)
        };
        let (r, g, b) = if contrast_with(DARK_TEXT) >= contrast_with(LIGHT_TEXT) {
            DARK_TEXT
        } else {
            LIGHT_TEXT
        };
        Color::from_rgb_u8(r, g, b)
    }
}

/// WCAG relative luminance of an sRGB color.
fn relative_luminance(r: u8, g: u8, b: u8) -> f32 {
    let linear = |c: u8| {
        let v = f32::from(c) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_supported_hex_forms() {
        assert_eq!(BadgeColor::from_hex("#f00"), Ok(BadgeColor::rgba(255, 0, 0, 255)));
        assert_eq!(BadgeColor::from_hex("#f008"), Ok(BadgeColor::rgba(255, 0, 0, 136)));
        assert_eq!(
            BadgeColor::from_hex("#89dceb"),
            Ok(BadgeColor::rgba(0x89, 0xdc, 0xeb, 255))
        );
        assert_eq!(
            BadgeColor::from_hex("89dcebcc"),
            Ok(BadgeColor::rgba(0x89, 0xdc, 0xeb, 0xcc))
        );
        assert_eq!(
            BadgeColor::from_hex("  #FFFFFF "),
            Ok(BadgeColor::rgba(255, 255, 255, 255))
        );
    }

    #[test]
    fn rejects_invalid_input_without_panicking() {
        for bad in ["", "#", "#12", "#12345", "#gggggg", "#12345678901", "röt", "#ff 00 00"] {
            assert!(BadgeColor::from_hex(bad).is_err(), "'{bad}' must be rejected");
        }
    }

    #[test]
    fn contrast_text_is_dark_on_light_and_light_on_dark() {
        assert_eq!(
            BadgeColor::rgba(255, 255, 255, 255).contrast_text(),
            Color::from_rgb_u8(0x11, 0x11, 0x1b)
        );
        assert_eq!(
            BadgeColor::rgba(0, 0, 0, 255).contrast_text(),
            Color::from_rgb_u8(255, 255, 255)
        );
    }
}
