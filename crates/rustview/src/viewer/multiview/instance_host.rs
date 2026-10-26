//! Where a Multiview is shown: the host surface, the projection of the virtual container into it, the size and window of the
//! video surfaces and the description a host draws.
//!
//! `set_host` is the only place that decides which window draws the videos and which flags the players get; every host (the
//! fullscreen window, a viewer tab, a tab-grid tile) just hands in its window and rectangle.

use super::container::{Container, Projection, gutter_floor, lone_medium_fills_host};
use super::instance::MultiviewInstance;
use super::instance_layout::layout_params;
use super::instance_video_zoom::note_placement;
use super::layout::{GutterKind, LayoutParams, Rect};
use super::route::HostMode;
use super::video_placement::VideoPlacement;
use super::view::{GutterView, HostedView, TileView, VideoFrameView};
use crate::viewer::video_surface::HostWindowId;
use rustview_core::settings::schema::{MULTIVIEW_GUTTER_LOGICAL_PX, MULTIVIEW_MIN_VIDEO_WINDOW_PX};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use std::time::Instant;

/// The rectangle a host gives to a Multiview, in physical pixels of the Slint window `window` (whose canvas draws the videos).
#[derive(Debug, Clone, PartialEq)]
pub struct HostSurface {
    pub mode: HostMode,
    /// The window that draws the videos (`video_surface::hosts`; 0 = none).
    pub window: HostWindowId,
    /// The X11 window of the host (0 on Wayland): only the hint window is placed relative to it (it goes in M3 of
    /// `docs/video_in_slint/HANDOFF.md`).
    pub parent_xid: u64,
    pub rect: Rect,
    /// Scale factor of the host window.
    pub dpi: f32,
    /// The visible part of the tab grid (window pixels): the small bar windows of the grid stay inside it (they go in M6 of
    /// `docs/video_in_slint/HANDOFF.md`). `None` outside the grid.
    pub clip: Option<Rect>,
}

impl HostSurface {
    /// A frameless fullscreen window of `width` x `height` physical pixels.
    pub fn fullscreen(window: HostWindowId, parent_xid: u64, width: i32, height: i32, dpi: f32) -> Self {
        Self {
            mode: HostMode::Fullscreen,
            window,
            parent_xid,
            rect: Rect::new(0, 0, width.max(1), height.max(1)),
            dpi,
            clip: None,
        }
    }

    /// The media area of the active viewer tab inside the main window.
    pub fn tab_view(window: HostWindowId, parent_xid: u64, rect: Rect, dpi: f32) -> Self {
        Self {
            mode: HostMode::TabView,
            window,
            parent_xid,
            rect,
            dpi,
            clip: None,
        }
    }

    /// The canvas of a tab-grid item (Slint clips and scrolls the videos with the grid); `clip` is the visible part of the grid.
    pub fn tab_grid(window: HostWindowId, parent_xid: u64, rect: Rect, dpi: f32, clip: Rect) -> Self {
        Self {
            mode: HostMode::TabGrid,
            window,
            parent_xid,
            rect,
            dpi,
            clip: Some(clip),
        }
    }

    #[allow(
        clippy::float_cmp,
        reason = "the DPI is copied from the window, never computed; any difference means a re-layout"
    )]
    fn same_geometry(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.window == other.window
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
    /// A menu of the application is open (no bars meanwhile).
    pub menu_open: bool,
    /// A dialog is open over the host (no bars meanwhile).
    pub modal_locked: bool,
}

impl HostState {
    pub fn hidden(container: &Container) -> Self {
        Self {
            surface: HostSurface {
                mode: HostMode::Hidden,
                window: 0,
                parent_xid: 0,
                rect: container.rect(),
                dpi: container.dpi,
                clip: None,
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
            return false;
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
        self.attach_surfaces();
        if relayout {
            self.apply_layout();
        } else {
            self.reproject();
        }
        self.apply_player_flags();
        // A Multiview that was never shown (a tab opened in the background) starts with the first host that shows it. The
        // fullscreen starts its tiles itself, once its window exists.
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

    /// A menu of the application is open (or closed again): no bar is shown meanwhile.
    pub fn set_menu_open(&mut self, open: bool) {
        self.host.menu_open = open;
    }

    /// A dialog is open over the host (or closed again): no bar is shown meanwhile.
    pub fn set_modal_locked(&mut self, locked: bool) {
        self.host.modal_locked = locked;
    }

    /// The host rectangle of the medium of tile `index`.
    pub(super) fn host_content(&self, index: usize) -> Option<Rect> {
        self.layout
            .tiles
            .get(index)
            .map(|t| self.host.projection.rect(t.content))
    }

    /// Re-places everything after the layout or the projection changed.
    pub(super) fn reproject(&mut self) {
        self.sync_zoom_frames();
        self.size_surfaces();
        self.prune_feedback();
        self.bump();
    }

    /// Lets every video draw into the window of the host. A video that drew into another window (the fullscreen and the main
    /// window have GL contexts of their own) gets a new render context there; it keeps playing meanwhile.
    fn attach_surfaces(&mut self) {
        let window = self.host.surface.window;
        if window == 0 {
            return;
        }
        for tile in &mut self.tiles {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            if video.surface.host() != Some(window) {
                video.surface.attach(window, &video.player);
                tracing::debug!(
                    media_id = tile.media_id,
                    window,
                    "[Multiview: Host] Video draws in the window of the new host"
                );
            }
        }
    }

    /// Gives every video surface the size of the frame its video is drawn into (the room of the tile in the fullscreen and the
    /// tab view, else the medium's place); a frame that would be too small draws nothing.
    fn size_surfaces(&mut self) {
        let projection = self.host.projection;
        let placements: Vec<Option<VideoPlacement>> = (0..self.tiles.len()).map(|i| self.video_placement(i)).collect();
        for ((tile, placed), placement) in self.tiles.iter_mut().zip(&self.layout.tiles).zip(placements) {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            let placement = placement.unwrap_or_else(|| {
                let r = projection.rect(placed.content);
                VideoPlacement::new(r, r)
            });
            let frame = placement.frame;
            if placement.window.w < MULTIVIEW_MIN_VIDEO_WINDOW_PX || placement.window.h < MULTIVIEW_MIN_VIDEO_WINDOW_PX
            {
                video.surface.set_size(0, 0);
                continue;
            }
            video
                .surface
                .set_size(frame.w.saturating_as::<u32>(), frame.h.saturating_as::<u32>());
            note_placement(video, placement);
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

    /// Lets every video draw into `window` (the main window), paused, so the Multiview can live on as a background tab after its
    /// fullscreen window is gone. The host becomes `Hidden`; the next `set_host` places and resumes everything.
    pub fn park(&mut self, window: HostWindowId, parent_xid: u64, rect: Rect) {
        self.clear_hover();
        for tile in &mut self.tiles {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            video.surface.attach(window, &video.player);
            video.player.set_view_active(false);
            video.player.set_tab_grid_mode(false);
        }
        self.host.surface = HostSurface {
            mode: HostMode::Hidden,
            window,
            parent_xid,
            rect,
            dpi: self.host.surface.dpi,
            clip: None,
        };
        self.host.flags_mode = None;
        self.bump();
        tracing::info!(
            instance = self.id,
            players = self.tiles.iter().filter(|t| t.video.is_some()).count(),
            "[Multiview: Host] Parked in the main window"
        );
    }

    /// Pauses the players because another tab (or the Browse view) took over. The user's pause state is kept; `set_host` brings
    /// everything back.
    pub fn deactivate(&mut self) {
        if self.host.surface.mode == HostMode::Hidden {
            return;
        }
        self.clear_feedback();
        self.clear_hover();
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            video.player.set_view_active(false);
        }
        self.host.surface.mode = HostMode::Hidden;
        self.host.flags_mode = None;
        self.bump();
    }

    /// Stops playback because the tab was closed. The instance is destroyed by its last owner.
    pub fn stop_and_hide(&mut self) {
        self.clear_feedback();
        self.clear_hover();
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            video.player.stop();
        }
        self.host.surface.mode = HostMode::Hidden;
        self.host.flags_mode = None;
        self.bump();
    }

    /// Where the canvas draws the texture of the video of tile `index`, whose medium is at `rect` (host pixels).
    fn video_frame_view(&self, index: usize, rect: Rect) -> Option<VideoFrameView> {
        let video = self.tiles.get(index)?.video.as_ref()?;
        let row = video.surface.row_in(self.host.surface.window)?;
        let placement = video.placement.unwrap_or_else(|| VideoPlacement::new(rect, rect));
        let relative = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);
        Some(VideoFrameView {
            row,
            frame: relative(placement.frame),
            clip: relative(placement.window),
        })
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
                let rect = projection.rect(placed.content);
                TileView {
                    key: tile.key,
                    media_id: tile.media_id,
                    rect: relative(rect),
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
                    video: self.video_frame_view(index, rect),
                    volume: self.volume_bar_view(index, now),
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
