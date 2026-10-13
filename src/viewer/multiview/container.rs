//! The virtual container of a Multiview and its projection into a host rectangle (pure).
//!
//! A Multiview is always laid out in the pixel space of a virtual display, the container: the size and scale factor of the
//! display the fullscreen Multiview was last shown on. A host (fullscreen window, tab view, tab-grid tile) maps the container
//! into its own rectangle with one uniform scale, centered ("contain"). Fullscreen is the identity, so the arrangement shown
//! in a tab is an exact miniature of the fullscreen arrangement and resizer drags convert back without distortion.

use super::layout::{LayoutParams, Rect};

/// Pixel size and scale factor of the virtual display. Width and height are at least 1, the dpi at least 0.5.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Container {
    pub width: i32,
    pub height: i32,
    pub dpi: f32,
}

impl Container {
    pub fn new(width: i32, height: i32, dpi: f32) -> Self {
        let dpi = if dpi.is_finite() { dpi.max(0.5) } else { 1.0 };
        Self {
            width: width.max(1),
            height: height.max(1),
            dpi,
        }
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }

    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    /// Layout parameters in container pixels. `gutter_floor` is a minimum gutter width in container pixels (0 for none).
    pub fn layout_params(
        &self,
        gutter_logical: f32,
        min_column_logical: f32,
        min_wide_aspect: f32,
        gutter_floor: i32,
    ) -> LayoutParams {
        let base = (gutter_logical * self.dpi).round() as i32;
        LayoutParams {
            width: self.width,
            height: self.height,
            gutter: base.max(gutter_floor),
            min_column: (min_column_logical * self.dpi).round() as i32,
            min_wide_aspect,
        }
    }
}

/// Smallest gutter (in container pixels) that is still `host_gutter_px` wide in the host after scaling by `scale`.
pub fn gutter_floor(host_gutter_px: i32, scale: f64) -> i32 {
    if scale >= 1.0 || scale <= 0.0 || host_gutter_px <= 0 {
        return 0;
    }
    (f64::from(host_gutter_px) / scale).ceil() as i32
}

/// Uniform mapping from container pixels to host pixels: `host = origin + round(container * scale)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    screen: Rect,
    scale: f64,
}

impl Projection {
    /// Fullscreen: the container is shown 1:1 at the origin of the host.
    pub fn identity(container: &Container) -> Self {
        Self {
            screen: container.rect(),
            scale: 1.0,
        }
    }

    /// Largest uniform scale that fits the container into `host`, centered inside it.
    pub fn fit(container: &Container, host: Rect) -> Self {
        let (host_w, host_h) = (host.w.max(1), host.h.max(1));
        let scale = (f64::from(host_w) / f64::from(container.width))
            .min(f64::from(host_h) / f64::from(container.height));
        let width = ((f64::from(container.width) * scale).round() as i32).clamp(1, host_w);
        let height = ((f64::from(container.height) * scale).round() as i32).clamp(1, host_h);
        let screen = Rect::new(
            host.x + (host_w - width) / 2,
            host.y + (host_h - height) / 2,
            width,
            height,
        );
        Self { screen, scale }
    }

    /// The projected container in host coordinates.
    pub fn screen(&self) -> Rect {
        self.screen
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Maps a container rectangle to host coordinates. Edges are rounded, not sizes, so neighbors that touch in the container
    /// keep touching in the host.
    pub fn rect(&self, r: Rect) -> Rect {
        let x0 = self.screen.x + (f64::from(r.x) * self.scale).round() as i32;
        let y0 = self.screen.y + (f64::from(r.y) * self.scale).round() as i32;
        let x1 = self.screen.x + (f64::from(r.x + r.w) * self.scale).round() as i32;
        let y1 = self.screen.y + (f64::from(r.y + r.h) * self.scale).round() as i32;
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    /// Maps a host point (for example the pointer) to container coordinates.
    pub fn point_to_container(&self, x: i32, y: i32) -> (i32, i32) {
        let to_container =
            |v: i32, origin: i32| (f64::from(v - origin) / self.scale).round() as i32;
        (
            to_container(x, self.screen.x),
            to_container(y, self.screen.y),
        )
    }

    /// Converts a distance in host pixels (drag delta, snap radius) to container pixels.
    pub fn length_to_container(&self, host_px: f32) -> f32 {
        (f64::from(host_px) / self.scale) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_HD: Container = Container {
        width: 1920,
        height: 1080,
        dpi: 1.0,
    };

    #[test]
    fn identity_leaves_every_rectangle_untouched() {
        let p = Projection::identity(&FULL_HD);
        assert_eq!(p.scale(), 1.0);
        assert_eq!(p.screen(), Rect::new(0, 0, 1920, 1080));
        for r in [
            Rect::new(0, 0, 608, 1080),
            Rect::new(624, 175, 1296, 729),
            Rect::new(7, 3, 1, 1),
        ] {
            assert_eq!(p.rect(r), r);
        }
        assert_eq!(p.point_to_container(321, 654), (321, 654));
        assert_eq!(p.length_to_container(12.5), 12.5);
    }

    #[test]
    fn a_host_of_the_same_size_is_the_identity_too() {
        let p = Projection::fit(&FULL_HD, Rect::new(0, 0, 1920, 1080));
        assert_eq!(p, Projection::identity(&FULL_HD));
    }

    #[test]
    fn fit_centers_and_keeps_the_aspect_ratio() {
        // 16:9 container in a 1500x800 viewport at (0, 64): height limits, side margins remain
        let p = Projection::fit(&FULL_HD, Rect::new(0, 64, 1500, 800));
        assert!((p.scale() - 800.0 / 1080.0).abs() < 1e-9);
        assert_eq!(p.screen().h, 800);
        assert_eq!(p.screen().y, 64);
        assert_eq!(p.screen().x, (1500 - p.screen().w) / 2);
        assert!((p.screen().w as f32 / p.screen().h as f32 - FULL_HD.aspect()).abs() < 0.01);

        // a very tall host: width limits, vertical margins remain
        let p = Projection::fit(&FULL_HD, Rect::new(10, 20, 960, 2000));
        assert_eq!(p.screen().w, 960);
        assert_eq!(p.screen().x, 10);
        assert_eq!(p.screen().y, 20 + (2000 - p.screen().h) / 2);
    }

    #[test]
    fn upscaling_into_a_larger_host_works() {
        let p = Projection::fit(&FULL_HD, Rect::new(0, 0, 3840, 2160));
        assert_eq!(p.scale(), 2.0);
        assert_eq!(
            p.rect(Rect::new(100, 50, 400, 300)),
            Rect::new(200, 100, 800, 600)
        );
    }

    #[test]
    fn touching_neighbors_still_touch_after_projection() {
        for scale_host in [
            Rect::new(0, 0, 1500, 800),
            Rect::new(0, 0, 777, 431),
            Rect::new(0, 0, 2400, 1300),
            Rect::new(0, 0, 321, 200),
        ] {
            let p = Projection::fit(&FULL_HD, scale_host);
            let (a, b, c) = (
                Rect::new(0, 0, 608, 1080),
                Rect::new(624, 0, 640, 1080),
                Rect::new(1280, 0, 640, 1080),
            );
            let (pa, pb, pc) = (p.rect(a), p.rect(b), p.rect(c));
            // the gutter between a and b (16 px) projects to the same gap on both sides
            assert_eq!(p.rect(Rect::new(608, 0, 16, 1080)).x, pa.x + pa.w);
            assert_eq!(
                p.rect(Rect::new(608, 0, 16, 1080)).x + p.rect(Rect::new(608, 0, 16, 1080)).w,
                pb.x
            );
            assert_eq!(p.rect(Rect::new(1264, 0, 16, 1080)).x, pb.x + pb.w);
            assert_eq!(
                p.rect(Rect::new(1264, 0, 16, 1080)).x + p.rect(Rect::new(1264, 0, 16, 1080)).w,
                pc.x
            );
            assert_eq!(
                pc.x + pc.w,
                p.screen().x + p.screen().w,
                "the last column ends at the projected container edge"
            );
        }
    }

    #[test]
    fn host_points_round_trip_into_the_container() {
        let p = Projection::fit(&FULL_HD, Rect::new(0, 64, 1500, 800));
        for r in [Rect::new(0, 0, 608, 1080), Rect::new(624, 175, 1296, 729)] {
            let host = p.rect(r);
            let (cx, cy) = p.point_to_container(host.x, host.y);
            assert!(
                (cx - r.x).abs() <= 2 && (cy - r.y).abs() <= 2,
                "{r:?} -> {host:?} -> ({cx}, {cy})"
            );
        }
    }

    #[test]
    fn distances_scale_back_to_container_pixels() {
        let p = Projection::fit(&FULL_HD, Rect::new(0, 0, 960, 540));
        assert_eq!(p.scale(), 0.5);
        assert_eq!(p.length_to_container(10.0), 20.0);
    }

    #[test]
    fn degenerate_inputs_never_panic() {
        let tiny = Container::new(0, -5, f32::NAN);
        assert_eq!((tiny.width, tiny.height, tiny.dpi), (1, 1, 1.0));
        let p = Projection::fit(&FULL_HD, Rect::new(0, 0, 0, 0));
        assert!(p.screen().w >= 1 && p.screen().h >= 1);
        assert!(p.rect(Rect::new(0, 0, 1920, 1080)).w >= 0);
        let p = Projection::fit(&tiny, Rect::new(5, 5, 100, 100));
        assert!(p.screen().w >= 1);
    }

    #[test]
    fn layout_params_follow_the_container_dpi() {
        let c = Container::new(2400, 1350, 1.25);
        let p = c.layout_params(16.0, 160.0, 1.2, 0);
        assert_eq!(
            (p.width, p.height, p.gutter, p.min_column),
            (2400, 1350, 20, 200)
        );
        assert_eq!(
            c.layout_params(16.0, 160.0, 1.2, 26).gutter,
            26,
            "a floor above the base gutter wins"
        );
        assert_eq!(
            c.layout_params(16.0, 160.0, 1.2, 4).gutter,
            20,
            "a floor below the base gutter is ignored"
        );
    }

    #[test]
    fn the_gutter_floor_keeps_host_gutters_usable() {
        assert_eq!(gutter_floor(16, 1.0), 0, "no floor at or above full scale");
        assert_eq!(gutter_floor(16, 2.0), 0);
        assert_eq!(gutter_floor(16, 0.5), 32);
        assert_eq!(gutter_floor(16, 0.74), 22);
        assert_eq!(gutter_floor(0, 0.5), 0);
        assert_eq!(gutter_floor(16, 0.0), 0);
    }
}
