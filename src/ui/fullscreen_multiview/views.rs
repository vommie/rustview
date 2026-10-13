//! Conversion of layout rectangles (physical pixels) into the Slint view structs (logical pixels).

use super::layout::{Gutter, GutterKind, Rect};
use super::model::TileKind;
use super::tiles::TileRuntime;
use crate::ui::{MultiviewBadgeView, MultiviewGutterView, MultiviewTileView};
use slint::{Model, VecModel};

fn logical(px: i32, scale: f32) -> f32 {
    px as f32 / scale.max(0.01)
}

pub fn tile_view(tile: &TileRuntime, content: Rect, scale: f32) -> MultiviewTileView {
    let kind = if tile.error.is_some() {
        2
    } else if tile.kind == TileKind::Video {
        1
    } else {
        0
    };
    MultiviewTileView {
        x: logical(content.x, scale),
        y: logical(content.y, scale),
        width: logical(content.w, scale),
        height: logical(content.h, scale),
        kind,
        image: tile.image.clone().unwrap_or_default(),
        is_loading: tile.is_loading,
        error_text: tile.error.clone().unwrap_or_default().into(),
    }
}

pub fn gutter_view(gutter: &Gutter, scale: f32) -> MultiviewGutterView {
    MultiviewGutterView {
        x: logical(gutter.rect.x, scale),
        y: logical(gutter.rect.y, scale),
        width: logical(gutter.rect.w, scale),
        height: logical(gutter.rect.h, scale),
        vertical: gutter.kind == GutterKind::Vertical,
    }
}

pub fn badge_view(rect: Option<Rect>, muted: bool, scale: f32) -> MultiviewBadgeView {
    match rect {
        Some(r) => MultiviewBadgeView {
            visible: true,
            x: logical(r.x, scale),
            y: logical(r.y, scale),
            size: logical(r.w, scale),
            muted,
        },
        None => MultiviewBadgeView {
            visible: false,
            x: 0.0,
            y: 0.0,
            size: 0.0,
            muted,
        },
    }
}

/// Updates the rows in place when the row count is unchanged. Resetting the model would recreate the repeater items and
/// abort a gutter drag in progress (the dragged gutter is one of the items).
pub fn sync_model<T: Clone + 'static>(model: &VecModel<T>, items: Vec<T>) {
    if model.row_count() == items.len() {
        for (row, item) in items.into_iter().enumerate() {
            model.set_row_data(row, item);
        }
    } else {
        model.set_vec(items);
    }
}
