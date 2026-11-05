//! The popups a collapsed control bar opens (pure): the vertical volume bar of the speaker, the "..." menu with its submenu,
//! and the bubble of the A-B button. The save bubble and the load menu of the A-B group are `ab_state::AbPopup`; at most one
//! popup of a bar is open (opening one closes the other), the A-B bubble is a toggle of its own that stays open.
//!
//! Whether the A-B bubble is open is remembered for the next videos and the next start (`view.video_bar_ab_bubble_open`):
//! the app installs the stored value and a writer at startup (`install`).

use super::bar_menu::MenuPath;
use super::collapse::BarFit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};

/// A popup of the bar besides the A-B save bubble and load menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarPopup {
    /// The vertical volume bar above the speaker (or above "..." once the sound is in the menu).
    Volume,
    /// The "..." menu, with its open submenus.
    More { path: MenuPath },
    /// The menu of the filter button, with its open submenu.
    Filters { path: MenuPath },
}

impl BarPopup {
    /// Whether the popup still has its opener in a bar of `fit`: the volume bar needs a bar without slider, the "..." menu
    /// a bar with "...", the filter menu one with the filter button.
    pub fn fits(self, fit: BarFit) -> bool {
        match self {
            Self::Volume => !fit.has_slider(),
            Self::More { .. } => fit.has_more(),
            Self::Filters { .. } => fit.filter_in_bar(),
        }
    }

    /// The open submenus of a menu (`None`: the volume popup).
    pub fn path(self) -> Option<MenuPath> {
        match self {
            Self::More { path } | Self::Filters { path } => Some(path),
            Self::Volume => None,
        }
    }
}

static AB_BUBBLE_OPEN: AtomicBool = AtomicBool::new(rustview_core::settings::schema::DEFAULT_VIDEO_BAR_AB_BUBBLE_OPEN);

type Writer = Box<dyn Fn(bool) + Send + Sync>;

static WRITER: LazyLock<RwLock<Option<Writer>>> = LazyLock::new(|| RwLock::new(None));

/// The stored state of the A-B bubble and how a change is stored (once, at startup).
pub fn install(open: bool, writer: impl Fn(bool) + Send + Sync + 'static) {
    AB_BUBBLE_OPEN.store(open, Ordering::Relaxed);
    if let Ok(mut slot) = WRITER.write() {
        *slot = Some(Box::new(writer));
    }
}

/// Whether a bar that collapses its A-B group into a button shows the bubble (the user's last choice).
pub fn ab_bubble_open() -> bool {
    AB_BUBBLE_OPEN.load(Ordering::Relaxed)
}

/// The user opened or closed the A-B bubble: every bar follows, and the choice is stored.
pub fn set_ab_bubble_open(open: bool) {
    if AB_BUBBLE_OPEN.swap(open, Ordering::Relaxed) == open {
        return;
    }
    tracing::info!(open, "[Multiview: VideoBar] A-B bubble toggled");
    if let Ok(slot) = WRITER.read()
        && let Some(writer) = slot.as_ref()
    {
        writer(open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_popup_closes_when_its_opener_leaves_the_bar() {
        assert!(!BarPopup::Volume.fits(BarFit::Full));
        assert!(BarPopup::Volume.fits(BarFit::AudioButton));
        assert!(BarPopup::Volume.fits(BarFit::NoTime));
        let menu = BarPopup::More {
            path: MenuPath::default(),
        };
        assert!(!menu.fits(BarFit::AbButton));
        assert!(menu.fits(BarFit::FilterMore));
        let filters = BarPopup::Filters {
            path: MenuPath::default(),
        };
        assert!(filters.fits(BarFit::AbButton));
        assert!(!filters.fits(BarFit::FilterMore));
    }
}
