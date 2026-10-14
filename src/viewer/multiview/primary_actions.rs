//! The buttons of the primary action bar (pure).
//!
//! The primary bar sits in the top-left corner of the whole Multiview and holds the buttons that concern the view, not one
//! medium (the media action bar, `media_actions.rs`, does that). Like there, Rust decides *which* buttons exist, their order,
//! their state and their tooltip (it explains the state, and why a button is disabled); the Slint bar only draws them. A new
//! button is one provider appended to `PROVIDERS`.

use super::route::{HostMode, OriginKind};
use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimaryActionId {
    SendToTab,
    Close,
    MuteAll,
    PlayPauseAll,
    ResetLayout,
    HideIdleCursor,
    Pin,
}

impl PrimaryActionId {
    const ALL: [Self; 7] = [
        Self::SendToTab,
        Self::Close,
        Self::MuteAll,
        Self::PlayPauseAll,
        Self::ResetLayout,
        Self::HideIdleCursor,
        Self::Pin,
    ];

    /// Stable number that crosses the Slint boundary.
    pub const fn code(self) -> i32 {
        match self {
            Self::SendToTab => 0,
            Self::Close => 1,
            Self::MuteAll => 2,
            Self::PlayPauseAll => 3,
            Self::ResetLayout => 4,
            Self::HideIdleCursor => 5,
            Self::Pin => 6,
        }
    }

    pub fn from_code(code: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.code() == code)
    }
}

/// Everything a provider may look at; all of it is in-memory state of the Multiview.
#[derive(Debug, Clone, Copy)]
pub struct PrimaryBarContext {
    pub host: HostMode,
    pub origin: OriginKind,
    pub tile_count: usize,
    /// Tiles that have a running player.
    pub video_count: usize,
    /// At least one video is audible.
    pub any_audible: bool,
    /// At least one video is playing (not paused by the user).
    pub any_playing: bool,
    /// The user moved a resizer, so the arrangement is no longer the automatic one.
    pub layout_custom: bool,
    /// The fullscreen cannot be closed.
    pub pinned: bool,
    pub hide_idle_cursor: bool,
    pub idle_cursor_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryActionSpec {
    pub id: PrimaryActionId,
    pub icon: Icon,
    pub tooltip: String,
    pub enabled: bool,
    /// `Some(on)` for buttons that show a toggle state.
    pub active: Option<bool>,
}

type Provider = fn(&PrimaryBarContext) -> Option<PrimaryActionSpec>;

/// Order of the buttons in the bar, left to right.
const PROVIDERS: &[Provider] = &[
    send_to_tab,
    close,
    mute_all,
    play_pause_all,
    reset_layout,
    hide_idle_cursor,
    pin,
];

/// The bar exists in the fullscreen window and in the viewer tab; the tab-grid miniature is a picture.
fn interactive(ctx: &PrimaryBarContext) -> bool {
    matches!(ctx.host, HostMode::Fullscreen | HostMode::TabView)
}

fn fullscreen(ctx: &PrimaryBarContext) -> bool {
    ctx.host == HostMode::Fullscreen
}

const PINNED_REASON: &str = "unpin the view first";

fn send_to_tab(ctx: &PrimaryBarContext) -> Option<PrimaryActionSpec> {
    // Only a fullscreen that follows the Browse selection can become a tab; every other view already is one.
    (fullscreen(ctx) && ctx.origin == OriginKind::Browse).then(|| PrimaryActionSpec {
        id: PrimaryActionId::SendToTab,
        icon: Icon::OpenInBackgroundTab,
        tooltip: if ctx.pinned {
            format!("Send this view to a background tab (disabled: {PINNED_REASON})")
        } else {
            "Send this whole view to a background tab and close the fullscreen".to_string()
        },
        enabled: !ctx.pinned,
        active: None,
    })
}

fn close(ctx: &PrimaryBarContext) -> Option<PrimaryActionSpec> {
    interactive(ctx).then(|| {
        let (what, blocked) = if fullscreen(ctx) {
            ("Close fullscreen", ctx.pinned)
        } else {
            ("Close tab", false)
        };
        PrimaryActionSpec {
            id: PrimaryActionId::Close,
            icon: Icon::Close,
            tooltip: if blocked {
                format!("{what} (disabled: {PINNED_REASON})")
            } else {
                what.to_string()
            },
            enabled: !blocked,
            active: None,
        }
    })
}

/// One button for all videos; it needs at least two of them (one video has its own button in the media action bar).
fn mute_all(ctx: &PrimaryBarContext) -> Option<PrimaryActionSpec> {
    (interactive(ctx) && ctx.video_count >= 2).then(|| PrimaryActionSpec {
        id: PrimaryActionId::MuteAll,
        icon: if ctx.any_audible { Icon::VolumeOff } else { Icon::VolumeOn },
        tooltip: if ctx.any_audible {
            format!("Mute all {} videos", ctx.video_count)
        } else {
            format!("All {} videos are muted: unmute them all", ctx.video_count)
        },
        enabled: true,
        active: None,
    })
}

fn play_pause_all(ctx: &PrimaryBarContext) -> Option<PrimaryActionSpec> {
    (interactive(ctx) && ctx.video_count >= 1).then(|| {
        let many = ctx.video_count > 1;
        PrimaryActionSpec {
            id: PrimaryActionId::PlayPauseAll,
            icon: if ctx.any_playing { Icon::PlaybackPause } else { Icon::PlaybackPlay },
            tooltip: match (ctx.any_playing, many) {
                (true, true) => format!("Pause all {} videos", ctx.video_count),
                (true, false) => "Pause the video".to_string(),
                (false, true) => format!("All {} videos are paused: play them all", ctx.video_count),
                (false, false) => "The video is paused: play it".to_string(),
            },
            enabled: true,
            active: None,
        }
    })
}

fn reset_layout(ctx: &PrimaryBarContext) -> Option<PrimaryActionSpec> {
    (interactive(ctx) && ctx.tile_count >= 2).then(|| PrimaryActionSpec {
        id: PrimaryActionId::ResetLayout,
        icon: Icon::LayoutReset,
        tooltip: if ctx.layout_custom {
            "Reset the arrangement: all resizers go back to automatic".to_string()
        } else {
            "The arrangement is already automatic (disabled: no resizer was moved)".to_string()
        },
        enabled: ctx.layout_custom,
        active: None,
    })
}

fn hide_idle_cursor(ctx: &PrimaryBarContext) -> Option<PrimaryActionSpec> {
    interactive(ctx).then(|| {
        let seconds = f64::from(ctx.idle_cursor_ms) / 1000.0;
        PrimaryActionSpec {
            id: PrimaryActionId::HideIdleCursor,
            icon: if ctx.hide_idle_cursor { Icon::CursorHidden } else { Icon::CursorVisible },
            tooltip: if ctx.hide_idle_cursor {
                format!("The mouse cursor is hidden after {seconds:.1} s without movement (click to keep it visible)")
            } else {
                format!("Hide the mouse cursor after {seconds:.1} s without movement (it shows again when it moves)")
            },
            enabled: true,
            active: Some(ctx.hide_idle_cursor),
        }
    })
}

/// Only the fullscreen can be pinned: a tab can always be closed.
fn pin(ctx: &PrimaryBarContext) -> Option<PrimaryActionSpec> {
    fullscreen(ctx).then(|| PrimaryActionSpec {
        id: PrimaryActionId::Pin,
        icon: if ctx.pinned { Icon::Lock } else { Icon::LockOpen },
        tooltip: if ctx.pinned {
            "Pinned: the fullscreen cannot be closed (Esc, double click and the close button are blocked). Click to unpin"
                .to_string()
        } else {
            "Pin the fullscreen: it cannot be closed by accident until you unpin it".to_string()
        },
        enabled: true,
        active: Some(ctx.pinned),
    })
}

pub fn primary_actions_for(ctx: &PrimaryBarContext) -> Vec<PrimaryActionSpec> {
    PROVIDERS.iter().filter_map(|provider| provider(ctx)).collect()
}
