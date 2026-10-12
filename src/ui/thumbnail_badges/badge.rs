//! The thumbnail badge specification and its builder API.
//!
//! A `ThumbnailBadge` only describes *what* is shown and *where it wants to be* (orientation,
//! optional slot, optional color). Sizing, ordering and placement are done by the engine, and the
//! shared look is defined once in `ui/thumbnail_badge.slint`. That guarantees all badges share the
//! same base design no matter who creates them.

use super::color::BadgeColor;
use crate::icons::Icon;
use crate::ui::{ThumbnailBadgeOrientation, ThumbnailBadgeTone};

/// What a badge displays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgeContent {
    /// Compact rounded text label (e.g. "VIDEO", "04:30").
    Text(String),
    /// Circular label with a short glyph (e.g. a rating number).
    Round(String),
    /// Bare icon from the icon library.
    Icon(Icon),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailBadge {
    /// Stable identifier, used for logging and diagnostics.
    pub id: &'static str,
    pub content: BadgeContent,
    pub orientation: ThumbnailBadgeOrientation,
    /// Explicit position inside the badge's area (0 = first). `None` keeps the automatic order.
    pub slot: Option<usize>,
    pub tone: ThumbnailBadgeTone,
    /// Optional explicit color overriding the tone.
    pub color: Option<BadgeColor>,
}

impl ThumbnailBadge {
    fn new(id: &'static str, content: BadgeContent) -> Self {
        Self {
            id,
            content,
            orientation: ThumbnailBadgeOrientation::BottomLeft,
            slot: None,
            tone: ThumbnailBadgeTone::Standard,
            color: None,
        }
    }

    /// Compact text badge.
    pub fn text(id: &'static str, text: impl Into<String>) -> Self {
        Self::new(id, BadgeContent::Text(text.into()))
    }

    /// Circular badge with a short glyph.
    pub fn round(id: &'static str, glyph: impl Into<String>) -> Self {
        Self::new(id, BadgeContent::Round(glyph.into()))
    }

    /// Icon badge. Colored icons (tag) keep their colors, monochrome icons are tinted by the badge color.
    pub fn icon(id: &'static str, icon: Icon) -> Self {
        Self::new(id, BadgeContent::Icon(icon))
    }

    /// Anchors the badge to a thumbnail corner. Left corners are the primary (left-aligned) area,
    /// right corners the secondary (right-aligned) area.
    pub fn at(mut self, orientation: ThumbnailBadgeOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Requests an explicit position inside the badge's area (0 = first badge of the area).
    pub fn slot(mut self, slot: usize) -> Self {
        self.slot = Some(slot);
        self
    }

    /// Selects a semantic theme color.
    pub fn tone(mut self, tone: ThumbnailBadgeTone) -> Self {
        self.tone = tone;
        self
    }

    /// Overrides the theme color with an explicit color.
    pub fn color(mut self, color: BadgeColor) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the theme color with a hex string. An invalid string is logged and ignored, so a
    /// bad color can never prevent the badge itself from being shown.
    pub fn color_hex(mut self, hex: &str) -> Self {
        match BadgeColor::from_hex(hex) {
            Ok(color) => self.color = Some(color),
            Err(e) => tracing::warn!(badge = self.id, error = %e, "[ThumbnailBadges] Ignoring invalid badge color"),
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_applies_all_options() {
        let badge = ThumbnailBadge::text("demo", "X")
            .at(ThumbnailBadgeOrientation::TopRight)
            .slot(2)
            .tone(ThumbnailBadgeTone::Danger)
            .color_hex("#ff0000");
        assert_eq!(badge.orientation, ThumbnailBadgeOrientation::TopRight);
        assert_eq!(badge.slot, Some(2));
        assert_eq!(badge.tone, ThumbnailBadgeTone::Danger);
        assert_eq!(badge.color, Some(BadgeColor::rgba(255, 0, 0, 255)));
    }

    #[test]
    fn invalid_hex_keeps_badge_without_color() {
        let badge = ThumbnailBadge::icon("demo", Icon::Tag).color_hex("not-a-color");
        assert_eq!(badge.color, None);
    }
}
