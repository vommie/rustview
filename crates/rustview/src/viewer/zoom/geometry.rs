//! The geometry of zooming one image inside the room of a tile (pure, physical pixels).
//!
//! The room (`Area`) is the tile plus the free space around it that the layout leaves unused; the image fitted into the tile
//! (its `home`, "100%") may grow into all of it. All coordinates here are relative to the room.
//!
//! A view is `zoom` (1.0 = the image fitted into its home, "100%") and the point of the image in the middle of the room
//! (`cx`, `cy`, 0..1 of the image). The centre is the representation that survives everything: a resized tile, a changed
//! zoom, a different tile of another size. Everything else (the scale in screen pixels per source pixel, the rectangle the
//! image takes in the tile, the part of the source that is visible) is derived from it.
//!
//! Rules every operation keeps (these are what the tested viewers agree on):
//! - the visible window never leaves the image on an axis where the image is larger than the room (no empty margins when
//!   zoomed in);
//! - on an axis where the image is smaller than the room it stays where the fit is (its home), pushed inwards where it would
//!   leave the room;
//! - zooming around a point keeps the source point under that point where the clamp allows it; at an edge the clamp wins.

/// Tolerance for comparing pixel sizes.
const EPS: f64 = 1e-6;

/// Rounds to a whole pixel, half up, with a bias far above float noise (1e-12) and far below anything visible: a position that is
/// exactly (or by noise nearly) between two pixels always lands on the same one instead of flipping from one frame to the next.
fn snap(v: f64) -> f64 {
    (v + 0.5 + 1e-6).floor()
}

/// Zoom limits as factors of the fitted size (1.0 = 100%).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub min: f64,
    pub max: f64,
}

impl Limits {
    pub const DEFAULT: Self = Self { min: 0.1, max: 10.0 };

    /// Limits with `min <= 1 < max` guaranteed. A `min` of 1 means the picture cannot be made smaller than the fit; otherwise the
    /// slider has the reset in its middle.
    pub fn new(min: f64, max: f64) -> Self {
        let min = if min.is_finite() {
            min.clamp(0.01, 1.0)
        } else {
            Self::DEFAULT.min
        };
        let min = if min > 0.99 { 1.0 } else { min };
        let max = if max.is_finite() {
            max.clamp(1.01, 64.0)
        } else {
            Self::DEFAULT.max
        };
        Self { min, max }
    }

    /// Whether the picture can be made smaller than the fit at all.
    pub fn can_zoom_out(&self) -> bool {
        self.min < 1.0
    }

    pub fn clamp(&self, zoom: f64) -> f64 {
        if zoom.is_finite() {
            zoom.clamp(self.min, self.max)
        } else {
            1.0
        }
    }
}

/// What is shown of the image: the zoom and the centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomView {
    pub zoom: f64,
    pub cx: f64,
    pub cy: f64,
}

impl ZoomView {
    pub const FIT: Self = Self {
        zoom: 1.0,
        cx: 0.5,
        cy: 0.5,
    };

    /// The view of 100%: the plain, fitted picture (the centre does not matter at 100% of the fitted size, it is always the
    /// middle, so a zoom of 1.0 is the fit).
    pub fn is_fit(&self) -> bool {
        (self.zoom - 1.0).abs() < 1e-4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectF {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x <= self.right() && y <= self.bottom()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The part of the tile the image covers and the part of the source that goes there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Visible {
    /// Whole pixels in the tile, never empty.
    pub dest: IRect,
    /// The source rectangle (pixels of the original image) that is scaled to `dest`; the scale is the same on both axes up to
    /// the rounding of `dest`.
    pub src: RectF,
}

/// Where the zoomed image is (see `Frame::placement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Where pixel (0, 0) of the virtual image is in the room.
    pub x: i32,
    pub y: i32,
    /// Size of the virtual image.
    pub w: i32,
    pub h: i32,
    /// The part of the virtual image that is inside the room, in its pixels.
    pub window: IRect,
}

impl Placement {
    /// Where `window` is in the room.
    pub fn dest(&self) -> IRect {
        IRect {
            x: self.x + self.window.x,
            y: self.y + self.window.y,
            w: self.window.w,
            h: self.window.h,
        }
    }
}

/// The room an image may use: its size, and where the tile (the place of the fitted picture) is inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Area {
    pub size: (i32, i32),
    pub home: IRect,
}

impl Area {
    /// A room that is exactly the tile.
    pub fn plain(tile: (i32, i32)) -> Self {
        Self {
            size: tile,
            home: IRect {
                x: 0,
                y: 0,
                w: tile.0,
                h: tile.1,
            },
        }
    }

    /// The tile `home` grown by the free space around it, `home` being given relative to the room's top left corner.
    pub fn new(size: (i32, i32), home: IRect) -> Self {
        Self { size, home }
    }

    pub fn is_valid(&self) -> bool {
        self.size.0 > 0 && self.size.1 > 0 && self.home.w > 0 && self.home.h > 0
    }
}

/// The image and the room it is shown in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub image_w: f64,
    pub image_h: f64,
    /// The room.
    pub tile_w: f64,
    pub tile_h: f64,
    home: RectF,
}

impl Frame {
    /// `None` while the image or the tile has no size (nothing can be zoomed then). The room is the tile.
    pub fn new(image: (u32, u32), tile: (i32, i32)) -> Option<Self> {
        Self::in_area(image, Area::plain(tile))
    }

    /// The image in a room of its own (the tile and the free space around it).
    pub fn in_area(image: (u32, u32), area: Area) -> Option<Self> {
        (image.0 > 0 && image.1 > 0 && area.is_valid()).then(|| Self {
            image_w: f64::from(image.0),
            image_h: f64::from(image.1),
            tile_w: f64::from(area.size.0),
            tile_h: f64::from(area.size.1),
            home: RectF::new(
                f64::from(area.home.x),
                f64::from(area.home.y),
                f64::from(area.home.w),
                f64::from(area.home.h),
            ),
        })
    }

    /// Screen pixels per source pixel at 100% (the image fitted into its home).
    pub fn fit_scale(&self) -> f64 {
        (self.home.w / self.image_w).min(self.home.h / self.image_h)
    }

    pub fn scale(&self, zoom: f64) -> f64 {
        self.fit_scale() * zoom
    }

    /// Size of the whole image on screen.
    fn extent(&self, zoom: f64) -> (f64, f64) {
        let s = self.scale(zoom);
        (self.image_w * s, self.image_h * s)
    }

    /// The size of the whole zoomed image on screen.
    pub fn extent_px(&self, view: ZoomView) -> (f64, f64) {
        self.extent(view.zoom)
    }

    /// Whether the image is larger than the room on any axis, i.e. dragging can move something.
    pub fn is_pannable(&self, view: ZoomView) -> bool {
        let (ex, ey) = self.extent(view.zoom);
        ex > self.tile_w + EPS || ey > self.tile_h + EPS
    }

    fn clamp_axis(c: f64, extent: f64, tile: f64) -> f64 {
        if extent <= tile + EPS {
            0.5
        } else {
            let half = tile / (2.0 * extent);
            c.clamp(half, 1.0 - half)
        }
    }

    /// Puts the centre where the rules above allow it.
    pub fn clamp_center(&self, view: ZoomView) -> ZoomView {
        let (ex, ey) = self.extent(view.zoom);
        ZoomView {
            zoom: view.zoom,
            cx: Self::clamp_axis(view.cx, ex, self.tile_w),
            cy: Self::clamp_axis(view.cy, ey, self.tile_h),
        }
    }

    /// Keeps zoom and centre valid: the zoom inside the limits, the centre inside the image.
    pub fn normalize(&self, view: ZoomView, limits: Limits) -> ZoomView {
        let view = ZoomView {
            zoom: limits.clamp(view.zoom),
            cx: if view.cx.is_finite() { view.cx } else { 0.5 },
            cy: if view.cy.is_finite() { view.cy } else { 0.5 },
        };
        self.clamp_center(view)
    }

    /// Zooms to `zoom`. With an `anchor` (a point of the room in pixels, for example the pointer) the point of the image under
    /// it stays under it; an anchor that is not over the image zooms around the middle of the view instead.
    pub fn zoom_to(&self, view: ZoomView, zoom: f64, anchor: Option<(f64, f64)>, limits: Limits) -> ZoomView {
        let zoom = limits.clamp(zoom);
        let (new_x, new_y) = self.extent(zoom);
        let mut cx = view.cx;
        let mut cy = view.cy;
        if let Some((ax, ay)) = anchor {
            let old = self.image_rect(view);
            // The point of the image under the anchor, 0..1.
            let (px, py) = ((ax - old.x) / old.w, (ay - old.y) / old.h);
            // A hair outside (rounding) still is over the image.
            const SLACK: f64 = 1e-9;
            if (-SLACK..=1.0 + SLACK).contains(&px) && (-SLACK..=1.0 + SLACK).contains(&py) {
                let (px, py) = (px.clamp(0.0, 1.0), py.clamp(0.0, 1.0));
                // Where the image has to start for that point to stay under the anchor, as a centre of the view.
                cx = (self.tile_w / 2.0 - (ax - px * new_x)) / new_x;
                cy = (self.tile_h / 2.0 - (ay - py * new_y)) / new_y;
            }
        }
        self.clamp_center(ZoomView { zoom, cx, cy })
    }

    /// Moves the image by a drag of `dx`, `dy` screen pixels (the image follows the pointer).
    ///
    /// A drag is whole device pixels (the pointer moves in them), so the picture moves by exactly that many on screen, every time:
    /// the position it starts from is snapped to a whole pixel once and then only whole pixels are added. Adding the distance to
    /// the stored centre (a share of the image) and rounding the result for the screen would make the picture hop by 0, 1, 1, 0,
    /// 1 pixels for the same pointer speed, which reads as jitter.
    pub fn pan_pixels(&self, view: ZoomView, dx: f64, dy: f64) -> ZoomView {
        let (ex, ey) = self.extent(view.zoom);
        let rect = self.image_rect(view);
        let (dx, dy) = (dx.round(), dy.round());
        let cx = if ex > self.tile_w + EPS {
            (self.tile_w / 2.0 - (snap(rect.x) + dx)) / ex
        } else {
            view.cx
        };
        let cy = if ey > self.tile_h + EPS {
            (self.tile_h / 2.0 - (snap(rect.y) + dy)) / ey
        } else {
            view.cy
        };
        self.clamp_center(ZoomView {
            zoom: view.zoom,
            cx,
            cy,
        })
    }

    /// Where the image starts on an axis: pushed from its home into the room while it is smaller than the room, else placed by
    /// the centre of the view.
    fn axis_start(c: f64, extent: f64, room: f64, home_start: f64, home_len: f64) -> f64 {
        if extent <= room + EPS {
            (home_start + home_len / 2.0 - extent / 2.0).clamp(0.0, (room - extent).max(0.0))
        } else {
            room / 2.0 - c * extent
        }
    }

    /// The image on screen, relative to the room (it may reach beyond it).
    pub fn image_rect(&self, view: ZoomView) -> RectF {
        let (ex, ey) = self.extent(view.zoom);
        RectF::new(
            Self::axis_start(view.cx, ex, self.tile_w, self.home.x, self.home.w),
            Self::axis_start(view.cy, ey, self.tile_h, self.home.y, self.home.h),
            ex,
            ey,
        )
    }

    /// The part of the image the tile shows, as shares of the image (0..1): the rectangle of the minimap.
    pub fn viewport_norm(&self, view: ZoomView) -> RectF {
        let rect = self.image_rect(view);
        let x0 = ((0.0 - rect.x) / rect.w).clamp(0.0, 1.0);
        let x1 = ((self.tile_w - rect.x) / rect.w).clamp(0.0, 1.0);
        let y0 = ((0.0 - rect.y) / rect.h).clamp(0.0, 1.0);
        let y1 = ((self.tile_h - rect.y) / rect.h).clamp(0.0, 1.0);
        RectF::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }

    /// The pixels to produce: the visible part of the image in whole tile pixels, and the source rectangle that belongs to
    /// exactly those pixels (so rounding the destination never shifts or stretches the picture).
    pub fn visible(&self, view: ZoomView) -> Option<Visible> {
        let rect = self.image_rect(view);
        let s = self.scale(view.zoom);
        let x0 = snap(rect.x.max(0.0));
        let y0 = snap(rect.y.max(0.0));
        let x1 = snap(rect.right().min(self.tile_w));
        let y1 = snap(rect.bottom().min(self.tile_h));
        if x1 - x0 < 1.0 || y1 - y0 < 1.0 || s <= 0.0 {
            return None;
        }
        let sx0 = ((x0 - rect.x) / s).clamp(0.0, self.image_w);
        let sy0 = ((y0 - rect.y) / s).clamp(0.0, self.image_h);
        let sx1 = ((x1 - rect.x) / s).clamp(0.0, self.image_w);
        let sy1 = ((y1 - rect.y) / s).clamp(0.0, self.image_h);
        if sx1 - sx0 <= 0.0 || sy1 - sy0 <= 0.0 {
            return None;
        }
        Some(Visible {
            dest: IRect {
                x: x0 as i32,
                y: y0 as i32,
                w: (x1 - x0) as i32,
                h: (y1 - y0) as i32,
            },
            src: RectF::new(sx0, sy0, sx1 - sx0, sy1 - sy0),
        })
    }

    /// The zoomed image as a grid of whole pixels ("virtual image", `w` x `h`) that sits at (`x`, `y`) in the room, and which part of
    /// that grid is in the room (`window`). The grid depends on the zoom alone; panning only moves it by whole pixels, so a picture
    /// rendered for it stays sharp and exact while the view is dragged around.
    pub fn placement(&self, view: ZoomView) -> Option<Placement> {
        let rect = self.image_rect(view);
        let (x, y) = (snap(rect.x) as i32, snap(rect.y) as i32);
        let (w, h) = ((snap(rect.w) as i32).max(1), (snap(rect.h) as i32).max(1));
        let (room_w, room_h) = (self.tile_w.round() as i32, self.tile_h.round() as i32);
        let (x0, y0) = ((-x).max(0), (-y).max(0));
        let (x1, y1) = (w.min(room_w - x), h.min(room_h - y));
        (x1 > x0 && y1 > y0).then(|| Placement {
            x,
            y,
            w,
            h,
            window: IRect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            },
        })
    }

    /// Source pixels per pixel of the virtual image of `zoom` (the inverse of the scale).
    pub fn source_per_pixel(&self, zoom: f64) -> f64 {
        1.0 / self.scale(zoom)
    }

    /// A drag of the minimap's rectangle: the centre the view gets when the rectangle's middle is at (`nx`, `ny`), shares of
    /// the image. On an axis where the whole image is visible nothing moves.
    pub fn center_for_viewport(&self, view: ZoomView, nx: f64, ny: f64) -> ZoomView {
        self.clamp_center(ZoomView {
            zoom: view.zoom,
            cx: nx,
            cy: ny,
        })
    }
}
