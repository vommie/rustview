//! The corner marks of the selected tile: where they go, and the part of them that mpv draws over a video.
//!
//! The marks frame the VIEW of the tile (its canvas, `host_room`: the column at the full height of its row), never the medium
//! (user decision 2026-10-25), so they stay put whatever the medium's size, zoom or kind. The canvas draws them for every tile;
//! where a video's native window covers a corner, mpv draws the part of that mark that lies inside its window on its OSD layer
//! (`overlay-add`, like the feedback icon of `feedback_overlay`). The window is the medium's place, or the whole room while the
//! video is zoomed, so the parts are cut from the marks in host coordinates and moved into the window, never derived from the
//! window's own size. Each part is a small bitmap of its own, so nothing the size of the window is ever copied.

use super::layout::Rect;
use crate::viewer::MpvPlayer;
use crate::viewer::mpv_engine::overlay::FOCUS_OVERLAY_IDS;
use rustview_core::util::cast::SaturatingAs;

/// `Theme.primary`, the color of the marks the canvas draws (and of the Browse selection).
pub const MARK_COLOR: [u8; 3] = [0x89, 0xb4, 0xfa];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Self; 4] = [Self::TopLeft, Self::TopRight, Self::BottomLeft, Self::BottomRight];

    const fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopRight)
    }

    const fn is_left(self) -> bool {
        matches!(self, Self::TopLeft | Self::BottomLeft)
    }
}

/// The marks of one view: its rectangle and the length and thickness of the marks (host pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CornerMarks {
    pub view: Rect,
    pub length: i32,
    pub thickness: i32,
}

impl CornerMarks {
    /// Marks for the view `view`. A mark never takes more than half of the shorter side (the canvas clamps its marks the same
    /// way); `None` when the view is too small to carry any.
    pub fn new(view: Rect, length: i32, thickness: i32) -> Option<Self> {
        let length = length.min(view.w / 2).min(view.h / 2);
        let thickness = thickness.min(length);
        (thickness > 0).then_some(Self {
            view,
            length,
            thickness,
        })
    }

    /// The `length` x `length` square that holds the mark of `corner`, in host coordinates.
    pub fn square(&self, corner: Corner) -> Rect {
        let Rect { x, y, w, h } = self.view;
        Rect::new(
            if corner.is_left() { x } else { x + w - self.length },
            if corner.is_top() { y } else { y + h - self.length },
            self.length,
            self.length,
        )
    }

    /// Whether the pixel at (`col`, `row`) of the square of `corner` belongs to the mark: an L of `thickness` along the two outer
    /// edges of the view.
    fn covers(&self, corner: Corner, col: i32, row: i32) -> bool {
        let far = self.length - self.thickness;
        let on_row = if corner.is_top() {
            row < self.thickness
        } else {
            row >= far
        };
        let on_column = if corner.is_left() {
            col < self.thickness
        } else {
            col >= far
        };
        on_row || on_column
    }

    /// The parts of the marks that lie inside a window at `window` (host coordinates), in the coordinates of the window.
    pub fn in_window(&self, window: Rect) -> WindowMarks {
        WindowMarks {
            marks: *self,
            window,
            parts: Corner::ALL.map(|corner| {
                let square = self.square(corner);
                intersection(square, window).map(|cut| MarkPart {
                    corner,
                    cut: Rect::new(cut.x - square.x, cut.y - square.y, cut.w, cut.h),
                    at: (cut.x - window.x, cut.y - window.y),
                })
            }),
        }
    }

    /// Premultiplied BGRA pixels of `part`: the piece `part.cut` of the square of its corner.
    pub fn pixels(&self, part: &MarkPart) -> Vec<u8> {
        let [red, green, blue] = MARK_COLOR;
        let Rect { x, y, w, h } = part.cut;
        let mut pixels = Vec::with_capacity((w.max(0) * h.max(0) * 4).saturating_as::<usize>());
        for row in y..y + h {
            for col in x..x + w {
                let pixel = if self.covers(part.corner, col, row) {
                    [blue, green, red, 255]
                } else {
                    [0; 4]
                };
                pixels.extend_from_slice(&pixel);
            }
        }
        pixels
    }
}

/// The overlap of two rectangles, `None` when they do not overlap.
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// The piece of the mark of `corner` that a video window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkPart {
    pub corner: Corner,
    /// The piece, relative to the square of the mark (`CornerMarks::square`).
    pub cut: Rect,
    /// Its top left corner in the window.
    pub at: (i32, i32),
}

/// What mpv draws over one video: the marks cut to its window, one part per corner (`None` = that corner lies outside).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowMarks {
    pub marks: CornerMarks,
    pub window: Rect,
    pub parts: [Option<MarkPart>; 4],
}

impl WindowMarks {
    /// Whether any part of the marks lies inside the window.
    pub fn any(&self) -> bool {
        self.parts.iter().any(Option::is_some)
    }
}

/// Shows the parts of the marks inside the window (a corner outside it has no overlay); returns whether mpv took them (it does
/// not while no picture is set up, the next sync tries again).
pub fn show(player: &MpvPlayer, marks: &WindowMarks) -> bool {
    marks.parts.iter().zip(FOCUS_OVERLAY_IDS).all(|(part, id)| {
        if let Some(part) = part {
            let (w, h) = (part.cut.w.saturating_as::<u32>(), part.cut.h.saturating_as::<u32>());
            let pixels = marks.marks.pixels(part);
            player.show_overlay(id, &pixels, w, h, i64::from(part.at.0), i64::from(part.at.1))
        } else {
            player.remove_overlay(id);
            true
        }
    })
}

pub fn hide(player: &MpvPlayer) {
    for id in FOCUS_OVERLAY_IDS {
        player.remove_overlay(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_set(pixels: &[u8], width: i32, x: i32, y: i32) -> bool {
        let at = usize::try_from((y * width + x) * 4).unwrap();
        pixels[at + 3] == 255
    }

    fn whole(marks: &CornerMarks, corner: Corner) -> Vec<u8> {
        let part = MarkPart {
            corner,
            cut: Rect::new(0, 0, marks.length, marks.length),
            at: (0, 0),
        };
        marks.pixels(&part)
    }

    #[test]
    fn the_marks_sit_in_the_four_corners_of_the_view() {
        let marks = CornerMarks::new(Rect::new(100, 50, 800, 450), 24, 2).unwrap();
        assert_eq!(marks.square(Corner::TopLeft), Rect::new(100, 50, 24, 24));
        assert_eq!(marks.square(Corner::TopRight), Rect::new(876, 50, 24, 24));
        assert_eq!(marks.square(Corner::BottomLeft), Rect::new(100, 476, 24, 24));
        assert_eq!(marks.square(Corner::BottomRight), Rect::new(876, 476, 24, 24));
    }

    #[test]
    fn a_mark_is_an_l_along_the_outer_edges() {
        let marks = CornerMarks::new(Rect::new(0, 0, 800, 450), 8, 2).unwrap();
        let top_left = whole(&marks, Corner::TopLeft);
        assert_eq!(top_left.len(), 8 * 8 * 4);
        assert!(is_set(&top_left, 8, 0, 0) && is_set(&top_left, 8, 1, 1));
        assert!(is_set(&top_left, 8, 7, 1), "the top arm reaches the end");
        assert!(is_set(&top_left, 8, 1, 7), "the left arm reaches the end");
        assert!(!is_set(&top_left, 8, 4, 4), "the inside stays empty");
        let bottom_right = whole(&marks, Corner::BottomRight);
        assert!(is_set(&bottom_right, 8, 7, 7) && is_set(&bottom_right, 8, 0, 6) && is_set(&bottom_right, 8, 6, 0));
        assert!(!is_set(&bottom_right, 8, 0, 0));
    }

    #[test]
    fn the_color_is_premultiplied_bgra_of_the_primary_color() {
        let marks = CornerMarks::new(Rect::new(0, 0, 100, 100), 4, 1).unwrap();
        assert_eq!(&whole(&marks, Corner::TopLeft)[..4], &[0xfa, 0xb4, 0x89, 0xff]);
    }

    #[test]
    fn a_small_view_gets_shorter_marks_and_a_tiny_one_none() {
        let marks = CornerMarks::new(Rect::new(0, 0, 30, 20), 24, 2).unwrap();
        assert_eq!((marks.length, marks.thickness), (10, 2));
        assert_eq!(CornerMarks::new(Rect::new(0, 0, 1, 1), 24, 2), None);
    }

    #[test]
    fn a_window_that_fills_the_view_gets_every_mark_at_its_own_corners() {
        // A video whose window is exactly its view (or a zoomed one that grew into the whole room).
        let view = Rect::new(960, 0, 960, 1080);
        let marks = CornerMarks::new(view, 24, 2).unwrap();
        let shown = marks.in_window(view);
        let at: Vec<_> = shown.parts.iter().map(|p| p.unwrap().at).collect();
        assert_eq!(at, vec![(0, 0), (936, 0), (0, 1056), (936, 1056)]);
        assert!(shown.parts.iter().all(|p| p.unwrap().cut == Rect::new(0, 0, 24, 24)));
    }

    #[test]
    fn a_letterboxed_video_window_inside_the_view_shows_no_mark() {
        // An unzoomed 16:9 video in a tall column: the window sits in the middle, the marks in the free space above and below,
        // where the canvas draws them.
        let view = Rect::new(0, 0, 640, 1080);
        let window = Rect::new(0, 360, 640, 360);
        let shown = CornerMarks::new(view, 24, 2).unwrap().in_window(window);
        assert!(!shown.any());
    }

    #[test]
    fn a_window_that_covers_part_of_a_mark_gets_that_part_in_its_own_coordinates() {
        let view = Rect::new(0, 0, 1000, 600);
        let marks = CornerMarks::new(view, 24, 2).unwrap();
        // The window starts 10 px right of the view's left edge and 5 px below its top, and ends at its right edge.
        let window = Rect::new(10, 5, 990, 300);
        let shown = marks.in_window(window);
        let top_left = shown.parts[0].unwrap();
        assert_eq!(top_left.cut, Rect::new(10, 5, 14, 19));
        assert_eq!(top_left.at, (0, 0));
        let top_right = shown.parts[1].unwrap();
        assert_eq!(top_right.cut, Rect::new(0, 5, 24, 19));
        assert_eq!(top_right.at, (966, 0));
        assert_eq!(shown.parts[2], None);
        assert_eq!(shown.parts[3], None);
        // The cut piece of the top right mark: its right arm (the last two columns) runs down the whole piece, its top arm
        // (rows 0..2 of the square) lies above the window and is gone.
        let pixels = marks.pixels(&top_right);
        assert_eq!(pixels.len(), 24 * 19 * 4);
        assert!(is_set(&pixels, 24, 23, 0) && is_set(&pixels, 24, 22, 18));
        assert!(!is_set(&pixels, 24, 0, 0), "the top arm is above the window");
    }

    #[test]
    fn a_zoomed_video_window_larger_than_the_medium_still_gets_the_view_corners() {
        // Zoomed: the window grew from the medium (in the middle of the column) into the whole room; the marks stay in the
        // corners of the view, which is the room, not in the corners of the medium.
        let view = Rect::new(0, 0, 640, 1080);
        let medium = Rect::new(0, 360, 640, 360);
        let marks = CornerMarks::new(view, 32, 2).unwrap();
        assert!(!marks.in_window(medium).any());
        let grown = marks.in_window(view);
        assert_eq!(grown.parts[3].unwrap().at, (608, 1048));
        assert_ne!(
            marks.in_window(medium),
            grown,
            "a new window size sends the marks again"
        );
    }

    #[test]
    fn rectangles_overlap_or_not() {
        assert_eq!(
            intersection(Rect::new(0, 0, 10, 10), Rect::new(5, 5, 10, 10)),
            Some(Rect::new(5, 5, 5, 5))
        );
        assert_eq!(intersection(Rect::new(0, 0, 10, 10), Rect::new(10, 0, 10, 10)), None);
    }
}
