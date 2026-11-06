//! Dark or light (questionnaire 2026-11-06-rustview-light-theme, shared with `RustCut`): Settings > Appearance > Theme
//! follows the system (KDE's color scheme through the desktop portal), or is always dark or always light. Without an
//! answer from the system the app stays dark.

/// `appearance.theme` in `settings.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub const NAMES: [&'static str; 3] = ["system", "dark", "light"];

    /// The stored name; an unknown one means "follow the system".
    pub fn from_name(name: &str) -> Self {
        match name {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// The position in the Settings window's choice (Follow system, Dark, Light).
    pub fn index(self) -> usize {
        match self {
            Self::System => 0,
            Self::Dark => 1,
            Self::Light => 2,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Dark,
            2 => Self::Light,
            _ => Self::System,
        }
    }
}

/// What the desktop prefers (the portal's `org.freedesktop.appearance` `color-scheme`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemScheme {
    /// No answer yet, no portal, or "no preference".
    #[default]
    Unknown,
    Dark,
    Light,
}

/// Whether the windows are dark.
pub fn resolve_dark(mode: ThemeMode, system: SystemScheme) -> bool {
    match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => system != SystemScheme::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_mode_ignores_the_system() {
        for system in [SystemScheme::Unknown, SystemScheme::Dark, SystemScheme::Light] {
            assert!(resolve_dark(ThemeMode::Dark, system));
            assert!(!resolve_dark(ThemeMode::Light, system));
        }
    }

    #[test]
    fn following_the_system_stays_dark_without_an_answer() {
        assert!(resolve_dark(ThemeMode::System, SystemScheme::Unknown));
        assert!(resolve_dark(ThemeMode::System, SystemScheme::Dark));
        assert!(!resolve_dark(ThemeMode::System, SystemScheme::Light));
    }

    #[test]
    fn names_and_indexes_round_trip() {
        for mode in [ThemeMode::System, ThemeMode::Dark, ThemeMode::Light] {
            assert_eq!(ThemeMode::from_name(mode.name()), mode);
            assert_eq!(ThemeMode::from_index(mode.index()), mode);
        }
        assert_eq!(ThemeMode::from_name("purple"), ThemeMode::System);
        assert_eq!(ThemeMode::from_index(9), ThemeMode::System);
    }
}
