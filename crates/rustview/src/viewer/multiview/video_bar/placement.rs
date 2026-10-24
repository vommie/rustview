//! Where the control bar of a video goes (pure): at the bottom of the video, over its whole width (a pill keeps a margin to
//! the edges). Its window is taller than the bar: the bubble with the time under the pointer floats above the progress bar.
//! All rectangles are physical pixels in the coordinates of the host window.

use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::video_bar::prefs::BarStyle;
use rustview_core::settings::schema::{
    MULTIVIEW_VIDEO_BAR_MIN_VIDEO_WIDTH_LOGICAL_PX, MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX,
    MULTIVIEW_VIDEO_BAR_MINI_MIN_VIDEO_WIDTH_LOGICAL_PX, MULTIVIEW_VIDEO_BAR_PILL_INSET_LOGICAL_PX,
};
use rustview_core::util::cast::FloatToInt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoBarPlacement {
    /// The window of the bar.
    pub window: Rect,
    /// The bar inside it (at its bottom).
    pub bar: Rect,
}

/// The height of the time bubble above the bar, in logical pixels (it grows with the bar).
pub fn bubble_height(bar_height: f32) -> f32 {
    (bar_height * 0.8).round() + 4.0
}

/// The bar for the video window `video`, `None` when the video is too small to carry one.
pub fn place(video: Rect, style: BarStyle, bar_height: f32, scale: f32) -> Option<VideoBarPlacement> {
    place_with(
        video,
        style,
        bar_height,
        MULTIVIEW_VIDEO_BAR_MIN_VIDEO_WIDTH_LOGICAL_PX,
        scale,
    )
}

/// The small bar of a video in a tab grid item (a narrower video still carries it).
pub fn place_mini(video: Rect, scale: f32) -> Option<VideoBarPlacement> {
    place_with(
        video,
        BarStyle::Bar,
        MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX,
        MULTIVIEW_VIDEO_BAR_MINI_MIN_VIDEO_WIDTH_LOGICAL_PX,
        scale,
    )
}

fn place_with(video: Rect, style: BarStyle, bar_height: f32, min_width: f32, scale: f32) -> Option<VideoBarPlacement> {
    let scale = scale.max(0.5);
    let px = |logical: f32| (logical * scale).round().to_int::<i32>();
    let height = px(bar_height).max(1);
    let bubble = px(bubble_height(bar_height));
    let inset = match style {
        BarStyle::Bar => 0,
        BarStyle::Pill => px(MULTIVIEW_VIDEO_BAR_PILL_INSET_LOGICAL_PX),
    };
    if video.w < px(min_width).max(2 * inset + 1) || video.h < height + inset + bubble {
        return None;
    }
    let bar = Rect::new(
        video.x + inset,
        video.y + video.h - inset - height,
        video.w - 2 * inset,
        height,
    );
    Some(VideoBarPlacement {
        window: Rect::new(bar.x, bar.y - bubble, bar.w, height + bubble),
        bar,
    })
}

impl VideoBarPlacement {
    /// The same placement moved by (`dx`, `dy`): from coordinates of the host window to coordinates of the screen.
    pub fn translated(self, dx: i32, dy: i32) -> Self {
        let shift = |r: Rect| Rect::new(r.x + dx, r.y + dy, r.w, r.h);
        Self {
            window: shift(self.window),
            bar: shift(self.bar),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_spans_the_bottom_and_the_pill_keeps_a_margin() {
        let video = Rect::new(100, 50, 800, 450);
        let bar = place(video, BarStyle::Bar, 30.0, 1.0).unwrap();
        assert_eq!(bar.bar, Rect::new(100, 470, 800, 30));
        assert_eq!(bar.window, Rect::new(100, 442, 800, 58));
        let pill = place(video, BarStyle::Pill, 30.0, 1.0).unwrap();
        assert_eq!(pill.bar, Rect::new(110, 460, 780, 30));
        let scaled = place(video, BarStyle::Bar, 30.0, 2.0).unwrap();
        assert_eq!(scaled.bar.h, 60);
    }

    #[test]
    fn a_grid_item_carries_the_small_bar_down_to_a_narrower_video() {
        let video = Rect::new(0, 0, 150, 90);
        assert!(place(video, BarStyle::Bar, 30.0, 1.0).is_none());
        let mini = place_mini(video, 1.0).unwrap();
        assert_eq!(mini.bar, Rect::new(0, 68, 150, 22));
        assert!(place_mini(Rect::new(0, 0, 100, 90), 1.0).is_none());
    }

    #[test]
    fn a_small_video_gets_no_bar() {
        assert!(place(Rect::new(0, 0, 150, 400), BarStyle::Bar, 30.0, 1.0).is_none());
        assert!(place(Rect::new(0, 0, 400, 50), BarStyle::Bar, 30.0, 1.0).is_none());
        assert!(place(Rect::new(0, 0, 400, 80), BarStyle::Bar, 30.0, 1.0).is_some());
    }
}
