//! Icon library: typed access to every icon of the application.
//!
//! The icon set is data driven. `assets/icons/icons.toml` is the single source of truth; `build.rs` generates
//! the Rust enum [`Icon`] (this module) and the Slint enum `IconId` (see `ui/icons.slint`) from it, so both
//! sides always agree. All icons are Tabler SVGs (`assets/icons/tabler/`), except for the colored
//! rating icons (`assets/icons/custom/`).
//!
//! Rendering rules:
//! - Monochrome icons are tinted with a theme color by `AppIcon` (Slint `colorize`), so they follow any
//!   dark/light theme without extra assets.
//! - Colored icons (ratings) are never tinted, see [`Icon::is_monochrome`].
//! - Rust code that needs pixels outside of Slint (e.g. the drag ghost) uses [`raster::render_png`].

pub mod raster;

include!(concat!(env!("OUT_DIR"), "/icon_registry.rs"));

use std::fmt;

/// Static description of one icon (generated from the manifest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconSpec {
    /// Manifest id in snake_case, e.g. `rating_3`.
    pub id: &'static str,
    /// SVG path relative to `assets/`, e.g. `icons/tabler/outline/folder.svg`.
    pub source: &'static str,
    /// `false` for icons that keep their own colors (ratings).
    pub monochrome: bool,
    pub doc: &'static str,
    /// Additional names accepted by [`Icon::from_name`].
    pub aliases: &'static [&'static str],
    /// Embedded SVG source text.
    pub svg: &'static str,
}

impl Icon {
    /// Returns the static description of the icon.
    pub fn spec(self) -> &'static IconSpec {
        // Discriminants are assigned in declaration order, which matches `SPECS`.
        &SPECS[self as usize]
    }

    /// Manifest id, e.g. `"folder_open"`.
    pub fn id(self) -> &'static str {
        self.spec().id
    }

    /// Whether the icon is tinted with the theme color (false for rating icons).
    pub fn is_monochrome(self) -> bool {
        self.spec().monochrome
    }

    /// Embedded SVG source text.
    pub fn svg(self) -> &'static str {
        self.spec().svg
    }

    /// Rating badge icon for a rating value (0..=5, larger values are clamped).
    pub fn rating(rating: u8) -> Icon {
        match rating {
            0 => Icon::Rating0,
            1 => Icon::Rating1,
            2 => Icon::Rating2,
            3 => Icon::Rating3,
            4 => Icon::Rating4,
            _ => Icon::Rating5,
        }
    }
}

impl fmt::Display for Icon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Converts an optional icon to the Slint enum (`IconId.none` when absent).
pub fn to_slint(icon: Option<Icon>) -> crate::ui::IconId {
    icon.map_or(crate::ui::IconId::None, Icon::to_slint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn specs_follow_enum_order() {
        for (index, icon) in Icon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, index, "{icon} is out of order");
            assert_eq!(SPECS[index].id, icon.id());
        }
    }

    #[test]
    fn every_name_resolves_to_its_icon() {
        for icon in Icon::ALL {
            assert_eq!(Icon::from_name(icon.id()), Some(icon), "id of {icon}");
            for alias in icon.spec().aliases {
                assert_eq!(Icon::from_name(alias), Some(icon), "alias '{alias}' of {icon}");
            }
        }
        assert_eq!(Icon::from_name(" Folder-Open "), Some(Icon::FolderOpen));
        assert_eq!(Icon::from_name("does-not-exist"), None);
    }

    #[test]
    fn embedded_svgs_are_valid_vectors() {
        for icon in Icon::ALL {
            let svg = icon.svg();
            assert!(svg.contains("<svg"), "{icon} is not an SVG");
            assert!(svg.contains("viewBox"), "{icon} has no viewBox");
        }
    }

    #[test]
    fn only_ratings_are_colored() {
        let colored: HashSet<Icon> = Icon::ALL.iter().copied().filter(|i| !i.is_monochrome()).collect();
        let expected: HashSet<Icon> = [
            Icon::RatingClear,
            Icon::Rating0,
            Icon::Rating1,
            Icon::Rating2,
            Icon::Rating3,
            Icon::Rating4,
            Icon::Rating5,
        ]
        .into_iter()
        .collect();
        assert_eq!(colored, expected);
    }

    #[test]
    fn monochrome_tabler_icons_use_current_color() {
        for icon in Icon::ALL.iter().filter(|i| i.is_monochrome()) {
            assert!(icon.svg().contains("currentColor"), "{icon} must be tintable");
        }
    }

    #[test]
    fn rating_icons_are_clamped() {
        assert_eq!(Icon::rating(0), Icon::Rating0);
        assert_eq!(Icon::rating(5), Icon::Rating5);
        assert_eq!(Icon::rating(9), Icon::Rating5);
    }
}
