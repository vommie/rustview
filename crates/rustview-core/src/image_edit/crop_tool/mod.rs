//! The interaction of the crop tool (pure): the rectangle, its eight handles, moving it, drawing a new one, Shift to keep the
//! ratio, and the ratio button. Positions come in as image pixels (the host converts the pointer through the zoom frame, so the
//! tool works the same at every zoom); the rectangle is whole pixels of the picture that is cropped.
//!
//! The ratio button cycles the presets; choosing one keeps the area of the last rectangle the user made (drawn, resized, at first
//! the whole picture) and where the user put it (`placement`: an edge it touches stays touched, otherwise its center stays), so
//! cycling never shrinks the selection step by step and never makes it jump. A resize without Shift is free and the button then
//! reads "Free"; with Shift the proportions stay.

use super::geometry::{PixelRect, align_origin, align_position, snap_coordinate};
use super::ratio::{AspectRatio, FREE_LABEL, RatioChoice, RatioSet};
use crate::util::cast::{Az, FloatToInt};
use placement::Placement;
use resize::{draw, resize};

/// A crop is never smaller than this many pixels per side (unless the picture itself is smaller).
pub const MIN_CROP_SIZE: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    pub const ALL: [Self; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];

    const fn moves_left(self) -> bool {
        matches!(self, Self::TopLeft | Self::Left | Self::BottomLeft)
    }

    const fn moves_right(self) -> bool {
        matches!(self, Self::TopRight | Self::Right | Self::BottomRight)
    }

    const fn moves_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::Top | Self::TopRight)
    }

    const fn moves_bottom(self) -> bool {
        matches!(self, Self::BottomLeft | Self::Bottom | Self::BottomRight)
    }

    pub const fn is_corner(self) -> bool {
        matches!(
            self,
            Self::TopLeft | Self::TopRight | Self::BottomRight | Self::BottomLeft
        )
    }

    /// Where the handle sits on `rect` (image pixels).
    pub fn position(self, rect: PixelRect) -> (f64, f64) {
        let (x0, y0) = (f64::from(rect.x), f64::from(rect.y));
        let (x1, y1) = (f64::from(rect.right()), f64::from(rect.bottom()));
        let x = if self.moves_left() {
            x0
        } else if self.moves_right() {
            x1
        } else {
            f64::midpoint(x0, x1)
        };
        let y = if self.moves_top() {
            y0
        } else if self.moves_bottom() {
            y1
        } else {
            f64::midpoint(y0, y1)
        };
        (x, y)
    }

    /// A stable number for the Slint boundary (the mouse cursor of each handle).
    pub const fn code(self) -> i32 {
        match self {
            Self::TopLeft => 0,
            Self::Top => 1,
            Self::TopRight => 2,
            Self::Right => 3,
            Self::BottomRight => 4,
            Self::Bottom => 5,
            Self::BottomLeft => 6,
            Self::Left => 7,
        }
    }
}

/// What lies under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Handle(Handle),
    /// Inside the rectangle: a drag moves it.
    Inside,
    /// Outside the rectangle: a drag draws a new one.
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragKind {
    Move,
    Resize(Handle),
    Draw { anchor: (u32, u32) },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    kind: DragKind,
    start: PixelRect,
    press: (f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CropTool {
    /// Size of the picture that is cropped (after earlier, applied crops).
    image: (u32, u32),
    /// Size of the original picture: the ratio "O".
    original: (u32, u32),
    rect: PixelRect,
    /// What the ratio button keeps (the area and the anchors of the rectangle the user made).
    placement: Placement,
    choice: RatioChoice,
    /// The rectangle still has the proportions of `choice` (else the button reads "Free").
    follows_choice: bool,
    /// The top left corner keeps to this grid (the blocks of a JPEG that is cropped without loss).
    grid: Option<(u32, u32)>,
    drag: Option<Drag>,
}

impl CropTool {
    /// The tool starts with the whole picture selected.
    pub fn new(image: (u32, u32), original: (u32, u32), grid: Option<(u32, u32)>) -> Self {
        let full = PixelRect::full(image);
        let follows_choice = AspectRatio::new(original.0, original.1).is_some_and(|r| follows(full, r));
        Self {
            image,
            original,
            rect: full,
            placement: Placement::of(full, image),
            choice: RatioChoice::Original,
            follows_choice,
            grid: grid.filter(|(w, h)| *w > 1 && *h > 1),
            drag: None,
        }
    }

    /// Starts with `rect` selected instead of the whole picture (the part a zoomed picture shows); the ratio button keeps its
    /// area and place too.
    pub fn select(&mut self, rect: PixelRect, ratios: &RatioSet) {
        let rect = self.aligned(rect.clamped_into(self.image));
        self.rect = rect;
        self.placement = Placement::of(rect, self.image);
        self.follows_choice = ratios
            .resolve(self.choice, self.original)
            .is_some_and(|r| follows(rect, r));
    }

    pub fn rect(&self) -> PixelRect {
        self.rect
    }

    pub fn image(&self) -> (u32, u32) {
        self.image
    }

    pub fn choice(&self) -> RatioChoice {
        self.choice
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The text of the ratio button.
    pub fn ratio_label<'a>(&self, ratios: &'a RatioSet) -> &'a str {
        if self.follows_choice {
            ratios.label(self.choice)
        } else {
            FREE_LABEL
        }
    }

    /// The text of the size label: the size the cropped picture will have.
    pub fn size_label(&self) -> String {
        format!("{} x {} px", self.rect.width, self.rect.height)
    }

    /// The selection is the whole picture (applying it would change nothing).
    pub fn is_whole_image(&self) -> bool {
        self.rect.is_full(self.image)
    }

    /// What is under the pointer at (`x`, `y`). `tolerance` is the reach of a handle in image pixels (the host derives it from
    /// the size of the handles on screen and the zoom). A rectangle too small for its handles is moved from inside. While the
    /// rectangle is the whole picture (a fresh tool) it cannot move anywhere, so a drag inside it draws a new one.
    pub fn hit_test(&self, x: f64, y: f64, tolerance: f64) -> Hit {
        let rect = self.rect;
        let inside = rect.contains(x, y);
        let inside_hit = if self.is_whole_image() {
            Hit::Outside
        } else {
            Hit::Inside
        };
        let small = f64::from(rect.width) < tolerance * 4.0 || f64::from(rect.height) < tolerance * 4.0;
        if small && inside {
            return inside_hit;
        }
        let near = |handle: Handle| {
            let (hx, hy) = handle.position(rect);
            (x - hx).abs() <= tolerance && (y - hy).abs() <= tolerance
        };
        if let Some(corner) = Handle::ALL.into_iter().filter(|h| h.is_corner()).find(|h| near(*h)) {
            return Hit::Handle(corner);
        }
        let (x0, y0, x1, y1) = (
            f64::from(rect.x),
            f64::from(rect.y),
            f64::from(rect.right()),
            f64::from(rect.bottom()),
        );
        let within_x = x >= x0 && x <= x1;
        let within_y = y >= y0 && y <= y1;
        if within_x && (y - y0).abs() <= tolerance {
            return Hit::Handle(Handle::Top);
        }
        if within_x && (y - y1).abs() <= tolerance {
            return Hit::Handle(Handle::Bottom);
        }
        if within_y && (x - x0).abs() <= tolerance {
            return Hit::Handle(Handle::Left);
        }
        if within_y && (x - x1).abs() <= tolerance {
            return Hit::Handle(Handle::Right);
        }
        if inside { inside_hit } else { Hit::Outside }
    }

    /// The button went down at (`x`, `y`) on `hit`; nothing changes until the pointer moves.
    pub fn press(&mut self, x: f64, y: f64, hit: Hit) {
        let kind = match hit {
            Hit::Handle(handle) => DragKind::Resize(handle),
            Hit::Inside => DragKind::Move,
            Hit::Outside => DragKind::Draw {
                anchor: (snap_coordinate(x, self.image.0), snap_coordinate(y, self.image.1)),
            },
        };
        self.drag = Some(Drag {
            kind,
            start: self.rect,
            press: (x, y),
        });
    }

    /// The pointer moved to (`x`, `y`) while the button is down; `shift` keeps the proportions. Returns whether the rectangle
    /// changed.
    pub fn drag_to(&mut self, x: f64, y: f64, shift: bool, ratios: &RatioSet) -> bool {
        let Some(drag) = self.drag else { return false };
        let chosen = ratios.resolve(self.choice, self.original);
        let before = self.rect;
        let rect = match drag.kind {
            DragKind::Move => {
                let moved = self.moved(drag, x, y);
                match self.grid {
                    Some(grid) => align_position(moved, grid, self.image),
                    None => moved,
                }
            }
            DragKind::Resize(handle) => {
                let ratio = shift.then(|| self.shift_ratio(drag.start, chosen)).flatten();
                self.aligned(resize(drag.start, handle, (x, y), self.image, ratio))
            }
            DragKind::Draw { anchor } => {
                let ratio = shift.then(|| chosen.map(AspectRatio::value)).flatten();
                self.aligned(draw(anchor, (x, y), self.image, ratio))
            }
        };
        self.rect = rect;
        match drag.kind {
            // The ratio button keeps the size the user made and takes the new place.
            DragKind::Move => self.placement = self.placement.moved(rect, self.image),
            DragKind::Resize(_) | DragKind::Draw { .. } => {
                self.placement = Placement::of(rect, self.image);
                self.follows_choice = chosen.is_some_and(|r| follows(rect, r));
            }
        }
        rect != before
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    /// The ratio button was clicked: the next landscape (left click) or portrait (right click) ratio, placed like the rectangle
    /// the user made.
    pub fn cycle_ratio(&mut self, ratios: &RatioSet, landscape: bool) {
        self.choice = ratios.next(self.choice, landscape);
        self.apply_choice(ratios);
    }

    /// Places the chosen ratio again (also after the presets changed).
    pub fn apply_choice(&mut self, ratios: &RatioSet) {
        if ratios.resolve(self.choice, self.original).is_none() {
            self.choice = RatioChoice::Original;
        }
        if let Some(ratio) = ratios.resolve(self.choice, self.original) {
            self.rect = self.aligned(self.placement.place(ratio, self.image));
            self.follows_choice = true;
        }
    }

    /// The proportions Shift keeps: the chosen ratio while the rectangle has it (exact, no rounding drift), else the
    /// rectangle's own.
    fn shift_ratio(&self, start: PixelRect, chosen: Option<AspectRatio>) -> Option<f64> {
        if self.follows_choice
            && let Some(ratio) = chosen
        {
            return Some(ratio.value());
        }
        (start.width > 0 && start.height > 0).then(|| f64::from(start.width) / f64::from(start.height))
    }

    fn moved(&self, drag: Drag, x: f64, y: f64) -> PixelRect {
        let (dx, dy) = (x - drag.press.0, y - drag.press.1);
        let start = drag.start;
        let max_x = self.image.0.saturating_sub(start.width);
        let max_y = self.image.1.saturating_sub(start.height);
        PixelRect::new(
            (f64::from(start.x) + dx)
                .round()
                .clamp(0.0, max_x.az::<f64>())
                .to_int::<u32>(),
            (f64::from(start.y) + dy)
                .round()
                .clamp(0.0, max_y.az::<f64>())
                .to_int::<u32>(),
            start.width,
            start.height,
        )
    }

    fn aligned(&self, rect: PixelRect) -> PixelRect {
        match self.grid {
            Some(grid) => align_origin(rect, grid),
            None => rect,
        }
    }
}

/// The rectangle has the proportions of `ratio` (up to the rounding to whole pixels).
fn follows(rect: PixelRect, ratio: AspectRatio) -> bool {
    let r = ratio.value();
    let (w, h) = (f64::from(rect.width), f64::from(rect.height));
    (w - h * r).abs() <= 1.0 || (h - w / r).abs() <= 1.0
}

mod placement;
mod resize;

#[cfg(test)]
mod tests;
