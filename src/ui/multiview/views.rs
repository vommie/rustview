//! Conversion of the Slint-free description of a Multiview (physical pixels) into the Slint view structs (logical pixels).

use crate::library::models::MediaId;
use crate::ui::button_feedback_bridge;
use crate::ui::{
    MultiviewActionView, MultiviewGutterView, MultiviewScreenView, MultiviewTileView,
};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::model::TileKind;
use crate::viewer::multiview::view::{ActionView, GutterView, HostedView, Navigation, TileView};
use slint::{Model, VecModel};
use std::collections::HashMap;
use std::rc::Rc;

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

fn action_view(action: &ActionView) -> MultiviewActionView {
    MultiviewActionView {
        id: action.id.code(),
        icon: action.icon.to_slint(),
        tooltip: action.tooltip.into(),
        toggle: action.active.is_some(),
        active: action.active.unwrap_or(false),
        feedback: button_feedback_bridge::to_slint(action.feedback),
    }
}

/// The `navigation` property of the fullscreen window: 0 = off, 1 = previous/next allowed, 2 = refused.
pub fn navigation_code(navigation: Navigation) -> i32 {
    match navigation {
        Navigation::Off => 0,
        Navigation::Allowed => 1,
        Navigation::Blocked => 2,
    }
}

/// The buttons of the action bar window.
pub fn action_views(actions: &[ActionView]) -> Vec<MultiviewActionView> {
    actions.iter().map(action_view).collect()
}

/// A logical rectangle in window pixels -> physical window pixels. Edges are rounded, not sizes, so neighbors stay seamless
/// and the result is never empty.
pub fn physical_rect(x: f32, y: f32, width: f32, height: f32, scale: f32) -> Rect {
    let edge = |logical: f32| (logical * scale).round() as i32;
    let (x0, y0) = (edge(x), edge(y));
    Rect::new(
        x0,
        y0,
        (edge(x + width) - x0).max(1),
        (edge(y + height) - y0).max(1),
    )
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

pub fn tile_views(view: &HostedView, images: &mut ImageCache) -> Vec<MultiviewTileView> {
    view.tiles
        .iter()
        .map(|t| tile_view(t, view.dpi, images))
        .collect()
}

pub fn gutter_views(view: &HostedView) -> Vec<MultiviewGutterView> {
    view.gutters
        .iter()
        .map(|g| gutter_view(g, view.dpi))
        .collect()
}

/// The Slint models a host draws a Multiview from.
#[derive(Default)]
pub struct ViewModels {
    pub tiles: Rc<VecModel<MultiviewTileView>>,
    pub gutters: Rc<VecModel<MultiviewGutterView>>,
}

impl ViewModels {
    /// Updates the models from the description of the instance.
    pub fn sync(&self, view: &HostedView, images: &mut ImageCache) {
        images.retain(&view.tiles);
        sync_model(&self.tiles, tile_views(view, images));
        sync_model(&self.gutters, gutter_views(view));
    }

    pub fn clear(&self) {
        self.tiles.set_vec(Vec::new());
        self.gutters.set_vec(Vec::new());
    }
}

/// Updates the rows in place when the row count is unchanged. Resetting the model would recreate the repeater items and
/// abort a gutter drag in progress (the dragged gutter is one of the items) or reset the hover of a bar button.
pub(super) fn sync_model<T: Clone + 'static>(model: &VecModel<T>, items: Vec<T>) {
    if model.row_count() == items.len() {
        for (row, item) in items.into_iter().enumerate() {
            model.set_row_data(row, item);
        }
    } else {
        model.set_vec(items);
    }
}
