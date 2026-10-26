//! Where an action bar goes and when it shows (pure).
//!
//! The media bar sits in the top right corner of its tile's room, the primary bar in the top left corner of the Multiview, both
//! a small margin inside. The canvas draws them (`ui/multiview_action_bar.slint`); the size of a bar follows from its buttons
//! alone, so it is known here without asking Slint. All rectangles are physical pixels of the host window.

use super::layout::Rect;
use rustview_core::settings::schema::{
    MULTIVIEW_BAR_ARROW_LOGICAL_PX, MULTIVIEW_BAR_BUTTON_LOGICAL_PX, MULTIVIEW_BAR_INSET_LOGICAL_PX,
    MULTIVIEW_BAR_MARGIN_LOGICAL_PX, MULTIVIEW_BAR_SPACING_LOGICAL_PX,
};
use rustview_core::util::cast::{Az, FloatToInt};

/// The corner of its area a bar sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarCorner {
    TopRight,
    TopLeft,
}

/// The size of a bar with `buttons` buttons, one of them a split button with an arrow of its own when `split` (logical pixels).
pub fn bar_size_logical(buttons: usize, split: bool) -> (f32, f32) {
    let count = buttons.az::<f32>();
    let width = count * MULTIVIEW_BAR_BUTTON_LOGICAL_PX
        + if split { MULTIVIEW_BAR_ARROW_LOGICAL_PX } else { 0.0 }
        + (count - 1.0).max(0.0) * MULTIVIEW_BAR_SPACING_LOGICAL_PX
        + 2.0 * MULTIVIEW_BAR_INSET_LOGICAL_PX;
    (
        width,
        MULTIVIEW_BAR_BUTTON_LOGICAL_PX + 2.0 * MULTIVIEW_BAR_INSET_LOGICAL_PX,
    )
}

/// Places a bar of `size_logical` in `corner` of `area` (the room of a tile for the media bar, the whole Multiview for the primary
/// bar). `None` if the area is too small to carry the bar with its margin: then there is no bar.
pub fn place_bar(corner: BarCorner, area: Rect, size_logical: (f32, f32), scale: f32) -> Option<Rect> {
    let scale = scale.max(0.5);
    let px = |logical: f32| (logical * scale).round().to_int::<i32>();
    let margin = px(MULTIVIEW_BAR_MARGIN_LOGICAL_PX);
    let (bar_w, bar_h) = (px(size_logical.0), px(size_logical.1));
    if bar_w <= 0 || bar_h <= 0 || area.w < bar_w + 2 * margin || area.h < bar_h + 2 * margin {
        return None;
    }
    let x = match corner {
        BarCorner::TopRight => area.x + area.w - margin - bar_w,
        BarCorner::TopLeft => area.x + margin,
    };
    Some(Rect::new(x, area.y + margin, bar_w, bar_h))
}

/// The pointer is on the bar or within `reveal_px` (logical pixels) of its edge.
pub fn in_reveal_zone(bar: Rect, pointer: (i32, i32), scale: f32, reveal_px: u32) -> bool {
    let reveal = (reveal_px.az::<f32>() * scale.max(0.5)).round().to_int::<i32>();
    Rect::new(bar.x - reveal, bar.y - reveal, bar.w + 2 * reveal, bar.h + 2 * reveal).contains(pointer.0, pointer.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_follows_from_the_buttons() {
        assert_eq!(bar_size_logical(1, false), (34.0, 34.0));
        // Three buttons, two gaps, the arrow of the split button.
        assert_eq!(bar_size_logical(3, true), (3.0 * 28.0 + 16.0 + 2.0 * 2.0 + 6.0, 34.0));
        assert_eq!(bar_size_logical(0, false), (6.0, 34.0));
    }

    #[test]
    fn a_bar_sits_in_its_corner_with_a_margin() {
        let area = Rect::new(100, 50, 400, 300);
        let right = place_bar(BarCorner::TopRight, area, (100.0, 34.0), 1.0);
        assert_eq!(right, Some(Rect::new(100 + 400 - 8 - 100, 58, 100, 34)));
        let left = place_bar(BarCorner::TopLeft, area, (100.0, 34.0), 2.0);
        assert_eq!(left, Some(Rect::new(116, 66, 200, 68)));
    }

    #[test]
    fn an_area_too_small_carries_no_bar() {
        assert_eq!(
            place_bar(BarCorner::TopRight, Rect::new(0, 0, 110, 300), (100.0, 34.0), 1.0),
            None
        );
        assert_eq!(
            place_bar(BarCorner::TopLeft, Rect::new(0, 0, 400, 40), (100.0, 34.0), 1.0),
            None
        );
    }

    #[test]
    fn the_reveal_zone_reaches_around_the_bar() {
        let bar = Rect::new(100, 100, 50, 30);
        assert!(in_reveal_zone(bar, (120, 110), 1.0, 0));
        assert!(in_reveal_zone(bar, (95, 110), 1.0, 10));
        assert!(!in_reveal_zone(bar, (85, 110), 1.0, 10));
        assert!(in_reveal_zone(bar, (85, 110), 2.0, 10), "the distance is logical");
    }
}
