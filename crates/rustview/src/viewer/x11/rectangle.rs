//! Rectangles as the X protocol has them (the shapes of the floating bar and hint windows).

use x11rb::protocol::xproto::Rectangle;

/// A rectangle in window coordinates (`x`, `y` and size), as the X protocol has it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XRectangle {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl From<XRectangle> for Rectangle {
    fn from(rect: XRectangle) -> Self {
        Rectangle {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

/// The rectangles in the form the X protocol requests take.
pub(super) fn protocol_rectangles(rects: &[XRectangle]) -> Vec<Rectangle> {
    rects.iter().copied().map(Rectangle::from).collect()
}
