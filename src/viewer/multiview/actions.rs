//! The action buttons of the hover bar (pure).
//!
//! Rust decides *which* buttons a hovered medium gets and in which order; the Slint action bar only draws them. A new
//! action is one provider function appended to `PROVIDERS`. Providers look at where the Multiview is shown and what it
//! was opened from: a Multiview that follows the Browse selection offers sticky/remove/open-as-tab; a Multiview that
//! belongs to a tab (shown in the tab or in the fullscreen) offers remove/open-as-tab too, but no sticky (a tab has no
//! Browse selection to stay independent of). On the last medium remove turns into a close button that fits the host.

use super::model::TileKind;
use super::route::{HostMode, OriginKind};
use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    Mute,
    Sticky,
    OpenAsTab,
    Remove,
    Close,
}

impl ActionId {
    /// Stable number that crosses the Slint boundary.
    pub const fn code(self) -> i32 {
        match self {
            Self::Mute => 0,
            Self::Sticky => 1,
            Self::OpenAsTab => 2,
            Self::Remove => 3,
            Self::Close => 4,
        }
    }

    pub fn from_code(code: i32) -> Option<Self> {
        [
            Self::Mute,
            Self::Sticky,
            Self::OpenAsTab,
            Self::Remove,
            Self::Close,
        ]
        .into_iter()
        .find(|id| id.code() == code)
    }
}

/// Everything a provider may look at; all of it is in-memory state of the hovered tile and its Multiview.
#[derive(Debug, Clone, Copy)]
pub struct ActionContext {
    pub host: HostMode,
    pub origin: OriginKind,
    pub tile_count: usize,
    pub kind: TileKind,
    /// `Some(muted)` while the tile has sound control (a playing video), else `None`.
    pub muted: Option<bool>,
    pub sticky: bool,
    /// The Multiview is only the fullscreen view of a normal image or video tab: that tab already exists, so there is
    /// nothing to open as a tab.
    pub single_tab: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSpec {
    pub id: ActionId,
    pub icon: Icon,
    pub tooltip: &'static str,
    /// `Some(on)` for buttons that show a toggle state.
    pub active: Option<bool>,
}

type Provider = fn(&ActionContext) -> Option<ActionSpec>;

/// Order of the buttons in the bar, left to right.
const PROVIDERS: &[Provider] = &[mute, sticky, open_as_tab, remove, close];

/// The bar exists in the fullscreen window and in the viewer tab; the tab-grid miniature is a picture.
fn interactive(ctx: &ActionContext) -> bool {
    matches!(ctx.host, HostMode::Fullscreen | HostMode::TabView)
}

/// Only a Multiview that follows the Browse selection can keep media independent of that selection.
fn is_workspace(ctx: &ActionContext) -> bool {
    ctx.host == HostMode::Fullscreen && ctx.origin == OriginKind::Browse
}

fn mute(ctx: &ActionContext) -> Option<ActionSpec> {
    let muted = ctx.muted.filter(|_| interactive(ctx))?;
    Some(ActionSpec {
        id: ActionId::Mute,
        icon: if muted {
            Icon::VolumeOff
        } else {
            Icon::VolumeOn
        },
        tooltip: if muted { "Unmute" } else { "Mute" },
        active: None,
    })
}

fn sticky(ctx: &ActionContext) -> Option<ActionSpec> {
    is_workspace(ctx).then_some(ActionSpec {
        id: ActionId::Sticky,
        icon: if ctx.sticky {
            Icon::MultiviewStickyOn
        } else {
            Icon::MultiviewSticky
        },
        tooltip: if ctx.sticky {
            "Sticky: stays when the selection changes (click to release)"
        } else {
            "Make sticky: keep this medium when the selection changes"
        },
        active: Some(ctx.sticky),
    })
}

fn open_as_tab(ctx: &ActionContext) -> Option<ActionSpec> {
    (interactive(ctx) && !ctx.single_tab).then_some(ActionSpec {
        id: ActionId::OpenAsTab,
        icon: Icon::OpenInBackgroundTab,
        tooltip: "Open in a background tab",
        active: None,
    })
}

fn remove(ctx: &ActionContext) -> Option<ActionSpec> {
    (interactive(ctx) && ctx.tile_count > 1).then_some(ActionSpec {
        id: ActionId::Remove,
        icon: Icon::MultiviewRemove,
        tooltip: if ctx.origin == OriginKind::Browse {
            "Remove from this view and deselect"
        } else {
            "Remove from this tab"
        },
        active: None,
    })
}

/// The last remaining medium cannot be removed (the view is never empty), so the button closes what shows it instead:
/// the fullscreen, or the tab.
fn close(ctx: &ActionContext) -> Option<ActionSpec> {
    (interactive(ctx) && ctx.tile_count <= 1).then_some(ActionSpec {
        id: ActionId::Close,
        icon: Icon::Close,
        tooltip: if ctx.host == HostMode::Fullscreen {
            "Close fullscreen"
        } else {
            "Close tab"
        },
        active: None,
    })
}

pub fn actions_for(ctx: &ActionContext) -> Vec<ActionSpec> {
    PROVIDERS.iter().filter_map(|provider| provider(ctx)).collect()
}
