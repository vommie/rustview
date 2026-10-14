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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(host: HostMode, origin: OriginKind, tile_count: usize, kind: TileKind) -> ActionContext {
        ActionContext {
            host,
            origin,
            tile_count,
            kind,
            muted: (kind == TileKind::Video).then_some(false),
            sticky: false,
            single_tab: false,
        }
    }

    fn ids(ctx: &ActionContext) -> Vec<ActionId> {
        actions_for(ctx).iter().map(|a| a.id).collect()
    }

    #[test]
    fn a_followed_video_gets_the_full_bar_in_a_fixed_order() {
        let c = ctx(HostMode::Fullscreen, OriginKind::Browse, 2, TileKind::Video);
        assert_eq!(
            ids(&c),
            vec![
                ActionId::Mute,
                ActionId::Sticky,
                ActionId::OpenAsTab,
                ActionId::Remove
            ]
        );
    }

    #[test]
    fn an_image_has_no_sound_button() {
        let c = ctx(HostMode::Fullscreen, OriginKind::Browse, 3, TileKind::Image);
        assert_eq!(
            ids(&c),
            vec![ActionId::Sticky, ActionId::OpenAsTab, ActionId::Remove]
        );
    }

    #[test]
    fn remove_becomes_close_for_the_last_medium() {
        let c = ctx(HostMode::Fullscreen, OriginKind::Browse, 1, TileKind::Image);
        assert_eq!(
            ids(&c),
            vec![ActionId::Sticky, ActionId::OpenAsTab, ActionId::Close]
        );
        let close = actions_for(&c).pop().expect("a close button");
        assert_eq!((close.icon, close.tooltip), (Icon::Close, "Close fullscreen"));
    }

    #[test]
    fn a_tab_in_the_fullscreen_can_shrink_and_open_media_as_tabs() {
        let video = ctx(HostMode::Fullscreen, OriginKind::Tab, 3, TileKind::Video);
        assert_eq!(
            ids(&video),
            vec![ActionId::Mute, ActionId::OpenAsTab, ActionId::Remove]
        );
        let image = ctx(HostMode::Fullscreen, OriginKind::Tab, 2, TileKind::Image);
        assert_eq!(ids(&image), vec![ActionId::OpenAsTab, ActionId::Remove]);
        let remove = actions_for(&image).pop().expect("a remove button");
        assert_eq!(remove.tooltip, "Remove from this tab");
        let last = ctx(HostMode::Fullscreen, OriginKind::Tab, 1, TileKind::Image);
        assert_eq!(ids(&last), vec![ActionId::OpenAsTab, ActionId::Close]);
        let close = actions_for(&last).pop().expect("a close button");
        assert_eq!((close.icon, close.tooltip), (Icon::Close, "Close fullscreen"));
    }

    #[test]
    fn the_fullscreen_of_a_normal_tab_only_offers_sound_and_closing() {
        let mut video = ctx(HostMode::Fullscreen, OriginKind::Tab, 1, TileKind::Video);
        video.single_tab = true;
        assert_eq!(ids(&video), vec![ActionId::Mute, ActionId::Close]);
        let mut image = ctx(HostMode::Fullscreen, OriginKind::Tab, 1, TileKind::Image);
        image.single_tab = true;
        assert_eq!(ids(&image), vec![ActionId::Close]);
    }

    #[test]
    fn every_medium_of_a_tab_view_has_a_bar() {
        let video = ctx(HostMode::TabView, OriginKind::Tab, 2, TileKind::Video);
        assert_eq!(
            ids(&video),
            vec![ActionId::Mute, ActionId::OpenAsTab, ActionId::Remove]
        );
        let image = ctx(HostMode::TabView, OriginKind::Tab, 4, TileKind::Image);
        assert_eq!(ids(&image), vec![ActionId::OpenAsTab, ActionId::Remove]);
    }

    #[test]
    fn the_last_medium_of_a_tab_view_closes_the_tab() {
        let last = ctx(HostMode::TabView, OriginKind::Tab, 1, TileKind::Image);
        assert_eq!(ids(&last), vec![ActionId::OpenAsTab, ActionId::Close]);
        let close = actions_for(&last).pop().expect("a close button");
        assert_eq!((close.icon, close.tooltip), (Icon::Close, "Close tab"));
    }

    #[test]
    fn only_a_followed_multiview_offers_sticky() {
        for host in [HostMode::Fullscreen, HostMode::TabView] {
            for count in 1..=4 {
                for kind in [TileKind::Image, TileKind::Video] {
                    let has_sticky = ids(&ctx(host, OriginKind::Tab, count, kind))
                        .contains(&ActionId::Sticky);
                    assert!(!has_sticky, "{host:?} with {count} tiles");
                }
            }
        }
        let browse = ctx(HostMode::Fullscreen, OriginKind::Browse, 2, TileKind::Image);
        assert!(ids(&browse).contains(&ActionId::Sticky));
    }

    #[test]
    fn the_grid_and_a_hidden_multiview_offer_nothing() {
        let grid = ctx(HostMode::TabGrid, OriginKind::Tab, 2, TileKind::Video);
        assert!(ids(&grid).is_empty());
        let hidden = ctx(HostMode::Hidden, OriginKind::Browse, 2, TileKind::Video);
        assert!(ids(&hidden).is_empty());
    }

    #[test]
    fn toggle_buttons_show_their_state() {
        let mut c = ctx(HostMode::Fullscreen, OriginKind::Browse, 2, TileKind::Video);
        let off = actions_for(&c);
        assert_eq!(
            (off[0].icon, off[0].tooltip),
            (Icon::VolumeOn, "Mute"),
            "audible video offers to mute"
        );
        assert_eq!((off[1].icon, off[1].active), (Icon::MultiviewSticky, Some(false)));
        c.muted = Some(true);
        c.sticky = true;
        let on = actions_for(&c);
        assert_eq!((on[0].icon, on[0].tooltip), (Icon::VolumeOff, "Unmute"));
        assert_eq!((on[1].icon, on[1].active), (Icon::MultiviewStickyOn, Some(true)));
    }

    #[test]
    fn a_video_without_sound_control_gets_no_mute_button() {
        let mut c = ctx(HostMode::Fullscreen, OriginKind::Browse, 2, TileKind::Video);
        c.muted = None;
        assert!(!ids(&c).contains(&ActionId::Mute));
    }

    #[test]
    fn codes_round_trip_and_unknown_codes_are_rejected() {
        for id in [
            ActionId::Mute,
            ActionId::Sticky,
            ActionId::OpenAsTab,
            ActionId::Remove,
            ActionId::Close,
        ] {
            assert_eq!(ActionId::from_code(id.code()), Some(id));
        }
        assert_eq!(ActionId::from_code(-1), None);
        assert_eq!(ActionId::from_code(99), None);
    }
}
