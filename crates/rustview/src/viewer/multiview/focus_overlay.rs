//! The corner marks of the selected tile on a video, drawn by mpv itself.
//!
//! Slint draws the marks of an image tile, but a video is a native window on top of the Slint scene. mpv draws the four
//! L-shaped marks on its OSD layer instead (`overlay-add`, like the feedback icon of `feedback_overlay`), in the corners of its
//! window, which is exactly the rectangle of the medium. Each mark is a small bitmap of its own, so nothing the size of the
//! window is ever copied.

use crate::viewer::MpvPlayer;
use crate::viewer::mpv_engine::overlay::FOCUS_OVERLAY_IDS;

/// `Theme.primary`, the color of the marks the canvas draws on images (and of the Browse selection).
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

/// The marks of one window: its size and the length and thickness of the marks (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CornerMarks {
    pub window: (u32, u32),
    pub length: u32,
    pub thickness: u32,
}

impl CornerMarks {
    /// Marks for a window of `window` pixels. A mark never takes more than half of the shorter side; `None` when the window
    /// is too small to carry any.
    pub fn new(window: (u32, u32), length: u32, thickness: u32) -> Option<Self> {
        let length = length.min(window.0 / 2).min(window.1 / 2);
        let thickness = thickness.min(length);
        (thickness > 0).then_some(Self {
            window,
            length,
            thickness,
        })
    }

    /// The top left corner of the `length` x `length` bitmap of `corner` in the window.
    pub fn origin(&self, corner: Corner) -> (i64, i64) {
        let far = |outer: u32| i64::from(outer) - i64::from(self.length);
        (
            if corner.is_left() { 0 } else { far(self.window.0) },
            if corner.is_top() { 0 } else { far(self.window.1) },
        )
    }

    /// Premultiplied BGRA pixels of the mark of `corner`: an L of `thickness` along the two outer edges, empty elsewhere.
    pub fn bitmap(&self, corner: Corner) -> Vec<u8> {
        let (edge, thick) = (self.length, self.thickness);
        let [red, green, blue] = MARK_COLOR;
        let mut pixels = vec![0u8; (u64::from(edge) * u64::from(edge) * 4).try_into().unwrap_or(0)];
        for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            let (col, row) = (index % edge, index / edge);
            let on_row = if corner.is_top() {
                row < thick
            } else {
                row >= edge - thick
            };
            let on_column = if corner.is_left() {
                col < thick
            } else {
                col >= edge - thick
            };
            if on_row || on_column {
                *pixel = [blue, green, red, 255];
            }
        }
        pixels
    }
}

/// Shows the four marks; returns whether mpv took them (it does not while no picture is set up, the next sync tries again).
pub fn show(player: &MpvPlayer, marks: &CornerMarks) -> bool {
    Corner::ALL.into_iter().zip(FOCUS_OVERLAY_IDS).all(|(corner, id)| {
        let (x, y) = marks.origin(corner);
        player.show_overlay(id, &marks.bitmap(corner), marks.length, marks.length, x, y)
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

    fn is_set(pixels: &[u8], length: u32, x: u32, y: u32) -> bool {
        let at = usize::try_from((y * length + x) * 4).unwrap();
        pixels[at + 3] == 255
    }

    #[test]
    fn the_marks_sit_in_the_four_corners_of_the_window() {
        let marks = CornerMarks::new((800, 450), 24, 2).unwrap();
        assert_eq!(marks.origin(Corner::TopLeft), (0, 0));
        assert_eq!(marks.origin(Corner::TopRight), (776, 0));
        assert_eq!(marks.origin(Corner::BottomLeft), (0, 426));
        assert_eq!(marks.origin(Corner::BottomRight), (776, 426));
    }

    #[test]
    fn a_mark_is_an_l_along_the_outer_edges() {
        let marks = CornerMarks::new((800, 450), 8, 2).unwrap();
        let top_left = marks.bitmap(Corner::TopLeft);
        assert_eq!(top_left.len(), 8 * 8 * 4);
        assert!(is_set(&top_left, 8, 0, 0) && is_set(&top_left, 8, 1, 1));
        assert!(is_set(&top_left, 8, 7, 1), "the top arm reaches the end");
        assert!(is_set(&top_left, 8, 1, 7), "the left arm reaches the end");
        assert!(!is_set(&top_left, 8, 4, 4), "the inside stays empty");
        let bottom_right = marks.bitmap(Corner::BottomRight);
        assert!(is_set(&bottom_right, 8, 7, 7) && is_set(&bottom_right, 8, 0, 6) && is_set(&bottom_right, 8, 6, 0));
        assert!(!is_set(&bottom_right, 8, 0, 0));
    }

    #[test]
    fn the_color_is_premultiplied_bgra_of_the_primary_color() {
        let marks = CornerMarks::new((100, 100), 4, 1).unwrap();
        assert_eq!(&marks.bitmap(Corner::TopLeft)[..4], &[0xfa, 0xb4, 0x89, 0xff]);
    }

    #[test]
    fn a_small_window_gets_shorter_marks_and_a_tiny_one_none() {
        let marks = CornerMarks::new((30, 20), 24, 2).unwrap();
        assert_eq!((marks.length, marks.thickness), (10, 2));
        assert_eq!(CornerMarks::new((1, 1), 24, 2), None);
    }
}
