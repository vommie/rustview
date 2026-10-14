//! Where a Multiview is shown: the host surface, the projection of the virtual container into it, the placement of the
//! video windows and the description a host draws.
//!
//! `set_host` is the only place that decides where windows go and which flags the players get; every host (the fullscreen
//! window, a viewer tab, a tab-grid tile) just hands in its rectangle.

use super::container::{Container, Projection, gutter_floor};
use super::instance::MultiviewInstance;
use super::instance_layout::layout_params;
use super::layout::{GutterKind, Rect};
use super::route::HostMode;
use super::view::{GutterView, HostedView, TileView};
use crate::settings::schema::{MULTIVIEW_GUTTER_LOGICAL_PX, MULTIVIEW_MIN_VIDEO_WINDOW_PX};
use crate::viewer::{SubwindowGeometry, ViewportBounds, XRectangle};

/// The rectangle a host gives to a Multiview, in physical pixels of the window `parent_xid`.
#[derive(Debug, Clone, PartialEq)]
pub struct HostSurface {
    pub mode: HostMode,
    pub parent_xid: u64,
    pub rect: Rect,
    /// Scale factor of the host window.
    pub dpi: f32,
    /// Visible part of the parent (the tab-grid viewport): video windows are clipped to it. `None` clips nothing.
    pub clip: Option<Rect>,
    /// Areas of the parent that must stay free of video (context menus, tooltips), in parent coordinates.
    pub overlay_cutouts: Vec<XRectangle>,
}

impl HostSurface {
    /// A frameless fullscreen window of `width` x `height` physical pixels.
    pub fn fullscreen(parent_xid: u64, width: i32, height: i32, dpi: f32) -> Self {
        Self {
            mode: HostMode::Fullscreen,
            parent_xid,
            rect: Rect::new(0, 0, width.max(1), height.max(1)),
            dpi,
            clip: None,
            overlay_cutouts: Vec::new(),
        }
    }

    /// The media area of the active viewer tab inside the main window.
    pub fn tab_view(
        parent_xid: u64,
        rect: Rect,
        dpi: f32,
        overlay_cutouts: Vec<XRectangle>,
    ) -> Self {
        Self {
            mode: HostMode::TabView,
            parent_xid,
            rect,
            dpi,
            clip: None,
            overlay_cutouts,
        }
    }

    /// The canvas of a tab-grid item; `clip` is the visible part of the grid.
    pub fn tab_grid(
        parent_xid: u64,
        rect: Rect,
        dpi: f32,
        clip: Rect,
        overlay_cutouts: Vec<XRectangle>,
    ) -> Self {
        Self {
            mode: HostMode::TabGrid,
            parent_xid,
            rect,
            dpi,
            clip: Some(clip),
            overlay_cutouts,
        }
    }

    fn same_geometry(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.parent_xid == other.parent_xid
            && self.rect == other.rect
            && self.dpi == other.dpi
            && self.clip == other.clip
    }
}

pub(super) struct HostState {
    pub surface: HostSurface,
    pub projection: Projection,
    /// The mode the player flags were last applied for.
    pub flags_mode: Option<HostMode>,
}

impl HostState {
    pub fn hidden(container: &Container) -> Self {
        Self {
            surface: HostSurface {
                mode: HostMode::Hidden,
                parent_xid: 0,
                rect: container.rect(),
                dpi: container.dpi,
                clip: None,
                overlay_cutouts: Vec::new(),
            },
            projection: Projection::identity(container),
            flags_mode: None,
        }
    }
}

/// A rectangle of the host in the coordinates of the parent window, as the X server wants it.
pub(super) fn cutout_rectangle(rect: Rect) -> XRectangle {
    XRectangle {
        x: rect.x.clamp(0, i16::MAX as i32) as i16,
        y: rect.y.clamp(0, i16::MAX as i32) as i16,
        width: rect.w.clamp(1, u16::MAX as i32) as u16,
        height: rect.h.clamp(1, u16::MAX as i32) as u16,
    }
}

impl MultiviewInstance {
    /// Shows the Multiview in `surface`. Returns `false` if nothing changed. A fullscreen host sets the container (the
    /// display the Multiview was last shown on); every other host scales that container into its rectangle.
    pub fn set_host(&mut self, surface: &HostSurface) -> bool {
        if self.host.surface.same_geometry(surface) {
            let changed = self.host.surface.overlay_cutouts != surface.overlay_cutouts;
            self.set_overlay_cutouts(&surface.overlay_cutouts);
            return changed;
        }
        if surface.mode == HostMode::Fullscreen {
            self.container = Container::new(surface.rect.w, surface.rect.h, surface.dpi);
        }
        let projection = Projection::fit(&self.container, surface.rect);
        let params = layout_params(&self.container, self.gutter_floor_in(surface, &projection));
        let relayout = params != self.params || self.layout.tiles.len() != self.tiles.len();
        self.params = params;
        self.host.surface = surface.clone();
        self.host.projection = projection;
        tracing::debug!(
            instance = self.id,
            mode = ?surface.mode,
            width = surface.rect.w,
            height = surface.rect.h,
            scale = surface.dpi,
            relayout,
            "[Multiview: Layout] Window geometry changed"
        );
        self.reparent_windows();
        if relayout {
            self.apply_layout();
        } else {
            self.reproject();
        }
        self.apply_player_flags();
        true
    }

    /// The interactive tab view keeps its gutters wide enough to grab even when the container is scaled down a lot; every
    /// other host shows the exact miniature.
    fn gutter_floor_in(&self, surface: &HostSurface, projection: &Projection) -> i32 {
        if surface.mode != HostMode::TabView {
            return 0;
        }
        let host_gutter = (MULTIVIEW_GUTTER_LOGICAL_PX * surface.dpi).round() as i32;
        gutter_floor(host_gutter, projection.scale())
    }

    /// Replaces the context menu and tooltip cutouts of the host and applies them to every visible video window.
    pub fn set_overlay_cutouts(&mut self, cutouts: &[XRectangle]) {
        if self.host.surface.overlay_cutouts == cutouts {
            return;
        }
        self.host.surface.overlay_cutouts = cutouts.to_vec();
        self.apply_all_cutouts();
    }

    /// The host rectangle of the video window of tile `index`.
    pub(super) fn host_content(&self, index: usize) -> Option<Rect> {
        self.layout
            .tiles
            .get(index)
            .map(|t| self.host.projection.rect(t.content))
    }

    /// Re-places everything after the layout or the projection changed. The shape of a video window is relative to the
    /// window, so it is reset before any window moves and applied again afterwards.
    pub(super) fn reproject(&mut self) {
        if let Some(index) = self.bar_tile {
            self.refresh_shape(index);
        }
        self.place_windows();
        self.apply_all_cutouts();
        self.bump();
    }

    /// Moves windows that live below another parent (for example after the fullscreen window went away) below the parent
    /// of the host. The shape is reset first because it is relative to the window and would clip at a stale place.
    fn reparent_windows(&mut self) {
        let parent = self.host.surface.parent_xid;
        let projection = self.host.projection;
        for (tile, placed) in self.tiles.iter_mut().zip(&self.layout.tiles) {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            if video.window.parent_xid() == parent {
                continue;
            }
            let r = projection.rect(placed.content);
            video.window.reset_clip_shape();
            let moved = video.window.reparent(
                parent,
                r.x,
                r.y,
                r.w.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX) as u32,
                r.h.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX) as u32,
            );
            match moved {
                Ok(()) => tracing::debug!(
                    media_id = tile.media_id,
                    parent,
                    "[Multiview: Host] Video window moved to the new parent"
                ),
                Err(e) => tracing::warn!(
                    media_id = tile.media_id,
                    error = %e,
                    "[Multiview: Host] Video window could not be moved to the new parent"
                ),
            }
        }
    }

    /// Moves every video window to the projection of its tile; windows that would become too small are hidden. In the
    /// tab grid the windows are also clipped to the visible part of the grid.
    fn place_windows(&mut self) {
        let projection = self.host.projection;
        let (mode, clip) = (self.host.surface.mode, self.host.surface.clip);
        for (tile, placed) in self.tiles.iter_mut().zip(&self.layout.tiles) {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            let r = projection.rect(placed.content);
            if r.w < MULTIVIEW_MIN_VIDEO_WINDOW_PX || r.h < MULTIVIEW_MIN_VIDEO_WINDOW_PX {
                video.window.hide();
                continue;
            }
            match (mode, clip) {
                (HostMode::TabGrid, Some(clip)) => {
                    video.window.update_geometry_and_clip(
                        SubwindowGeometry {
                            x: r.x,
                            y: r.y,
                            width: r.w as u32,
                            height: r.h as u32,
                        },
                        ViewportBounds {
                            x: clip.x,
                            y: clip.y,
                            width: clip.w.max(1) as u32,
                            height: clip.h.max(1) as u32,
                        },
                    );
                }
                _ => {
                    video.window.show();
                    video.window.move_resize(r.x, r.y, r.w as u32, r.h as u32);
                }
            }
        }
    }

    /// Everything that has to be cut out of the video window of tile `index`: the overlays of the host (menus, tooltips)
    /// plus the hover bar if it belongs to this tile. The shape of a window holds one set of rectangles, so they are
    /// applied together.
    fn window_cutouts(&self, index: usize) -> Vec<XRectangle> {
        let mut rects = self.host.surface.overlay_cutouts.clone();
        if self.bar_tile == Some(index)
            && let Some(bar) = self.bar_rect
        {
            rects.push(cutout_rectangle(bar));
        }
        rects
    }

    pub(super) fn apply_cutouts(&mut self, index: usize) {
        let rects = self.window_cutouts(index);
        if let Some(video) = self.tiles.get_mut(index).and_then(|t| t.video.as_mut()) {
            video.window.clip_cutout_rectangles(&rects);
        }
    }

    fn apply_all_cutouts(&mut self) {
        for index in 0..self.tiles.len() {
            self.apply_cutouts(index);
        }
    }

    /// Sets the mpv flags that belong to the mode of the host, once per mode change. The user's pause state is never
    /// touched: `set_view_active(true)` resumes only players the user did not pause.
    fn apply_player_flags(&mut self) {
        let mode = self.host.surface.mode;
        if self.host.flags_mode == Some(mode) {
            return;
        }
        self.host.flags_mode = Some(mode);
        let (grid, active) = match mode {
            HostMode::Hidden => return,
            HostMode::TabGrid => (true, false),
            HostMode::Fullscreen | HostMode::TabView => (false, true),
        };
        for video in self.tiles.iter().filter_map(|t| t.video.as_ref()) {
            video.player.set_tab_grid_mode(grid);
            video.player.set_view_active(active);
        }
        if active {
            self.apply_audio();
        }
    }

    /// Moves every video window below `parent_xid`, hidden and paused, so the Multiview can live on as a background tab
    /// after its fullscreen window is gone. The host becomes `Hidden`; the next `set_host` places and resumes everything.
    pub fn park(&mut self, parent_xid: u64, rect: Rect) -> Result<(), String> {
        self.clear_hover();
        self.rename_pinned = false;
        let (width, height) = (
            rect.w.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX) as u32,
            rect.h.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX) as u32,
        );
        let mut failed = None;
        for tile in &mut self.tiles {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            video.window.reset_clip_shape();
            if let Err(e) = video
                .window
                .reparent(parent_xid, rect.x, rect.y, width, height)
            {
                failed = Some(e.to_string());
            }
            video.window.hide();
            video.player.set_view_active(false);
            video.player.set_tab_grid_mode(false);
        }
        self.host.surface = HostSurface {
            mode: HostMode::Hidden,
            parent_xid,
            rect,
            dpi: self.host.surface.dpi,
            clip: None,
            overlay_cutouts: Vec::new(),
        };
        self.host.flags_mode = None;
        self.bump();
        tracing::info!(
            instance = self.id,
            players = self.tiles.iter().filter(|t| t.video.is_some()).count(),
            ok = failed.is_none(),
            "[Multiview: Host] Parked below the main window"
        );
        failed.map_or(Ok(()), Err)
    }

    /// Hides the windows and pauses the players because another tab (or the Browse view) took over. The user's pause
    /// state is kept; `set_host` brings everything back.
    pub fn deactivate(&mut self) {
        if self.host.surface.mode == HostMode::Hidden {
            return;
        }
        self.clear_hover();
        self.rename_pinned = false;
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            video.window.hide();
            video.player.set_view_active(false);
        }
        self.host.surface.mode = HostMode::Hidden;
        self.host.flags_mode = None;
        self.bump();
    }

    /// Stops playback and hides the windows because the tab was closed. The instance is destroyed by its last owner.
    pub fn stop_and_hide(&mut self) {
        self.clear_hover();
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            video.player.stop();
            video.window.hide();
        }
        self.host.surface.mode = HostMode::Hidden;
        self.host.flags_mode = None;
        self.bump();
    }

    /// What the host has to draw, with rectangles relative to the host rectangle.
    pub fn view(&self) -> HostedView {
        let (origin_x, origin_y) = (self.host.surface.rect.x, self.host.surface.rect.y);
        let relative = |r: Rect| Rect::new(r.x - origin_x, r.y - origin_y, r.w, r.h);
        let projection = &self.host.projection;
        let tiles = self
            .tiles
            .iter()
            .zip(&self.layout.tiles)
            .enumerate()
            .map(|(index, (tile, placed))| TileView {
                media_id: tile.media_id,
                rect: relative(projection.rect(placed.content)),
                kind: tile.kind,
                pixels: tile.pixels.clone(),
                is_loading: tile.is_loading && tile.pixels.is_none(),
                error: tile.error.clone(),
                actions: self.action_views(index),
            })
            .collect();
        let gutters = self
            .layout
            .gutters
            .iter()
            .map(|g| GutterView {
                rect: relative(projection.rect(g.rect)),
                vertical: g.kind == GutterKind::Vertical,
            })
            .collect();
        HostedView {
            revision: self.revision,
            dpi: self.host.surface.dpi,
            screen: relative(projection.screen()),
            tiles,
            gutters,
            bar_tile: self.bar_tile,
            navigation: self.navigation(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::instance::SharedInstance;
    use crate::viewer::multiview::layout;
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::tile::TileSeed;
    use std::path::PathBuf;

    const MAIN: u64 = 77;

    fn instance(aspects: &[f32]) -> SharedInstance {
        let seeds = aspects
            .iter()
            .enumerate()
            .map(|(i, &aspect)| TileSeed {
                media_id: i as u64 + 1,
                kind: TileKind::Image,
                aspect,
                path: PathBuf::new(),
                filename: format!("{i}.png"),
            })
            .collect();
        MultiviewInstance::detached(seeds)
    }

    fn cutout(x: i16, y: i16, width: u16, height: u16) -> XRectangle {
        XRectangle {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_tab_is_an_exact_miniature_of_the_fullscreen_arrangement() {
        let shared = instance(&[1.78, 0.56, 1.0]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 1920, 1080, 1.0));
        let fullscreen = mv.layout.clone();
        let fullscreen_view = mv.view();

        let host = Rect::new(0, 64, 960, 540);
        mv.set_host(&HostSurface::tab_grid(
            MAIN,
            host,
            1.0,
            Rect::new(0, 0, 1000, 700),
            Vec::new(),
        ));
        assert_eq!(
            mv.layout, fullscreen,
            "the arrangement lives in container pixels"
        );
        assert_eq!(mv.container().width, 1920);

        let view = mv.view();
        assert_eq!(view.screen, Rect::new(0, 0, 960, 540));
        for (small, big) in view.tiles.iter().zip(&fullscreen_view.tiles) {
            assert!(
                (small.rect.x - big.rect.x / 2).abs() <= 1,
                "{:?} vs {:?}",
                small.rect,
                big.rect
            );
            assert!((small.rect.w - big.rect.w / 2).abs() <= 1);
            assert!((small.rect.h - big.rect.h / 2).abs() <= 1);
        }
    }

    #[test]
    fn a_host_with_another_aspect_ratio_is_letterboxed_not_stretched() {
        let shared = instance(&[1.78, 1.78]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 1920, 1080, 1.0));
        mv.set_host(&HostSurface::tab_grid(
            MAIN,
            Rect::new(100, 200, 600, 600),
            1.0,
            Rect::new(0, 0, 2000, 2000),
            Vec::new(),
        ));
        let screen = mv.view().screen;
        assert_eq!(screen.w, 600);
        assert!(
            (screen.h - 338).abs() <= 1,
            "16:9 inside a square: {screen:?}"
        );
        assert_eq!(screen.y, (600 - screen.h) / 2, "centered vertically");
    }

    #[test]
    fn the_tab_view_keeps_the_gutters_grabbable() {
        let shared = instance(&[1.78, 1.78]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 3840, 2160, 1.0));
        let exact_gutter = mv.params.gutter;
        mv.set_host(&HostSurface::tab_view(
            MAIN,
            Rect::new(0, 64, 960, 540),
            1.0,
            Vec::new(),
        ));
        let scale = mv.host.projection.scale();
        let host_gutter = (f64::from(mv.params.gutter) * scale).round() as i32;
        assert!(
            mv.params.gutter > exact_gutter,
            "the container gutter grows"
        );
        assert!(host_gutter >= 16, "{host_gutter} px in the host");

        mv.set_host(&HostSurface::tab_grid(
            MAIN,
            Rect::new(0, 64, 960, 540),
            1.0,
            Rect::new(0, 0, 2000, 2000),
            Vec::new(),
        ));
        assert_eq!(
            mv.params.gutter, exact_gutter,
            "the grid shows the exact miniature"
        );
    }

    #[test]
    fn the_container_follows_the_display_only_in_fullscreen() {
        let shared = instance(&[1.78, 1.78]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 2560, 1440, 1.25));
        assert_eq!((mv.container().width, mv.container().dpi), (2560, 1.25));
        mv.set_host(&HostSurface::tab_view(
            MAIN,
            Rect::new(0, 64, 800, 500),
            1.0,
            Vec::new(),
        ));
        assert_eq!((mv.container().width, mv.container().dpi), (2560, 1.25));
        mv.set_host(&HostSurface::fullscreen(2, 1920, 1080, 1.0));
        assert_eq!((mv.container().width, mv.container().height), (1920, 1080));
        assert_eq!(mv.view().screen, Rect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn a_dragged_split_survives_a_round_trip_through_a_tab() {
        let shared = instance(&[1.78, 1.78, 1.78]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 1920, 1080, 1.0));
        mv.drag_started(0);
        mv.dragged(120.0);
        let pins = mv.pins();
        assert!(!pins.is_auto(), "the drag must pin the neighbors");
        mv.set_host(&HostSurface::tab_view(
            MAIN,
            Rect::new(0, 64, 700, 400),
            1.0,
            Vec::new(),
        ));
        mv.set_host(&HostSurface::fullscreen(2, 1920, 1080, 1.0));
        assert_eq!(mv.pins(), pins);
        let expected = layout::solve(&mv.params, &[1.78, 1.78, 1.78], &pins);
        assert_eq!(mv.layout, expected);
    }

    #[test]
    fn a_tab_drag_converts_back_to_container_pixels() {
        let shared = instance(&[1.78, 1.78]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 1920, 1080, 1.0));
        mv.set_host(&HostSurface::tab_view(
            MAIN,
            Rect::new(0, 64, 960, 540),
            1.0,
            Vec::new(),
        ));
        let before = mv.layout.gutters[0].rect.x;
        mv.drag_started(0);
        mv.dragged(10.0);
        let after = mv.layout.gutters[0].rect.x;
        assert!(
            (after - before - 20).abs() <= 2,
            "10 host px are about 20 container px at scale 0.5: {before} -> {after}"
        );
    }

    #[test]
    fn overlay_cutouts_are_stored_and_deduplicated() {
        let shared = instance(&[1.78, 1.78]);
        let mut mv = shared.lock();
        let surface = HostSurface::tab_view(
            MAIN,
            Rect::new(0, 64, 960, 540),
            1.0,
            vec![cutout(10, 10, 100, 50)],
        );
        assert!(mv.set_host(&surface));
        assert!(!mv.set_host(&surface), "the same surface changes nothing");
        assert_eq!(
            mv.host.surface.overlay_cutouts,
            vec![cutout(10, 10, 100, 50)]
        );

        let moved = HostSurface {
            overlay_cutouts: vec![cutout(20, 20, 100, 50)],
            ..surface
        };
        let revision = mv.revision();
        assert!(mv.set_host(&moved), "only the cutouts changed");
        assert_eq!(
            mv.revision(),
            revision,
            "cutouts are not part of the drawn view"
        );
        assert_eq!(
            mv.host.surface.overlay_cutouts,
            vec![cutout(20, 20, 100, 50)]
        );
    }

    #[test]
    fn the_bar_cutout_is_merged_with_the_overlay_cutouts() {
        let shared = instance(&[1.78, 1.78]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::tab_view(
            MAIN,
            Rect::new(0, 64, 960, 540),
            1.0,
            vec![cutout(10, 10, 100, 50)],
        ));
        assert_eq!(mv.window_cutouts(0), vec![cutout(10, 10, 100, 50)]);

        mv.bar_tile = Some(0);
        assert_eq!(
            mv.window_cutouts(0),
            vec![cutout(10, 10, 100, 50)],
            "no cutout until the host reports where the bar is"
        );
        mv.set_bar_overlay(Some(Rect::new(300, 26, 120, 34)));
        assert_eq!(
            mv.window_cutouts(0),
            vec![cutout(10, 10, 100, 50), cutout(300, 90, 120, 34)],
            "the bar is placed relative to the host rectangle (y + 64)"
        );
        assert_eq!(
            mv.window_cutouts(1),
            vec![cutout(10, 10, 100, 50)],
            "the cutout belongs to the window of one tile"
        );
    }

    #[test]
    fn parking_and_deactivating_leave_the_state_consistent() {
        let shared = instance(&[1.78, 0.56]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 1920, 1080, 1.0));
        assert!(mv.park(MAIN, Rect::new(0, 64, 800, 600)).is_ok());
        assert_eq!(mv.host_mode(), HostMode::Hidden);
        assert_eq!(mv.host.surface.parent_xid, MAIN);

        let surface = HostSurface::tab_view(MAIN, Rect::new(0, 64, 800, 600), 1.0, Vec::new());
        assert!(mv.set_host(&surface), "a parked instance is placed again");
        assert_eq!(mv.host_mode(), HostMode::TabView);
        mv.deactivate();
        assert_eq!(mv.host_mode(), HostMode::Hidden);
        assert!(
            mv.set_host(&surface),
            "a deactivated instance is placed again"
        );
    }

    #[test]
    fn tiles_that_become_tiny_do_not_break_the_view() {
        let shared = instance(&[1.78, 1.78, 1.78, 1.78]);
        let mut mv = shared.lock();
        mv.set_host(&HostSurface::fullscreen(1, 1920, 1080, 1.0));
        mv.set_host(&HostSurface::tab_grid(
            MAIN,
            Rect::new(0, 0, 40, 24),
            1.0,
            Rect::new(0, 0, 400, 400),
            Vec::new(),
        ));
        let view = mv.view();
        assert_eq!(view.tiles.len(), 4);
        assert!(view.tiles.iter().all(|t| t.rect.w >= 0 && t.rect.h >= 0));
    }
}
