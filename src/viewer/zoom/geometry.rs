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
        let min = if min.is_finite() { min.clamp(0.01, 1.0) } else { Self::DEFAULT.min };
        let min = if min > 0.99 { 1.0 } else { min };
        let max = if max.is_finite() { max.clamp(1.01, 64.0) } else { Self::DEFAULT.max };
        Self { min, max }
    }

    /// Whether the picture can be made smaller than the fit at all.
    pub fn can_zoom_out(&self) -> bool {
        self.min < 1.0
    }

    pub fn clamp(&self, zoom: f64) -> f64 {
        if zoom.is_finite() { zoom.clamp(self.min, self.max) } else { 1.0 }
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
    pub const FIT: Self = Self { zoom: 1.0, cx: 0.5, cy: 0.5 };

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

/// The room an image may use: its size, and where the tile (the place of the fitted picture) is inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Area {
    pub size: (i32, i32),
    pub home: IRect,
}

impl Area {
    /// A room that is exactly the tile.
    pub fn plain(tile: (i32, i32)) -> Self {
        Self { size: tile, home: IRect { x: 0, y: 0, w: tile.0, h: tile.1 } }
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
            home: RectF::new(f64::from(area.home.x), f64::from(area.home.y), f64::from(area.home.w), f64::from(area.home.h)),
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
    pub fn pan_pixels(&self, view: ZoomView, dx: f64, dy: f64) -> ZoomView {
        let (ex, ey) = self.extent(view.zoom);
        self.clamp_center(ZoomView { zoom: view.zoom, cx: view.cx - dx / ex, cy: view.cy - dy / ey })
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
        let x0 = rect.x.max(0.0).round();
        let y0 = rect.y.max(0.0).round();
        let x1 = rect.right().min(self.tile_w).round();
        let y1 = rect.bottom().min(self.tile_h).round();
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
            dest: IRect { x: x0 as i32, y: y0 as i32, w: (x1 - x0) as i32, h: (y1 - y0) as i32 },
            src: RectF::new(sx0, sy0, sx1 - sx0, sy1 - sy0),
        })
    }

    /// A drag of the minimap's rectangle: the centre the view gets when the rectangle's middle is at (`nx`, `ny`), shares of
    /// the image. On an axis where the whole image is visible nothing moves.
    pub fn center_for_viewport(&self, view: ZoomView, nx: f64, ny: f64) -> ZoomView {
        self.clamp_center(ZoomView { zoom: view.zoom, cx: nx, cy: ny })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits::DEFAULT;

    fn frame() -> Frame {
        // A 4000x3000 photo in a 1000x750 tile (exact fit, scale 0.25).
        Frame::new((4000, 3000), (1000, 750)).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn portrait_in_a_wide_room() -> Frame {
        // A 900x1600 picture fitted into a 506x900 tile that sits in the middle of a 1600x900 room.
        Frame::in_area((900, 1600), Area::new((1600, 900), IRect { x: 547, y: 0, w: 506, h: 900 })).unwrap()
    }

    #[test]
    fn the_fit_stays_where_the_tile_is_and_growing_uses_the_free_room() {
        let f = portrait_in_a_wide_room();
        let fit = f.image_rect(ZoomView::FIT);
        assert!(close(fit.x, 547.0) && close(fit.w, 506.0) && (fit.h - 900.0).abs() < 1.0, "{fit:?}");
        assert!(!f.is_pannable(ZoomView::FIT));
        // 200%: 1012 px wide, still inside the room (so not pannable sideways) and centred on the tile.
        let v = f.zoom_to(ZoomView::FIT, 2.0, None, LIMITS);
        let r = f.image_rect(v);
        assert!(close(r.w, 1012.0) && close(r.x + r.w / 2.0, 800.0), "{r:?}");
        // 400%: wider than the room, so it fills the room's whole width.
        let v = f.zoom_to(ZoomView::FIT, 4.0, None, LIMITS);
        let vis = f.visible(v).unwrap();
        assert_eq!((vis.dest.x, vis.dest.w), (0, 1600));
        assert!(f.is_pannable(v));
        let vp = f.viewport_norm(v);
        assert!(vp.x >= 0.0 && vp.right() <= 1.0 + 1e-9 && close(vp.w, 1600.0 / (506.0 * 4.0)), "{vp:?}");
    }

    #[test]
    fn a_picture_near_the_edge_of_the_room_is_pushed_inwards() {
        // The tile hugs the left edge of the room; growing around its own centre would leave the room.
        let f = Frame::in_area((1000, 1000), Area::new((1000, 500), IRect { x: 0, y: 0, w: 500, h: 500 })).unwrap();
        let v = f.zoom_to(ZoomView::FIT, 1.8, None, LIMITS);
        let r = f.image_rect(v);
        assert!(r.x >= 0.0 && r.right() <= 1000.0 + 1e-9, "{r:?}");
    }

    #[test]
    fn zooming_at_the_pointer_works_from_a_fit_that_is_smaller_than_the_room() {
        let f = portrait_in_a_wide_room();
        let anchor = (700.0, 300.0);
        let before = f.image_rect(ZoomView::FIT);
        let src_before = ((anchor.0 - before.x) / before.w, (anchor.1 - before.y) / before.h);
        let v = f.zoom_to(ZoomView::FIT, 5.0, Some(anchor), LIMITS);
        let after = f.image_rect(v);
        let src_after = ((anchor.0 - after.x) / after.w, (anchor.1 - after.y) / after.h);
        assert!(close(src_before.0, src_after.0) && close(src_before.1, src_after.1), "{src_before:?} {src_after:?}");
    }

    #[test]
    fn no_size_means_no_frame() {
        assert!(Frame::new((0, 10), (10, 10)).is_none());
        assert!(Frame::new((10, 10), (10, 0)).is_none());
        assert!(Frame::new((1, 1), (1, 1)).is_some());
    }

    #[test]
    fn fit_scale_fits_the_tighter_axis() {
        let f = Frame::new((2000, 1000), (800, 800)).unwrap();
        assert!(close(f.fit_scale(), 0.4));
        let f = Frame::new((1000, 2000), (800, 800)).unwrap();
        assert!(close(f.fit_scale(), 0.4));
    }

    #[test]
    fn the_fit_shows_the_whole_image_and_is_not_pannable() {
        let f = frame();
        let v = ZoomView::FIT;
        assert!(!f.is_pannable(v));
        let vp = f.viewport_norm(v);
        assert_eq!((vp.x, vp.y, vp.w, vp.h), (0.0, 0.0, 1.0, 1.0));
        let vis = f.visible(v).unwrap();
        assert_eq!(vis.dest, IRect { x: 0, y: 0, w: 1000, h: 750 });
        assert!(close(vis.src.w, 4000.0) && close(vis.src.h, 3000.0));
    }

    #[test]
    fn zooming_in_without_anchor_keeps_the_centre() {
        let f = frame();
        let v = f.zoom_to(ZoomView::FIT, 2.0, None, LIMITS);
        assert!(close(v.zoom, 2.0) && close(v.cx, 0.5) && close(v.cy, 0.5));
        let vp = f.viewport_norm(v);
        assert!(close(vp.w, 0.5) && close(vp.h, 0.5) && close(vp.x, 0.25));
    }

    #[test]
    fn the_point_under_the_anchor_stays_under_it() {
        let f = frame();
        let anchor = (750.0, 200.0);
        let before = f.image_rect(ZoomView::FIT);
        let src_before = ((anchor.0 - before.x) / before.w, (anchor.1 - before.y) / before.h);
        let v = f.zoom_to(ZoomView::FIT, 3.0, Some(anchor), LIMITS);
        let after = f.image_rect(v);
        let src_after = ((anchor.0 - after.x) / after.w, (anchor.1 - after.y) / after.h);
        assert!(close(src_before.0, src_after.0) && close(src_before.1, src_after.1), "{src_before:?} {src_after:?} {v:?}");
        // And again from the zoomed view, in a different spot.
        let anchor = (100.0, 600.0);
        let before = f.image_rect(v);
        let src_before = ((anchor.0 - before.x) / before.w, (anchor.1 - before.y) / before.h);
        let w = f.zoom_to(v, 5.0, Some(anchor), LIMITS);
        let after = f.image_rect(w);
        let src_after = ((anchor.0 - after.x) / after.w, (anchor.1 - after.y) / after.h);
        assert!(close(src_before.0, src_after.0) && close(src_before.1, src_after.1));
    }

    #[test]
    fn near_an_edge_the_clamp_wins_and_the_window_stays_inside_the_image() {
        let f = frame();
        // Zoom in at the very corner: the corner of the image ends in the corner of the tile.
        let v = f.zoom_to(ZoomView::FIT, 4.0, Some((0.0, 0.0)), LIMITS);
        let vp = f.viewport_norm(v);
        assert!(close(vp.x, 0.0) && close(vp.y, 0.0), "{vp:?}");
        let v = f.zoom_to(ZoomView::FIT, 4.0, Some((1000.0, 750.0)), LIMITS);
        let vp = f.viewport_norm(v);
        assert!(close(vp.right(), 1.0) && close(vp.bottom(), 1.0), "{vp:?}");
    }

    #[test]
    fn the_window_never_leaves_the_image_while_it_is_larger_than_the_tile() {
        let f = frame();
        for zoom in [1.01, 1.5, 2.0, 7.3, 10.0] {
            for (cx, cy) in [(-3.0, -3.0), (0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (9.0, 9.0), (0.01, 0.99)] {
                let v = f.clamp_center(ZoomView { zoom, cx, cy });
                let vp = f.viewport_norm(v);
                assert!(vp.x >= -1e-9 && vp.y >= -1e-9 && vp.right() <= 1.0 + 1e-9 && vp.bottom() <= 1.0 + 1e-9, "{zoom} {cx} {cy} {vp:?}");
                // The window has the size the zoom says: no clipped, shorter window.
                assert!(close(vp.w, 1.0 / zoom) && close(vp.h, 1.0 / zoom), "{zoom} {vp:?}");
                let vis = f.visible(v).unwrap();
                assert_eq!((vis.dest.x, vis.dest.y, vis.dest.w, vis.dest.h), (0, 0, 1000, 750));
            }
        }
    }

    #[test]
    fn zoomed_out_the_image_is_centred_and_smaller() {
        let f = frame();
        let v = f.zoom_to(ZoomView { zoom: 2.0, cx: 0.9, cy: 0.1 }, 0.5, Some((10.0, 10.0)), LIMITS);
        assert!(close(v.cx, 0.5) && close(v.cy, 0.5));
        let rect = f.image_rect(v);
        assert!(close(rect.w, 500.0) && close(rect.h, 375.0));
        assert!(close(rect.x, 250.0) && close(rect.y, 187.5));
        let vis = f.visible(v).unwrap();
        assert_eq!(vis.dest.w, 500);
        // The destination is rounded to whole pixels (y 187.5 -> 188), which trims a few source pixels at most.
        assert!(close(vis.src.w, 4000.0) && (vis.src.h - 3000.0).abs() < 8.0, "{:?}", vis.src);
        let vp = f.viewport_norm(v);
        assert_eq!((vp.x, vp.y, vp.w, vp.h), (0.0, 0.0, 1.0, 1.0));
        assert!(!f.is_pannable(v));
    }

    #[test]
    fn an_axis_with_bars_stays_centred_while_the_other_one_can_move() {
        // A very wide panorama in a squarish tile: at 100% it has bars above and below.
        let f = Frame::new((6000, 1000), (900, 600)).unwrap();
        let v = f.zoom_to(ZoomView::FIT, 3.0, None, LIMITS);
        // At 300% the height is 450 px < 600: still centred vertically, the width (2700 px) can move.
        let moved = f.pan_pixels(v, -400.0, -300.0);
        assert!(close(moved.cy, 0.5));
        assert!(moved.cx > 0.5);
        assert!(f.is_pannable(v));
        let rect = f.image_rect(moved);
        assert!(rect.y >= 0.0 && rect.bottom() <= 600.0);
    }

    #[test]
    fn panning_moves_the_image_with_the_pointer_and_stops_at_the_edges() {
        let f = frame();
        let v = f.zoom_to(ZoomView::FIT, 2.0, None, LIMITS);
        // Dragging the image 100 px to the right shows more of the left.
        let moved = f.pan_pixels(v, 100.0, 0.0);
        assert!(moved.cx < v.cx);
        let r0 = f.image_rect(v);
        let r1 = f.image_rect(moved);
        assert!(close(r1.x - r0.x, 100.0));
        // Far beyond the edge stops at the edge.
        let edge = f.pan_pixels(v, 99999.0, 99999.0);
        let r = f.image_rect(edge);
        assert!(close(r.x, 0.0) && close(r.y, 0.0));
        let edge = f.pan_pixels(v, -99999.0, -99999.0);
        let r = f.image_rect(edge);
        assert!(close(r.right(), 1000.0) && close(r.bottom(), 750.0));
    }

    #[test]
    fn limits_are_kept_and_nan_is_harmless() {
        let f = frame();
        assert!(close(f.zoom_to(ZoomView::FIT, 1000.0, None, LIMITS).zoom, 10.0));
        assert!(close(f.zoom_to(ZoomView::FIT, 0.0001, None, LIMITS).zoom, 0.1));
        assert!(close(f.zoom_to(ZoomView::FIT, f64::NAN, None, LIMITS).zoom, 1.0));
        let v = f.normalize(ZoomView { zoom: f64::NAN, cx: f64::NAN, cy: f64::INFINITY }, LIMITS);
        assert!(v.zoom.is_finite() && v.cx.is_finite() && v.cy.is_finite());
        let l = Limits::new(5.0, 0.5);
        assert!(l.min <= 1.0 && l.max > 1.0);
    }

    #[test]
    fn the_destination_and_source_rectangles_agree_whatever_the_rounding() {
        let f = Frame::new((3333, 2222), (1011, 677)).unwrap();
        for zoom in [0.37, 0.8, 1.0, 1.37, 2.9, 8.123] {
            for (cx, cy) in [(0.5, 0.5), (0.12, 0.8), (0.97, 0.03)] {
                let v = f.clamp_center(ZoomView { zoom, cx, cy });
                let vis = f.visible(v).unwrap();
                let s = f.scale(zoom);
                let rect = f.image_rect(v);
                // The source pixel at the left edge of the destination is where the image says it is.
                let expected_sx = (f64::from(vis.dest.x) - rect.x) / s;
                assert!((vis.src.x - expected_sx.max(0.0)).abs() < 1e-6, "{zoom}");
                // Stretching: destination width / source width is the scale (within one pixel of rounding).
                let implied = f64::from(vis.dest.w) / vis.src.w;
                assert!((implied - s).abs() * vis.src.w < 1.0, "{zoom} {implied} {s}");
                assert!(vis.dest.x >= 0 && vis.dest.y >= 0 && vis.dest.x + vis.dest.w <= 1011 && vis.dest.y + vis.dest.h <= 677);
            }
        }
    }

    #[test]
    fn a_tiny_tile_or_image_does_not_break_anything() {
        let f = Frame::new((1, 1), (1, 1)).unwrap();
        let v = f.zoom_to(ZoomView::FIT, 10.0, Some((0.0, 0.0)), LIMITS);
        assert!(f.visible(v).is_some());
        // A hairline of an image can be too thin to show a single pixel row; that is no picture, not a crash.
        let f = Frame::new((10_000, 10), (50, 50)).unwrap();
        let v = f.zoom_to(ZoomView::FIT, 10.0, Some((25.0, 25.0)), LIMITS);
        let _ = f.visible(v);
        let v = f.zoom_to(ZoomView::FIT, 10.0, None, Limits::new(0.1, 64.0));
        let _ = f.visible(v);
    }

    #[test]
    fn the_viewport_of_the_minimap_follows_the_centre() {
        let f = frame();
        let v = f.zoom_to(ZoomView::FIT, 4.0, None, LIMITS);
        let moved = f.center_for_viewport(v, 0.2, 0.9);
        let vp = f.viewport_norm(moved);
        // The window is 25% wide; centred on 0.2 it would reach to 0.075..0.325; vertically it is clamped to the image.
        assert!(close(vp.x, 0.075) && close(vp.w, 0.25));
        assert!(close(vp.bottom(), 1.0));
        // Beyond the image is stopped at the edge.
        let edge = f.center_for_viewport(v, -5.0, 7.0);
        let vp = f.viewport_norm(edge);
        assert!(close(vp.x, 0.0) && close(vp.bottom(), 1.0));
    }

    #[test]
    fn a_resized_tile_keeps_what_the_centre_says() {
        let big = frame();
        let v = big.zoom_to(ZoomView::FIT, 3.0, Some((300.0, 300.0)), LIMITS);
        let small = Frame::new((4000, 3000), (500, 375)).unwrap();
        let w = small.normalize(v, LIMITS);
        // The same part of the image is in the middle, and it is still a valid window.
        assert!(close(w.cx, v.cx) && close(w.cy, v.cy) && close(w.zoom, v.zoom));
        let vp = small.viewport_norm(w);
        assert!(close(vp.w, 1.0 / 3.0));
    }
}
