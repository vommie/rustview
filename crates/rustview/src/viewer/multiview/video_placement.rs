//! Where a video goes (pure). mpv renders the whole picture, always fitted, into a texture of the picture's own aspect; the canvas
//! draws that texture where the picture belongs, exactly like a picture: at the medium's place, or - while the video is zoomed -
//! where the zoom model puts the picture (`ZoomState::image_rect`, it may reach out of the room), clipped to the room of the tile.
//! So zooming and panning move and scale an image in Slint; mpv never gets a view of its own, and the minimap can show the same
//! texture in small.
//!
//! The texture has the size the picture is drawn at, but never more pixels than the video has (a zoom beyond its own size is
//! scaled up by the renderer). While a zoom animates the texture keeps its size until it falls far behind, so an animation does
//! not reallocate the textures on every step; a texture of the old size drawn at the new place is only softer for a moment,
//! never in the wrong place.

use super::layout::{Rect, fit};
use crate::viewer::zoom::geometry::RectF;
use rustview_core::util::cast::{Az, FloatToInt};

/// The longest edge a video texture may have (every GL 3 implementation allows it).
const MAX_TEXTURE_EDGE: u32 = 8192;

/// The two rectangles of a video, in host pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoPlacement {
    /// Where the whole picture (the texture) is drawn.
    pub frame: Rect,
    /// What of it shows: the medium's place, or the room of the tile while the video is zoomed.
    pub clip: Rect,
}

impl VideoPlacement {
    /// The plain video in its `place` (a lone video's place is the whole view): the picture fitted and centered in it when its
    /// `aspect` (width / height) is known, else the whole place (mpv letterboxes into it).
    pub fn plain(place: Rect, aspect: Option<f32>) -> Self {
        let frame = match aspect.filter(|a| a.is_finite() && *a > 0.0) {
            Some(aspect) => {
                let (w, h) = fit(aspect, place.w, place.h);
                Rect::new(place.x + (place.w - w) / 2, place.y + (place.h - h) / 2, w, h)
            }
            None => place,
        };
        Self { frame, clip: place }
    }

    /// A zoomed video: the picture at `image` (relative to `room`, from the zoom model), clipped to `room`.
    pub fn zoomed(room: Rect, image: RectF) -> Self {
        let frame = Rect::new(
            room.x + image.x.round().to_int::<i32>(),
            room.y + image.y.round().to_int::<i32>(),
            image.w.round().to_int::<i32>().max(1),
            image.h.round().to_int::<i32>().max(1),
        );
        Self { frame, clip: room }
    }

    /// The part of the picture that shows (`None` when nothing of it does).
    pub fn visible(&self) -> Option<Rect> {
        let (a, b) = (self.frame, self.clip);
        let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
        let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// The size of the texture: the size the picture is drawn at, scaled down (keeping its aspect) to at most the video's own
    /// `native` size (its display size, when known) and to `MAX_TEXTURE_EDGE`.
    pub fn texture_size(&self, native: Option<(u32, u32)>) -> (u32, u32) {
        let (w, h) = (self.frame.w.max(1).az::<f64>(), self.frame.h.max(1).az::<f64>());
        let mut scale: f64 = 1.0;
        if let Some((nw, nh)) = native.filter(|&(nw, nh)| nw > 0 && nh > 0) {
            scale = scale.min(f64::from(nw) / w).min(f64::from(nh) / h);
        }
        let edge = f64::from(MAX_TEXTURE_EDGE);
        scale = scale.min(edge / w).min(edge / h);
        let size = |v: f64| (v * scale).round().to_int::<u32>().max(1);
        (size(w), size(h))
    }
}

/// Whether a texture of `current` size may stay while the zoom animates toward a picture that wants `wanted`: it is not empty and
/// has at least half the wanted width (a larger one costs nothing more, it exists already).
pub fn keeps_texture(current: (u32, u32), wanted: (u32, u32), animating: bool) -> bool {
    current == wanted || animating && current.0 > 0 && current.1 > 0 && current.0.saturating_mul(2) >= wanted.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_video_is_drawn_and_shown_at_its_place() {
        let place = Rect::new(100, 143, 1154, 649);
        let placement = VideoPlacement::plain(place, Some(16.0 / 9.0));
        assert_eq!(placement.visible(), Some(place));
        assert_eq!(placement.texture_size(Some((1920, 1080))), (1154, 649));
        assert_eq!(VideoPlacement::plain(place, None).frame, place, "aspect not known yet");
    }

    #[test]
    fn a_lone_portrait_video_is_drawn_at_its_picture_in_the_whole_view() {
        let view = Rect::new(0, 0, 1600, 900);
        let placement = VideoPlacement::plain(view, Some(1080.0 / 1920.0));
        assert_eq!(placement.frame, Rect::new(547, 0, 506, 900));
        assert_eq!(placement.clip, view);
        assert_eq!(placement.visible(), Some(placement.frame));
        assert_eq!(placement.texture_size(Some((1080, 1920))), (506, 900));
    }

    #[test]
    fn a_zoomed_video_is_drawn_at_the_zoom_models_place_and_clipped_to_the_room() {
        let room = Rect::new(100, 92, 1000, 800);
        let placement = VideoPlacement::zoomed(room, RectF::new(-500.0, -200.4, 2000.0, 1125.0));
        assert_eq!(placement.frame, Rect::new(-400, -108, 2000, 1125));
        assert_eq!(placement.clip, room);
        assert_eq!(placement.visible(), Some(room), "the picture covers the room");
    }

    #[test]
    fn a_zoomed_out_video_shows_only_the_picture() {
        let room = Rect::new(0, 0, 1000, 800);
        let placement = VideoPlacement::zoomed(room, RectF::new(250.0, 259.0, 500.0, 281.0));
        assert_eq!(placement.visible(), Some(Rect::new(250, 259, 500, 281)));
    }

    #[test]
    fn the_texture_never_has_more_pixels_than_the_video() {
        let room = Rect::new(0, 0, 1000, 800);
        let placement = VideoPlacement::zoomed(room, RectF::new(-3000.0, -2000.0, 7680.0, 4320.0));
        assert_eq!(placement.texture_size(Some((1920, 1080))), (1920, 1080));
        let portrait = VideoPlacement::zoomed(room, RectF::new(0.0, -1000.0, 1000.0, 1778.0));
        assert_eq!(
            portrait.texture_size(Some((1080, 1920))),
            (1000, 1778),
            "below its own size"
        );
    }

    #[test]
    fn the_texture_stays_below_the_gl_limit() {
        let room = Rect::new(0, 0, 1000, 800);
        let placement = VideoPlacement::zoomed(room, RectF::new(0.0, 0.0, 20000.0, 10000.0));
        assert_eq!(placement.texture_size(None), (8192, 4096));
    }

    #[test]
    fn an_animation_keeps_the_texture_until_it_falls_far_behind() {
        assert!(keeps_texture((1000, 562), (1000, 562), false));
        assert!(!keeps_texture((1000, 562), (1200, 675), false), "settled: exact size");
        assert!(keeps_texture((1000, 562), (1900, 1069), true));
        assert!(!keeps_texture((1000, 562), (2100, 1181), true), "less than half");
        assert!(
            keeps_texture((1920, 1080), (800, 450), true),
            "zooming out keeps the larger one"
        );
        assert!(!keeps_texture((0, 0), (800, 450), true), "an empty texture never stays");
    }
}
