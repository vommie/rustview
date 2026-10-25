//! Where the native window of a video goes (pure). In the fullscreen and the tab view mpv's own window (the frame) always covers
//! the whole room of its tile, and the container window only clips it to what shows: the medium's place, or the whole room while
//! the video is zoomed. So a zoom that leaves 100 % or comes back to it changes only the clip and mpv's view, never the size of
//! mpv's window. A new size would show one stale frame: mpv follows the size of the window it is embedded in only after the X
//! server told it (`ConfigureNotify` in mpv's `video/out/x11_common.c`), so for a moment the old picture sits at the wrong place
//! or in a window of the wrong size, and the view meant for the new size is drawn into the old one.

use super::layout::Rect;
use crate::viewer::zoom::geometry::RectF;
use crate::viewer::zoom::mpv_view::{self, MpvView};
use rustview_core::util::cast::{Az, SaturatingAs};

/// The two rectangles of a video window, in host pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoPlacement {
    /// mpv's own window (the room of the tile in the fullscreen and the tab view, else the medium's place).
    pub frame: Rect,
    /// The part of the frame that shows (the container clips the frame to it); always inside the frame.
    pub window: Rect,
}

impl VideoPlacement {
    pub fn new(frame: Rect, window: Rect) -> Self {
        Self { frame, window }
    }

    /// The whole frame shows (a zoomed video, or one whose frame is its place): subtitles may use mpv's black borders.
    pub fn shows_all(&self) -> bool {
        self.frame == self.window
    }

    /// Whether more shows than at `old` (the clip grows, e.g. a zoom that leaves 100 %). Then mpv gets its new view before the
    /// clip grows; when the clip shrinks it shrinks first, so the moment between the two never shows mpv's background where
    /// the canvas belongs.
    pub fn grows_from(&self, old: Option<&Self>) -> bool {
        old.is_none_or(|old| area(self.window) > area(old.window))
    }

    /// The view that shows the plain fitted video at `home` (the medium's place, host pixels) inside the frame: what
    /// `MpvView::FIT` shows in a frame that is exactly `home`. For a video whose zoom does not know its size yet.
    pub fn home_view(&self, home: Rect) -> MpvView {
        let picture = RectF::new(
            (home.x - self.frame.x).az::<f64>(),
            (home.y - self.frame.y).az::<f64>(),
            home.w.az::<f64>(),
            home.h.az::<f64>(),
        );
        let size = (home.w.saturating_as::<u32>(), home.h.saturating_as::<u32>());
        mpv_view::view_for(size, (self.frame.w, self.frame.h), picture).unwrap_or(MpvView::FIT)
    }
}

fn area(rect: Rect) -> i64 {
    i64::from(rect.w.max(0)) * i64::from(rect.h.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_medium_that_fills_its_frame_is_mpvs_plain_view() {
        let home = Rect::new(100, 50, 1280, 720);
        let placement = VideoPlacement::new(home, home);
        assert!(placement.shows_all());
        assert!(placement.home_view(home).same_as(&MpvView::FIT));
    }

    #[test]
    fn a_medium_inside_a_taller_room_is_drawn_at_its_place() {
        // A 16:9 video in a column of 1154 x 751 at (100, 92): its place is 1154 x 649 in the middle; mpv alone would fit it
        // to the same width, so only the vertical offset differs: none, it is centered.
        let room = Rect::new(100, 92, 1154, 751);
        let home = Rect::new(100, 143, 1154, 649);
        let placement = VideoPlacement::new(room, home);
        assert!(!placement.shows_all());
        let view = placement.home_view(home);
        assert!(
            view.zoom.abs() < 1e-3 && view.pan_x.abs() < 1e-3 && view.pan_y.abs() < 1e-3,
            "{view:?}"
        );
    }

    #[test]
    fn a_medium_at_the_edge_of_a_wider_room_is_smaller_and_shifted() {
        // The room is 2000 x 1000, the medium 1000 x 500 in its top left corner: half of what mpv would fit (2000 x 1000),
        // its center a quarter of the room left and up from the room's center, i.e. half the picture's own size.
        let room = Rect::new(0, 0, 2000, 1000);
        let home = Rect::new(0, 0, 1000, 500);
        let view = VideoPlacement::new(room, home).home_view(home);
        assert!((view.zoom + 1.0).abs() < 1e-9, "{view:?}");
        assert!(
            (view.pan_x + 0.5).abs() < 1e-9 && (view.pan_y + 0.5).abs() < 1e-9,
            "{view:?}"
        );
    }

    #[test]
    fn the_clip_grows_when_the_zoom_leaves_the_medium_and_shrinks_when_it_comes_back() {
        let room = Rect::new(0, 0, 1000, 800);
        let home = VideoPlacement::new(room, Rect::new(0, 119, 1000, 562));
        let zoomed = VideoPlacement::new(room, room);
        assert!(zoomed.grows_from(Some(&home)));
        assert!(!home.grows_from(Some(&zoomed)));
        assert!(!zoomed.grows_from(Some(&zoomed)), "nothing changes");
        assert!(home.grows_from(None), "the first placement");
    }
}
