//! What a row of an action bar shows (`MultiviewActionBarView`), made from what the instance offers.

use super::{OpenMenu, Slot};
use crate::ui::multiview::views::{self, sync_model};
use crate::ui::{MultiviewActionBarView, MultiviewBarButtonView};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::view::{MediaBarTarget, PrimaryBarTarget, ZoomBubbleView};
use rustview_core::util::cast::{Az, SaturatingAs, WrappingAs};
use slint::ModelRc;

/// The row of the primary bar.
pub(super) fn primary_row(slot: &mut Slot, target: &PrimaryBarTarget, scale: f32) -> MultiviewActionBarView {
    sync_actions(slot, views::primary_action_views(&target.actions));
    MultiviewActionBarView {
        key: 0,
        primary: true,
        actions: ModelRc::from(slot.actions.clone()),
        split_index: -1,
        menu_owner_index: -1,
        ..placed(target.bar, scale)
    }
}

/// The row of the media bar of a tile, with its drop-down (`menu`) or its zoom slider (`zoom`) when one is open.
pub(super) fn media_row(
    slot: &mut Slot,
    target: &MediaBarTarget,
    menu: Option<&OpenMenu>,
    zoom: Option<&ZoomBubbleView>,
    scale: f32,
) -> MultiviewActionBarView {
    sync_actions(slot, views::action_views(&target.actions));
    let split = slot.last_actions.iter().position(|a| a.split);
    let mut row = MultiviewActionBarView {
        key: target.key.wrapping_as::<i32>(),
        primary: false,
        actions: ModelRc::from(slot.actions.clone()),
        split_index: split.map_or(-1, SaturatingAs::saturating_as::<i32>),
        ..placed(target.bar, scale)
    };
    match zoom {
        Some(zoom) => {
            if slot.last_snaps != zoom.snaps {
                slot.snaps.set_vec(zoom.snaps.clone());
                slot.last_snaps.clone_from(&zoom.snaps);
            }
            row.zoom_open = true;
            row.zoom_permille = zoom.permille;
            row.zoom_min_permille = zoom.min_permille;
            row.zoom_max_permille = zoom.max_permille;
            row.zoom_snaps = ModelRc::from(slot.snaps.clone());
            row.menu_owner_index = position_of(slot, MediaActionId::Zoom.code());
        }
        None => show_menu(&mut row, slot, menu),
    }
    row
}

/// Puts the drop-down `menu` into `row` (or takes it out).
pub(super) fn show_menu(row: &mut MultiviewActionBarView, slot: &Slot, menu: Option<&OpenMenu>) {
    if let Some(menu) = menu {
        row.menu_open = true;
        row.menu_items = menu.items.clone();
        row.menu_owner_index = position_of(slot, menu.opener);
    } else {
        row.menu_open = false;
        row.menu_items = ModelRc::default();
        if !row.zoom_open {
            row.menu_owner_index = -1;
        }
    }
}

/// Where the button with `code` is in the bar (-1 = not there).
fn position_of(slot: &Slot, code: i32) -> i32 {
    slot.last_actions
        .iter()
        .position(|a| a.id == code)
        .map_or(-1, SaturatingAs::saturating_as::<i32>)
}

/// A row at `bar` (physical pixels relative to the canvas) with nothing else set.
fn placed(bar: Rect, scale: f32) -> MultiviewActionBarView {
    let scale = scale.max(0.5);
    MultiviewActionBarView {
        x: bar.x.az::<f32>() / scale,
        y: bar.y.az::<f32>() / scale,
        ..Default::default()
    }
}

/// The buttons in the row's own list, changed only when they changed.
fn sync_actions(slot: &mut Slot, actions: Vec<MultiviewBarButtonView>) {
    if slot.last_actions != actions {
        sync_model(&slot.actions, actions.clone());
        slot.last_actions = actions;
    }
}
