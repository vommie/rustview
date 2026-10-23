//! The virtual container of a Multiview and its projection into a host rectangle (pure).
//!
//! A Multiview is always laid out in the pixel space of a virtual display, the container: the size and scale factor of the
//! display the fullscreen Multiview was last shown on. A host (fullscreen window, tab view, tab-grid tile) maps the container
//! into its own rectangle with one uniform scale, centered ("contain"). Fullscreen is the identity, so the arrangement shown
//! in a tab is an exact miniature of the fullscreen arrangement and resizer drags convert back without distortion.

use super::layout::{LayoutParams, Rect};
use super::route::HostMode;

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

    /// A container with the shape of a host rectangle: the Multiview is solved directly in it and projected 1:1.
    pub fn of_host(rect: Rect, dpi: f32) -> Self {
        Self::new(rect.w, rect.h, dpi)
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

/// A lone medium in the tab view uses the whole area of the tab instead of the miniature of the display: the display is a
/// stand-in for the fullscreen arrangement of several media, and scaled into a tab area of another shape it would leave bars
/// around a medium that has no neighbors to be arranged with. Two or more tiles are always arranged in the display's shape,
/// so the tab shows the miniature of the fullscreen arrangement. The decision is made again whenever the tile count or the
/// host changes (see `MultiviewInstance::apply_layout`).
pub fn lone_medium_fills_host(mode: HostMode, tile_count: usize) -> bool {
    mode == HostMode::TabView && tile_count == 1
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
        let scale =
            (f64::from(host_w) / f64::from(container.width)).min(f64::from(host_h) / f64::from(container.height));
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
        let to_container = |v: i32, origin: i32| (f64::from(v - origin) / self.scale).round() as i32;
        (to_container(x, self.screen.x), to_container(y, self.screen.y))
    }

    /// Converts a distance in host pixels (drag delta, snap radius) to container pixels.
    pub fn length_to_container(&self, host_px: f32) -> f32 {
        (f64::from(host_px) / self.scale) as f32
    }
}
