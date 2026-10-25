//! Where a Multiview is shown: the host surface, the projection of the virtual container into it, the placement of the
//! video windows and the description a host draws.
//!
//! `set_host` is the only place that decides where windows go and which flags the players get; every host (the fullscreen
//! window, a viewer tab, a tab-grid tile) just hands in its rectangle.

use super::container::{Container, Projection, gutter_floor, lone_medium_fills_host};
use super::instance::MultiviewInstance;
use super::instance_layout::layout_params;
use super::layout::{GutterKind, LayoutParams, Rect};
use super::route::HostMode;
use super::view::{GutterView, HostedView, TileView};
use crate::viewer::{SubwindowGeometry, ViewportBounds, XRectangle};
use rustview_core::settings::schema::{MULTIVIEW_GUTTER_LOGICAL_PX, MULTIVIEW_MIN_VIDEO_WINDOW_PX};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use std::time::Instant;

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
    pub fn tab_view(parent_xid: u64, rect: Rect, dpi: f32, overlay_cutouts: Vec<XRectangle>) -> Self {
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
    pub fn tab_grid(parent_xid: u64, rect: Rect, dpi: f32, clip: Rect, overlay_cutouts: Vec<XRectangle>) -> Self {
        Self {
            mode: HostMode::TabGrid,
            parent_xid,
            rect,
            dpi,
            clip: Some(clip),
            overlay_cutouts,
        }
    }

    #[allow(
        clippy::float_cmp,
        reason = "the DPI is copied from the window, never computed; any difference means a re-layout"
    )]
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
    /// A menu of the application is open: the video windows let the pointer through.
    pub menu_open: bool,
    /// A dialog is open over the host: the video windows let the pointer through.
    pub modal_locked: bool,
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
            menu_open: false,
            modal_locked: false,
        }
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
        let (projection, params) = self.geometry_in(surface.mode, surface.rect, surface.dpi);
        let relayout = params != self.params || self.layout.tiles.len() != self.tiles.len();
        self.params = params;
        self.host.surface = surface.clone();
        if surface.mode != HostMode::Fullscreen {
            self.pinned = false;
        }
        if !matches!(surface.mode, HostMode::Fullscreen | HostMode::TabView) {
            // The slider hangs below a bar that exists in these two hosts only.
            self.zoom_slider.clear();
            self.zoom_minimap_bubble.clear();
        }
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
        // A Multiview that was never shown (a tab opened in the background) starts with the first host that shows it. The
        // fullscreen starts its tiles itself, once its native window exists.
        if surface.mode != HostMode::Fullscreen {
            self.start_all();
        }
        true
    }

    /// The container the arrangement is solved in at the moment: the display's, except for a lone medium in the tab view, which
    /// gets the shape of the tab area so that it covers all of it (see `lone_medium_fills_host`).
    pub(super) fn solving_container(&self) -> Container {
        self.solving_container_in(self.host.surface.mode, self.host.surface.rect, self.host.surface.dpi)
    }

    fn solving_container_in(&self, mode: HostMode, rect: Rect, dpi: f32) -> Container {
        if lone_medium_fills_host(mode, self.tiles.len()) {
            Container::of_host(rect, dpi)
        } else {
            self.container
        }
    }

    /// The projection of the solving container into a host rectangle and the layout parameters for it.
    fn geometry_in(&self, mode: HostMode, rect: Rect, dpi: f32) -> (Projection, LayoutParams) {
        let container = self.solving_container_in(mode, rect, dpi);
        let projection = Projection::fit(&container, rect);
        let params = layout_params(&container, Self::gutter_floor_in(mode, dpi, &projection));
        (projection, params)
    }

    /// Fits projection and layout parameters to the current host and tile count. Called before every layout solve: the tile
    /// count decides the solving container, so the switch between a lone medium and an arrangement happens right where tiles
    /// come and go.
    pub(super) fn refresh_geometry(&mut self) {
        let surface = &self.host.surface;
        let (projection, params) = self.geometry_in(surface.mode, surface.rect, surface.dpi);
        self.host.projection = projection;
        self.params = params;
    }

    /// The interactive tab view keeps its gutters wide enough to grab even when the container is scaled down a lot; every
    /// other host shows the exact miniature.
    fn gutter_floor_in(mode: HostMode, dpi: f32, projection: &Projection) -> i32 {
        if mode != HostMode::TabView {
            return 0;
        }
        let host_gutter = (MULTIVIEW_GUTTER_LOGICAL_PX * dpi).round().to_int::<i32>();
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

    /// A menu of the application is open (or closed again): the video windows let the pointer through to the main window while
    /// it is, so that a click anywhere closes the menu (see `X11VideoSubwindow::set_input_passthrough`).
    pub fn set_menu_open(&mut self, open: bool) {
        self.host.menu_open = open;
        self.apply_pointer_passthrough();
    }

    /// A dialog is open over the host (or closed again): the video windows let the pointer through to the host window, whose
    /// lock answers the click (the dialog comes to the front) instead of mpv (a double click would toggle the fullscreen).
    pub fn set_modal_locked(&mut self, locked: bool) {
        if self.host.modal_locked == locked {
            return;
        }
        self.host.modal_locked = locked;
        self.apply_pointer_passthrough();
    }

    /// The video windows take no pointer input wherever the Multiview is shown: `RustView` handles the mouse over a video
    /// itself (its control bar, clicks, the wheel; in the tab grid the grid item does, like over a picture). Called on every
    /// tick and every change of the host, so a video that starts later gets it too (the windows change nothing when the
    /// state is the same).
    pub(super) fn apply_pointer_passthrough(&mut self) {
        let through = self.host.surface.mode != HostMode::Hidden || self.host.menu_open || self.host.modal_locked;
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            video.window.set_input_passthrough(through);
        }
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
        self.sync_zoom_frames();
        self.place_windows();
        self.apply_all_cutouts();
        self.sync_feedback_overlays(Instant::now());
        self.sync_focus_overlays();
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
                r.w.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX).saturating_as::<u32>(),
                r.h.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX).saturating_as::<u32>(),
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
        // A zoomed video uses the room of its tile (`instance_video_zoom`).
        let rects: Vec<Option<Rect>> = (0..self.tiles.len()).map(|i| self.video_window_rect(i)).collect();
        for ((tile, placed), zoomed) in self.tiles.iter_mut().zip(&self.layout.tiles).zip(rects) {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            let r = zoomed.unwrap_or_else(|| projection.rect(placed.content));
            if r.w < MULTIVIEW_MIN_VIDEO_WINDOW_PX || r.h < MULTIVIEW_MIN_VIDEO_WINDOW_PX {
                video.window.hide();
                continue;
            }
            video.zoom_window = Some(r);
            if let (HostMode::TabGrid, Some(clip)) = (mode, clip) {
                video.window.update_geometry_and_clip(
                    SubwindowGeometry {
                        x: r.x,
                        y: r.y,
                        width: r.w.saturating_as::<u32>(),
                        height: r.h.saturating_as::<u32>(),
                    },
                    ViewportBounds {
                        x: clip.x,
                        y: clip.y,
                        width: clip.w.max(1).saturating_as::<u32>(),
                        height: clip.h.max(1).saturating_as::<u32>(),
                    },
                );
            } else {
                video.window.show();
                video
                    .window
                    .move_resize(r.x, r.y, r.w.saturating_as::<u32>(), r.h.saturating_as::<u32>());
            }
        }
    }

    /// Cuts the overlays of the host (menus, tooltips) out of the video window of tile `index`. The action bar is not one
    /// of them: it floats in a window of its own above the Multiview and is blended with the video by the compositor.
    pub(super) fn apply_cutouts(&mut self, index: usize) {
        let mut rects = self.host.surface.overlay_cutouts.clone();
        if let Some(video) = self.tiles.get_mut(index).and_then(|t| t.video.as_mut()) {
            // The minimap of a zoomed video (and the slider bubble on it), drawn by the canvas below the window.
            rects.extend_from_slice(&video.minimap_cut);
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
        self.apply_video_background();
        self.apply_pointer_passthrough();
        if mode != HostMode::TabGrid {
            self.sync_time_badges(&[]);
        }
        if active {
            self.apply_audio();
        }
    }

    /// Sets the color behind the media of the fullscreen (`None`: mpv's own black), for videos that are playing now and
    /// those that start later. Only the fullscreen host uses it; a tab always gets mpv's default back.
    pub fn set_video_background(&mut self, color: Option<[u8; 3]>) {
        if self.video_background == color {
            return;
        }
        self.video_background = color;
        self.apply_video_background();
    }

    /// The background color mpv has to use in the current host.
    pub(super) fn effective_video_background(&self) -> [u8; 3] {
        match self.host.surface.mode {
            HostMode::Fullscreen => self.video_background.unwrap_or([0, 0, 0]),
            _ => [0, 0, 0],
        }
    }

    pub(super) fn apply_video_background(&self) {
        let color = self.effective_video_background();
        for video in self.tiles.iter().filter_map(|t| t.video.as_ref()) {
            video.player.set_background_color(color);
        }
        tracing::debug!(
            instance = self.id,
            mode = ?self.host.surface.mode,
            "[Multiview: Background] Video background applied"
        );
    }

    /// Moves every video window below `parent_xid`, hidden and paused, so the Multiview can live on as a background tab
    /// after its fullscreen window is gone. The host becomes `Hidden`; the next `set_host` places and resumes everything.
    pub fn park(&mut self, parent_xid: u64, rect: Rect) -> Result<(), String> {
        self.clear_hover();
        let (width, height) = (
            rect.w.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX).saturating_as::<u32>(),
            rect.h.max(MULTIVIEW_MIN_VIDEO_WINDOW_PX).saturating_as::<u32>(),
        );
        let mut failed = None;
        for tile in &mut self.tiles {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            video.window.reset_clip_shape();
            if let Err(e) = video.window.reparent(parent_xid, rect.x, rect.y, width, height) {
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
        self.clear_feedback();
        self.clear_hover();
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
        self.clear_feedback();
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
        let now = Instant::now();
        let tiles = self
            .tiles
            .iter()
            .zip(&self.layout.tiles)
            .enumerate()
            .map(|(index, (tile, placed))| {
                let (zoom, minimap, pannable) = self.zoom_views(index);
                TileView {
                    key: tile.key,
                    media_id: tile.media_id,
                    rect: relative(projection.rect(placed.content)),
                    kind: tile.kind,
                    pixels: tile.pixels.clone(),
                    is_loading: tile.is_loading && tile.pixels.is_none(),
                    error: tile.error.clone(),
                    zoom,
                    minimap,
                    pannable,
                    feedback: self.feedback_pulse(tile.key, now),
                    crop: self.crop_view(index),
                    dirty: self.tile_is_dirty(index),
                    focus: self.focus_view(index),
                }
            })
            .collect();
        let gutters = self
            .layout
            .gutters
            .iter()
            .map(|g| GutterView {
                rect: relative(projection.rect(g.rect)),
                vertical: g.kind == GutterKind::Vertical,
                swap_only: g.kind == GutterKind::ColumnSwap,
            })
            .collect();
        HostedView {
            revision: self.revision,
            dpi: self.host.surface.dpi,
            screen: relative(projection.screen()),
            tiles,
            gutters,
            navigation: self.navigation(),
        }
    }
}
