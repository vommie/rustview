//! Where the control bar of a video goes (pure): at the bottom of the video, over its whole width (a pill keeps a margin to
//! the edges). Its rectangle is taller than the bar: the bubble with the time under the pointer floats above the progress bar,
//! and the A-B group adds its room. All rectangles are physical pixels in the coordinates of the host.

use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::video_bar::prefs::BarStyle;
use rustview_core::settings::schema::{
    MULTIVIEW_VIDEO_BAR_MIN_VIDEO_WIDTH_LOGICAL_PX, MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX,
    MULTIVIEW_VIDEO_BAR_MINI_MIN_VIDEO_WIDTH_LOGICAL_PX, MULTIVIEW_VIDEO_BAR_PILL_INSET_LOGICAL_PX,
};
use rustview_core::util::cast::{Az, FloatToInt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoBarPlacement {
    /// The bar with the room above it (the canvas draws the bar component there).
    pub window: Rect,
    /// The bar inside it (at its bottom).
    pub bar: Rect,
}

/// The height of the time bubble above the bar, in logical pixels (it grows with the bar).
pub fn bubble_height(bar_height: f32) -> f32 {
    (bar_height * 0.8).round() + 4.0
}

/// The room of the A-B repeat group above the bar, in logical pixels (`ui/multiview_video_bar.slint` draws into it):
/// the strip of saved repeats (a third of the bar, at least 12 px), a row for the flags of A and B and a row for the label of
/// the repeat under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbRoom {
    pub strip: f32,
    pub row: f32,
    /// How far the concave flank at each end of the strip's tab reaches out into the bar (like a Chrome tab).
    pub flank: f32,
}

impl AbRoom {
    pub fn for_bar(bar_height: f32) -> Self {
        let strip = (bar_height / 3.0).round().max(12.0);
        Self {
            strip,
            row: (bar_height * 0.54).round().max(14.0),
            flank: (strip * 1.5).round(),
        }
    }

    /// The whole room above the bar (the time bubble above the strip fits into it as well).
    pub fn above_bar(self, bar_height: f32) -> f32 {
        (self.strip + 2.0 * self.row).max(self.strip + bubble_height(bar_height))
    }
}

/// The heights of the popups of the A-B group in logical pixels (the same sizes the canvas lays them out with; a popup opens
/// above the bar and is no part of its rectangle).
pub fn ab_popup_height(popup: AbPopupSize, bar_height: f32) -> f32 {
    let pt = bar_height / 30.0;
    let p = |points: f32| (points * pt).round();
    match popup {
        AbPopupSize::Save { hint } => 2.0 * p(6.0) + p(26.0) + if hint { p(4.0) + p(18.0) } else { 0.0 } + p(7.0),
        AbPopupSize::Load { rows } => {
            2.0 * p(6.0) + p(26.0) + p(4.0) + rows.max(1).az::<f32>() * p(26.0) + p(4.0) + p(24.0) + p(7.0)
        }
    }
}

/// Which popup of the A-B group is open, as far as its height is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbPopupSize {
    Save {
        hint: bool,
    },
    /// How many rows the menu is high.
    Load {
        rows: usize,
    },
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
    /// The rectangle grows upward so that `room` (physical pixels) is free above the bar (the A-B group).
    pub fn with_room_above(self, room: i32) -> Self {
        let have = self.bar.y - self.window.y;
        if room <= have {
            return self;
        }
        let extra = room - have;
        Self {
            window: Rect::new(
                self.window.x,
                self.window.y - extra,
                self.window.w,
                self.window.h + extra,
            ),
            bar: self.bar,
        }
    }

    /// The same placement moved by (`dx`, `dy`): from coordinates of the host window to coordinates of the host's canvas.
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
    fn the_a_b_group_makes_room_above_the_bar() {
        let room = AbRoom::for_bar(30.0);
        assert_eq!((room.strip, room.row, room.flank), (12.0, 16.0, 18.0));
        assert!((room.above_bar(30.0) - 44.0).abs() < f32::EPSILON);
        let bar = place(Rect::new(100, 50, 800, 450), BarStyle::Bar, 30.0, 1.0).unwrap();
        let grown = bar.with_room_above(44 + 120);
        assert_eq!(grown.bar, bar.bar);
        assert_eq!(grown.window, Rect::new(100, 470 - 164, 800, 164 + 30));
        assert_eq!(bar.with_room_above(10), bar, "never smaller");
        let menu = ab_popup_height(AbPopupSize::Load { rows: 10 }, 30.0);
        assert!((menu - (12.0 + 26.0 + 4.0 + 260.0 + 4.0 + 24.0 + 7.0)).abs() < f32::EPSILON);
        assert!(
            ab_popup_height(AbPopupSize::Save { hint: true }, 30.0)
                > ab_popup_height(AbPopupSize::Save { hint: false }, 30.0)
        );
    }

    #[test]
    fn a_small_video_gets_no_bar() {
        assert!(place(Rect::new(0, 0, 150, 400), BarStyle::Bar, 30.0, 1.0).is_none());
        assert!(place(Rect::new(0, 0, 400, 50), BarStyle::Bar, 30.0, 1.0).is_none());
        assert!(place(Rect::new(0, 0, 400, 80), BarStyle::Bar, 30.0, 1.0).is_some());
    }
}
