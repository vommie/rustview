//! Where an action of the keymap applies, and which areas can be active at the same time (only those can conflict).

/// The areas, in the order of the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Area {
    /// The main window in every view (Browse, tab view, tab grid).
    General,
    /// The Browse view (its Rename also works in the tab grid).
    Browse,
    /// A Multiview shown in a tab; acts on the selected medium.
    Tab,
    /// The Multiview fullscreen window; acts on the selected medium.
    Fullscreen,
    /// The selected video in a tab or the fullscreen (mpv takes no keys; `RustView` drives the player).
    Player,
}

impl Area {
    pub const ALL: [Self; 5] = [Self::General, Self::Browse, Self::Tab, Self::Fullscreen, Self::Player];

    /// The prefix of its actions' ids in `settings.json`.
    pub const fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Browse => "browse",
            Self::Tab => "tab",
            Self::Fullscreen => "fullscreen",
            Self::Player => "player",
        }
    }

    /// The heading of its section on the Settings page.
    pub const fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Browse => "Browse",
            Self::Tab => "Multiview in a Tab",
            Self::Fullscreen => "Multiview in the Fullscreen",
            Self::Player => "Video Player",
        }
    }

    /// A line under the heading.
    pub const fn description(self) -> &'static str {
        match self {
            Self::General => "In the main window, whatever it shows.",
            Self::Browse => "In the view with the thumbnails or the table.",
            Self::Tab => "In a tab; the keys act on the selected medium.",
            Self::Fullscreen => "In the fullscreen; the keys act on the selected medium.",
            Self::Player => "On the selected video in a tab or the fullscreen.",
        }
    }

    /// Whether a key press can reach actions of both areas, so they must not share a key.
    pub const fn overlaps(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::General, Self::General | Self::Browse | Self::Tab | Self::Player)
                | (Self::Browse, Self::General | Self::Browse)
                | (Self::Tab, Self::General | Self::Tab | Self::Player)
                | (Self::Fullscreen, Self::Fullscreen | Self::Player)
                | (
                    Self::Player,
                    Self::General | Self::Tab | Self::Fullscreen | Self::Player
                )
        )
    }
}
