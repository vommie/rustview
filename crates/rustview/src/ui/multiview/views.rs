//! Conversion of the Slint-free description of a Multiview (physical pixels) into the Slint view structs (logical pixels).

use crate::ui::button_feedback::ButtonFeedback;
use crate::ui::button_feedback_bridge;
use crate::ui::{MultiviewBarButtonView, MultiviewGutterView, MultiviewScreenView, MultiviewTileView};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::media_actions::ButtonLook;
use crate::viewer::multiview::model::TileKind;
use crate::viewer::multiview::view::{
    GutterView, HostedView, MediaActionView, MinimapPicture, Navigation, PrimaryActionView, TileView, ZoomContentView,
};
use rustview_core::util::cast::{Az, FloatToInt, SaturatingAs};
use slint::{Model, VecModel};
use std::collections::HashMap;
use std::rc::Rc;

fn logical(px: i32, dpi: f32) -> f32 {
    px.az::<f32>() / dpi.max(0.01)
}

/// The Slint images of the tiles. A `slint::Image` is not `Send`, so the instance only holds the shared pixel buffer; the
/// presenter converts it once per buffer (a new buffer has a new address) and hands the same image to Slint afterwards,
/// which keeps the renderer from uploading the texture again on every sync.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<TileKey, (usize, slint::Image)>,
    /// The sharp pictures of zoomed images and the thumbnails of their minimaps: one of each per tile, kept for as long as the
    /// buffer is the same.
    zoomed: HashMap<TileKey, (usize, slint::Image)>,
    thumbs: HashMap<TileKey, (usize, slint::Image)>,
}

/// The Slint image of `pixels`, made once per buffer (a new buffer has a new address).
fn cached_image(
    entries: &mut HashMap<TileKey, (usize, slint::Image)>,
    tile: TileKey,
    pixels: &slint::SharedPixelBuffer<slint::Rgb8Pixel>,
) -> slint::Image {
    let key = pixels.as_bytes().as_ptr() as usize;
    if let Some((cached_key, image)) = entries.get(&tile)
        && *cached_key == key
    {
        return image.clone();
    }
    let image = slint::Image::from_rgb8(pixels.clone());
    entries.insert(tile, (key, image.clone()));
    image
}

impl ImageCache {
    fn image_for(&mut self, tile: &TileView) -> slint::Image {
        match tile.pixels.as_ref() {
            Some(pixels) => cached_image(&mut self.entries, tile.key, pixels),
            None => slint::Image::default(),
        }
    }

    fn zoomed_image_for(&mut self, tile: &TileView) -> slint::Image {
        if let Some(ZoomContentView::Rendered { pixels, .. }) = tile.zoom.as_ref().map(|z| &z.content) {
            cached_image(&mut self.zoomed, tile.key, pixels)
        } else {
            self.zoomed.remove(&tile.key);
            slint::Image::default()
        }
    }

    fn thumb_for(&mut self, tile: &TileView) -> slint::Image {
        match tile.minimap.as_ref().map(|m| &m.picture) {
            Some(MinimapPicture::Thumb(thumb)) => cached_image(&mut self.thumbs, tile.key, thumb),
            Some(MinimapPicture::Video(_)) | None => slint::Image::default(),
        }
    }

    fn retain(&mut self, tiles: &[TileView]) {
        for entries in [&mut self.entries, &mut self.zoomed, &mut self.thumbs] {
            entries.retain(|key, _| tiles.iter().any(|t| t.key == *key));
        }
    }
}

fn tile_view(tile: &TileView, dpi: f32, images: &mut ImageCache) -> MultiviewTileView {
    let kind = if tile.error.is_some() {
        2
    } else {
        i32::from(tile.kind == TileKind::Video)
    };
    let Rect { x, y, w, h } = tile.rect;
    let image = images.image_for(tile);
    let zoom_image = images.zoomed_image_for(tile);
    let mm_image = images.thumb_for(tile);
    let minimap_area = tile.minimap.as_ref().map(|m| m.area);
    let (zoomed, zoom_hq, zoom_area, zoom_rect, zoom_clip, zoom_pixelated) = match tile.zoom.as_ref() {
        Some(zoom) => {
            let hq = matches!(zoom.content, ZoomContentView::Rendered { .. });
            let clip = match &zoom.content {
                ZoomContentView::Preview { clip } | ZoomContentView::Rendered { clip, .. } => *clip,
            };
            (true, hq, zoom.area, zoom.dest, clip, zoom.pixelated)
        }
        None => (
            false,
            false,
            minimap_area.unwrap_or_default(),
            Rect::default(),
            Rect::default(),
            false,
        ),
    };
    let minimap = tile.minimap.as_ref();
    let mm_rect = minimap.map_or(Rect::default(), |m| m.rect);
    let mm_viewport = minimap.map_or((0.0, 0.0, 1.0, 1.0), |m| m.viewport);
    let bubble = minimap.and_then(|m| m.bubble.as_ref());
    let crop = tile.crop.as_ref();
    let crop_room = crop.map_or(Rect::default(), |c| c.clip);
    let fl = |v: f64| v.az::<f32>() / dpi.max(0.01);
    let crop_pic = crop.map_or((0.0, 0.0, 0.0, 0.0), |c| c.picture);
    let crop_rect = crop.map_or((0.0, 0.0, 0.0, 0.0), |c| c.rect);
    let focus_view = tile.focus.map_or(Rect::default(), |f| f.view);
    let volume = tile.volume;
    let volume_rect = volume.map_or(Rect::default(), |v| v.rect);
    let video_row = tile.video.map_or(-1, |v| v.row.saturating_as::<i32>());
    let (video_frame, video_clip) = tile
        .video
        .map_or((Rect::default(), Rect::default()), |v| (v.frame, v.clip));
    MultiviewTileView {
        x: logical(x, dpi),
        y: logical(y, dpi),
        width: logical(w, dpi),
        height: logical(h, dpi),
        kind,
        image,
        video_row,
        video_frame_x: logical(video_frame.x, dpi),
        video_frame_y: logical(video_frame.y, dpi),
        video_frame_width: logical(video_frame.w, dpi),
        video_frame_height: logical(video_frame.h, dpi),
        video_clip_x: logical(video_clip.x, dpi),
        video_clip_y: logical(video_clip.y, dpi),
        video_clip_width: logical(video_clip.w, dpi),
        video_clip_height: logical(video_clip.h, dpi),
        is_loading: tile.is_loading,
        error_text: tile.error.clone().unwrap_or_default().into(),
        zoomed,
        zoom_hq,
        zoom_image,
        zoom_area_x: logical(zoom_area.x, dpi),
        zoom_area_y: logical(zoom_area.y, dpi),
        zoom_area_width: logical(zoom_area.w, dpi),
        zoom_area_height: logical(zoom_area.h, dpi),
        zoom_x: logical(zoom_rect.x, dpi),
        zoom_y: logical(zoom_rect.y, dpi),
        zoom_width: logical(zoom_rect.w, dpi),
        zoom_height: logical(zoom_rect.h, dpi),
        zoom_clip_x: zoom_clip.x,
        zoom_clip_y: zoom_clip.y,
        zoom_clip_width: zoom_clip.w,
        zoom_clip_height: zoom_clip.h,
        zoom_pixelated,
        pannable: tile.pannable,
        mm_available: minimap.is_some(),
        mm_visible: minimap.is_some_and(|m| m.visible),
        mm_image,
        mm_video_row: match minimap.map(|m| &m.picture) {
            Some(MinimapPicture::Video(row)) => (*row).saturating_as::<i32>(),
            _ => -1,
        },
        mm_x: logical(mm_rect.x, dpi),
        mm_y: logical(mm_rect.y, dpi),
        mm_width: logical(mm_rect.w, dpi),
        mm_height: logical(mm_rect.h, dpi),
        mm_vx: mm_viewport.0.az::<f32>(),
        mm_vy: mm_viewport.1.az::<f32>(),
        mm_vw: mm_viewport.2.az::<f32>(),
        mm_vh: mm_viewport.3.az::<f32>(),
        mm_bubble_open: bubble.is_some(),
        mm_zoom_permille: bubble.map_or(1000, |b| b.permille),
        mm_zoom_min: bubble.map_or(100, |b| b.min_permille),
        mm_zoom_max: bubble.map_or(10000, |b| b.max_permille),
        mm_zoom_snaps: slint::ModelRc::new(slint::VecModel::from(bubble.map_or_else(Vec::new, |b| b.snaps.clone()))),
        feedback_icon: tile.feedback.map_or(-1, |pulse| pulse.icon.code()),
        feedback_alpha: tile.feedback.map_or(0.0, |pulse| pulse.alpha),
        volume_shown: volume.is_some(),
        volume_x: logical(volume_rect.x, dpi),
        volume_y: logical(volume_rect.y, dpi),
        volume_width: logical(volume_rect.w, dpi),
        volume_height: logical(volume_rect.h, dpi),
        volume_gap: logical(volume.map_or(0, |v| v.gap), dpi),
        volume_share: volume.map_or(0.0, |v| v.share),
        volume_icon: volume.map_or(0, |v| v.icon.code()),
        volume_muted: volume.is_some_and(|v| v.muted),
        volume_alpha: volume.map_or(0.0, |v| v.alpha),
        crop_open: crop.is_some(),
        crop_room_x: logical(crop_room.x, dpi),
        crop_room_y: logical(crop_room.y, dpi),
        crop_room_width: logical(crop_room.w, dpi),
        crop_room_height: logical(crop_room.h, dpi),
        crop_pic_x: fl(crop_pic.0),
        crop_pic_y: fl(crop_pic.1),
        crop_pic_width: fl(crop_pic.2),
        crop_pic_height: fl(crop_pic.3),
        crop_x: fl(crop_rect.0),
        crop_y: fl(crop_rect.1),
        crop_width: fl(crop_rect.2),
        crop_height: fl(crop_rect.3),
        crop_size_label: crop.map_or_else(Default::default, |c| c.size_label.as_str().into()),
        crop_ratio_label: crop.map_or_else(Default::default, |c| c.ratio_label.as_str().into()),
        crop_grid: crop.is_some_and(|c| c.grid),
        crop_dragging: crop.is_some_and(|c| c.dragging),
        focused: tile.focus.is_some(),
        focus_view_x: logical(focus_view.x, dpi),
        focus_view_y: logical(focus_view.y, dpi),
        focus_view_width: logical(focus_view.w, dpi),
        focus_view_height: logical(focus_view.h, dpi),
        focus_brighten: tile.focus.map_or(0.0, |f| f.brighten),
        focus_corner: logical(tile.focus.map_or(0, |f| f.corner), dpi),
        focus_thickness: logical(tile.focus.map_or(0, |f| f.thickness), dpi),
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
        swap_only: gutter.swap_only,
    }
}

fn action_view(action: &MediaActionView) -> MultiviewBarButtonView {
    let (emphasis, fading) = match action.look {
        ButtonLook::Normal => (0, false),
        ButtonLook::Emphasized => (1, false),
        ButtonLook::Busy => (2, false),
        ButtonLook::Done { fading } => (3, fading),
    };
    MultiviewBarButtonView {
        id: action.id.code(),
        icon: action.icon.to_slint(),
        tooltip: action.tooltip.as_str().into(),
        toggle: action.active.is_some(),
        active: action.active.unwrap_or(false),
        enabled: action.enabled,
        attention: false,
        feedback: button_feedback_bridge::to_slint(action.feedback),
        emphasis,
        fading,
        split: action.split,
    }
}

fn primary_action_view(action: &PrimaryActionView) -> MultiviewBarButtonView {
    MultiviewBarButtonView {
        id: action.id.code(),
        icon: action.icon.to_slint(),
        tooltip: action.tooltip.as_str().into(),
        toggle: action.active.is_some(),
        active: action.active.unwrap_or(false),
        enabled: action.enabled,
        attention: action.attention,
        feedback: button_feedback_bridge::to_slint(ButtonFeedback::default()),
        emphasis: 0,
        fading: false,
        split: false,
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

/// The buttons of the primary action bar.
pub fn primary_action_views(actions: &[PrimaryActionView]) -> Vec<MultiviewBarButtonView> {
    actions.iter().map(primary_action_view).collect()
}

/// The buttons of the media action bar.
pub fn action_views(actions: &[MediaActionView]) -> Vec<MultiviewBarButtonView> {
    actions.iter().map(action_view).collect()
}

/// A logical rectangle in window pixels -> physical window pixels. Edges are rounded, not sizes, so neighbors stay seamless
/// and the result is never empty.
pub fn physical_rect(x: f32, y: f32, width: f32, height: f32, scale: f32) -> Rect {
    let edge = |logical: f32| (logical * scale).round().to_int::<i32>();
    let (x0, y0) = (edge(x), edge(y));
    Rect::new(x0, y0, (edge(x + width) - x0).max(1), (edge(y + height) - y0).max(1))
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
    view.tiles.iter().map(|t| tile_view(t, view.dpi, images)).collect()
}

pub fn gutter_views(view: &HostedView) -> Vec<MultiviewGutterView> {
    view.gutters.iter().map(|g| gutter_view(g, view.dpi)).collect()
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
