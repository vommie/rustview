//! Conversion of the Slint-free description of a Multiview (physical pixels) into the Slint view structs (logical pixels).

use crate::library::models::MediaId;
use crate::ui::{MultiviewBadgeView, MultiviewGutterView, MultiviewScreenView, MultiviewTileView};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::model::TileKind;
use crate::viewer::multiview::view::{BadgeView, GutterView, HostedView, TileView};
use slint::{Model, VecModel};
use std::collections::HashMap;

fn logical(px: i32, dpi: f32) -> f32 {
    px as f32 / dpi.max(0.01)
}

/// The Slint images of the tiles. A `slint::Image` is not `Send`, so the instance only holds the shared pixel buffer; the
/// presenter converts it once per buffer (a new buffer has a new address) and hands the same image to Slint afterwards,
/// which keeps the renderer from uploading the texture again on every sync.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<MediaId, (usize, slint::Image)>,
}

impl ImageCache {
    fn image_for(&mut self, tile: &TileView) -> slint::Image {
        let Some(pixels) = tile.pixels.as_ref() else {
            return slint::Image::default();
        };
        let key = pixels.as_bytes().as_ptr() as usize;
        if let Some((cached_key, image)) = self.entries.get(&tile.media_id)
            && *cached_key == key
        {
            return image.clone();
        }
        let image = slint::Image::from_rgb8(pixels.clone());
        self.entries.insert(tile.media_id, (key, image.clone()));
        image
    }

    fn retain(&mut self, tiles: &[TileView]) {
        self.entries
            .retain(|id, _| tiles.iter().any(|t| t.media_id == *id));
    }
}

fn tile_view(tile: &TileView, dpi: f32, images: &mut ImageCache) -> MultiviewTileView {
    let kind = if tile.error.is_some() {
        2
    } else if tile.kind == TileKind::Video {
        1
    } else {
        0
    };
    let Rect { x, y, w, h } = tile.rect;
    MultiviewTileView {
        x: logical(x, dpi),
        y: logical(y, dpi),
        width: logical(w, dpi),
        height: logical(h, dpi),
        kind,
        image: images.image_for(tile),
        is_loading: tile.is_loading,
        error_text: tile.error.clone().unwrap_or_default().into(),
    }
}

fn gutter_view(gutter: &GutterView, dpi: f32) -> MultiviewGutterView {
    let Rect { x, y, w, h } = gutter.rect;
    MultiviewGutterView {
        x: logical(x, dpi),
        y: logical(y, dpi),
        width: logical(w, dpi),
        height: logical(h, dpi),
        vertical: gutter.vertical,
    }
}

pub fn badge_view(badge: Option<&BadgeView>, dpi: f32) -> MultiviewBadgeView {
    match badge {
        Some(b) => MultiviewBadgeView {
            visible: true,
            x: logical(b.rect.x, dpi),
            y: logical(b.rect.y, dpi),
            size: logical(b.rect.w, dpi),
            muted: b.muted,
        },
        None => MultiviewBadgeView {
            visible: false,
            x: 0.0,
            y: 0.0,
            size: 0.0,
            muted: false,
        },
    }
}

/// Where the projected container sits inside the canvas of the host.
pub fn screen_view(view: &HostedView) -> MultiviewScreenView {
    let Rect { x, y, w, h } = view.screen;
    MultiviewScreenView {
        x: logical(x, view.dpi),
        y: logical(y, view.dpi),
        width: logical(w, view.dpi),
        height: logical(h, view.dpi),
    }
}

/// Updates the Slint models from the description of the instance.
pub fn sync_models(
    tile_model: &VecModel<MultiviewTileView>,
    gutter_model: &VecModel<MultiviewGutterView>,
    view: &HostedView,
    images: &mut ImageCache,
) {
    images.retain(&view.tiles);
    let tiles = view
        .tiles
        .iter()
        .map(|t| tile_view(t, view.dpi, images))
        .collect();
    sync_model(tile_model, tiles);
    let gutters = view
        .gutters
        .iter()
        .map(|g| gutter_view(g, view.dpi))
        .collect();
    sync_model(gutter_model, gutters);
}

/// Updates the rows in place when the row count is unchanged. Resetting the model would recreate the repeater items and
/// abort a gutter drag in progress (the dragged gutter is one of the items).
fn sync_model<T: Clone + 'static>(model: &VecModel<T>, items: Vec<T>) {
    if model.row_count() == items.len() {
        for (row, item) in items.into_iter().enumerate() {
            model.set_row_data(row, item);
        }
    } else {
        model.set_vec(items);
    }
}
