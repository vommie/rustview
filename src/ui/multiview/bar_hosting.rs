//! What both hosts (the fullscreen window and the viewer tab) do with the two action bars on every tick: ask the instance
//! what each bar would show, and show or hide its window. Keeping it here means the hosts only differ in the window they
//! draw in.

use super::bar_overlay::{self, BarKind, BarOwner, BarRequest};
use super::views;
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::view::{MediaBarTarget, PrimaryBarTarget, ZoomBubbleView};
use std::rc::Rc;

/// What a host is: where its window is and how its bars react to clicks.
pub(super) struct BarHost {
    pub owner: BarOwner,
    pub host_xid: u64,
    pub scale: f32,
    pub media_click: Rc<dyn Fn(i32, i32)>,
    pub primary_click: Rc<dyn Fn(i32, i32)>,
}

/// What the content of a host offers its two bars right now.
#[derive(Default)]
pub(super) struct BarTargets {
    /// The media bars to show, one per tile at most.
    pub media: Vec<MediaBarTarget>,
    pub primary: Option<PrimaryBarTarget>,
    /// The zoom sliders the user has open, one per tile.
    pub zoom: Vec<ZoomBubbleView>,
    /// How many tiles there are (each gets a media bar window of its own, made ahead of time).
    pub tiles: usize,
}

impl BarTargets {
    /// What the Multiview `instance` offers `owner`'s bars; nothing while the host does not show it (`active` false).
    pub fn of_instance(owner: BarOwner, instance: &SharedInstance, active: bool) -> Self {
        if !active {
            return Self::default();
        }
        let open_menus = bar_overlay::open_menu_keys(owner);
        let guard = instance.lock();
        Self {
            media: guard.media_bar_targets(&open_menus),
            primary: guard.primary_bar_target(),
            zoom: guard.zoom_bubbles(),
            tiles: guard.tile_count(),
        }
    }
}

/// Shows, moves or hides both bars. With `active` false (the host does not show its content) both go away. Returns whether
/// any bar is visible now.
pub(super) fn update(host: &BarHost, targets: BarTargets, active: bool) -> bool {
    let BarTargets { media, primary, zoom, tiles } = targets;
    if active {
        bar_overlay::prepare_media_bars(tiles);
        bar_overlay::service_menus(host.owner);
    }
    // Every tile has a media bar of its own. The one of the tile under the pointer appears with the pointer; one whose menu or slider
    // is open stays whatever the pointer does and follows its tile, so any number can be open at once and none blocks another.
    let mut kept: Vec<TileKey> = Vec::new();
    for target in media {
        let visible = bar_overlay::show(&BarRequest {
            kind: BarKind::Media,
            owner: host.owner,
            host_xid: host.host_xid,
            tile: target.tile as i32,
            key: target.key,
            area: target.rect,
            pointer: target.pointer,
            reveal_distance_px: target.reveal_distance_px,
            forced: target.forced,
            scale: host.scale,
            actions: views::action_views(&target.actions),
            on_action: host.media_click.clone(),
        });
        if visible {
            kept.push(target.key);
        }
    }
    bar_overlay::sync_zooms(host.owner, &zoom);
    bar_overlay::hide_kind_except(BarKind::Media, host.owner, &kept);
    let media_shown = !kept.is_empty();
    let primary_shown = primary.is_some_and(|target| {
        bar_overlay::show(&BarRequest {
            kind: BarKind::Primary,
            owner: host.owner,
            host_xid: host.host_xid,
            tile: -1,
            key: 0,
            area: target.area,
            pointer: target.pointer,
            reveal_distance_px: target.reveal_distance_px,
            forced: target.forced,
            scale: host.scale,
            actions: views::primary_action_views(&target.actions),
            on_action: host.primary_click.clone(),
        })
    });
    if !primary_shown {
        bar_overlay::hide_kind(BarKind::Primary, host.owner);
    }
    media_shown || primary_shown
}
