//! What both hosts (the fullscreen window and the viewer tab) do with the two action bars on every tick: ask the instance
//! what each bar would show, and show or hide its window. Keeping it here means the hosts only differ in the window they
//! draw in.

use super::bar_overlay::{self, BarKind, BarOwner, BarRequest};
use super::views;
use crate::viewer::multiview::instance::SharedInstance;
use std::rc::Rc;

/// What a host is: where its window is and how its bars react to clicks.
pub(super) struct BarHost {
    pub owner: BarOwner,
    pub host_xid: u64,
    pub scale: f32,
    pub media_click: Rc<dyn Fn(i32, i32)>,
    pub primary_click: Rc<dyn Fn(i32, i32)>,
}

/// Shows, moves or hides both bars. With `active` false (the host does not show this instance) both go away. Returns whether
/// any bar is visible now.
pub(super) fn update(host: &BarHost, instance: &SharedInstance, active: bool) -> bool {
    let (media, primary) = if active {
        let guard = instance.lock();
        (guard.media_bar_target(), guard.primary_bar_target())
    } else {
        (None, None)
    };
    // An open menu keeps its bar where it is, however far the pointer is, until the user clicks elsewhere.
    let media_menu_open = active && bar_overlay::service_menu(BarKind::Media, host.owner);
    let media_shown = media_menu_open || media.is_some_and(|target| {
        bar_overlay::show(&BarRequest {
            kind: BarKind::Media,
            owner: host.owner,
            host_xid: host.host_xid,
            tile: target.tile as i32,
            area: target.rect,
            pointer: Some(target.pointer),
            reveal_distance_px: target.reveal_distance_px,
            forced: false,
            scale: host.scale,
            actions: views::action_views(&target.actions),
            on_action: host.media_click.clone(),
        })
    });
    if !media_shown {
        bar_overlay::hide_kind(BarKind::Media, host.owner);
    }
    let primary_shown = primary.is_some_and(|target| {
        bar_overlay::show(&BarRequest {
            kind: BarKind::Primary,
            owner: host.owner,
            host_xid: host.host_xid,
            tile: -1,
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
