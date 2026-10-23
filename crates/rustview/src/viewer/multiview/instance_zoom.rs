//! Instance behavior around zooming images: the zoom of a tile (`TileState::zoom`), the slider that hangs below the bar,
//! the wheel, dragging, the minimap, and the decoding and rendering that zooming needs.
//!
//! The model lives in `viewer::zoom` (pure); this file connects it to tiles: it gives each zoom its tile size when the layout
//! changes, runs what `ZoomState::poll` asks for on worker threads (the events come back through the sink) and describes the
//! result for the host (`TileView::zoom`, `TileView::minimap`, `zoom_bubbles`).

use super::events::PlayerEvent;
use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::media_actions::BarClick;
use super::model::TileKind;
use super::route::HostMode;
use super::view::{MinimapView, ZoomBubbleView, ZoomContentView, ZoomPictureView};
use crate::viewer::HighResImageLoader;
use crate::viewer::zoom::geometry::{Area, IRect};
use crate::viewer::zoom::minimap::MinimapLayout;
use crate::viewer::zoom::prefs as zoom_prefs;
use crate::viewer::zoom::render::SourceImage;
use crate::viewer::zoom::scale;
use crate::viewer::zoom::state::{PictureContent, RenderDone, Request};
use std::sync::Arc;
use std::time::Instant;

/// Length of the track of the zoom slider in logical pixels (the bubble is 210 wide, the slider 202; the padding and the handle
/// take 30); the snapping of the handle measures its distances with it.
pub const SLIDER_TRACK_PX: f64 = 172.0;

/// A mouse wheel notch is worth this much `delta-y` in Slint; touchpads send pixels and add up to a step the same way.
const WHEEL_STEP_DELTA: f32 = 120.0;

impl MultiviewInstance {
    /// Zooming works where the picture is interactive: the fullscreen and the tab view. The tab-grid item is a miniature.
    fn zoom_host_is_interactive(&self) -> bool {
        matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView)
    }

    fn zoomable(&self, index: usize) -> bool {
        self.zoom_host_is_interactive()
            && self
                .tiles
                .get(index)
                .is_some_and(|t| t.kind == TileKind::Image && t.error.is_none() && !t.is_loading && t.zoom.is_some())
    }

    /// `Some(slider open)` for a tile whose picture can be zoomed; this is what the zoom button of the bar looks at.
    pub(super) fn zoom_button_state(&self, index: usize) -> Option<bool> {
        self.zoomable(index)
            .then(|| self.zoom_slider.contains(&self.tiles[index].key))
    }

    pub(super) fn toggle_zoom_slider(&mut self, index: usize) {
        let Some(key) = self.tiles.get(index).map(|t| t.key) else {
            return;
        };
        let open = !self.zoom_slider.contains(&key);
        if open {
            self.zoom_slider.push(key);
        } else {
            self.zoom_slider.retain(|k| *k != key);
        }
        tracing::info!(open, "[Zoom: Slider] Toggled");
        self.bump();
    }

    /// The slider asked for something: a zoom, a step, the reset. The tile that has the slider is the one that zooms.
    pub(super) fn zoom_slider_click(&mut self, key: TileKey, click: BarClick) {
        let Some(index) = self.tile_index(key) else { return };
        if !self.zoomable(index) {
            return;
        }
        let prefs = zoom_prefs::current();
        let now = Instant::now();
        let Some(zoom) = self.tiles[index].zoom.as_mut() else {
            return;
        };
        match click {
            BarClick::ZoomTo(permille) => zoom.zoom_to(scale::from_permille(permille), None, &prefs, now),
            BarClick::ZoomPos(position) => {
                // The handle: pulled to the snap points that are close (the track is `SLIDER_TRACK_PX` long), then a zoom.
                let limits = prefs.limits();
                let p = scale::snap_position(
                    f64::from(position) / f64::from(BarClick::MAX_ZOOM_POS),
                    SLIDER_TRACK_PX,
                    limits,
                );
                zoom.zoom_to(scale::zoom_at(p, limits), None, &prefs, now);
            }
            BarClick::ZoomStep { up, shift, ctrl } => {
                zoom.zoom_step(up, scale::slider_wheel_step(shift, ctrl), None, &prefs, now);
            }
            BarClick::ZoomReset => zoom.reset(&prefs, now),
            BarClick::Action(_) | BarClick::MenuItem(_) => return,
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// What the slider bubble shows: the tile it belongs to and its numbers; `None` while it is closed (or its tile can no
    /// longer be zoomed).
    pub fn zoom_bubbles(&self) -> Vec<ZoomBubbleView> {
        self.zoom_slider
            .iter()
            .filter_map(|key| self.tile_index(*key))
            .filter_map(|index| self.bubble_for(index))
            .collect()
    }

    /// The numbers a zoom slider of tile `index` shows (the slider of the bar and the one on the minimap are the same view).
    fn bubble_for(&self, index: usize) -> Option<ZoomBubbleView> {
        if !self.zoomable(index) {
            return None;
        }
        let limits = zoom_prefs::current().limits();
        let zoom = self.tiles[index].zoom.as_ref()?;
        Some(ZoomBubbleView {
            tile: index,
            key: self.tiles[index].key,
            permille: scale::to_permille(zoom.zoom()),
            min_permille: scale::to_permille(limits.min),
            max_permille: scale::to_permille(limits.max),
            snaps: scale::ticks(limits, SLIDER_TRACK_PX)
                .into_iter()
                .map(|t| scale::to_permille(t.zoom) * 4 + i32::from(t.rank))
                .collect(),
        })
    }

    /// Closes the slider if its tile is gone or cannot be zoomed any more (a tick checks it).
    fn validate_zoom_slider(&mut self) {
        let valid = |this: &Self, key: &TileKey| this.tile_index(*key).is_some_and(|index| this.zoomable(index));
        let (slider, bubble) = (self.zoom_slider.len(), self.zoom_minimap_bubble.len());
        let (sliders, bubbles): (Vec<TileKey>, Vec<TileKey>) = (
            self.zoom_slider.iter().copied().filter(|k| valid(self, k)).collect(),
            self.zoom_minimap_bubble
                .iter()
                .copied()
                .filter(|k| valid(self, k))
                .collect(),
        );
        if sliders.len() != slider || bubbles.len() != bubble {
            self.zoom_slider = sliders;
            self.zoom_minimap_bubble = bubbles;
            self.bump();
        }
    }

    // ---- input ------------------------------------------------------------------------------------------------------------

    fn logical_to_physical(&self, value: f32) -> f64 {
        f64::from(value) * f64::from(self.host.surface.dpi.max(0.5))
    }

    /// Ctrl + mouse wheel over tile `index`: one step up (wheel up, `delta_y > 0`) or down per notch, around the pointer
    /// (`x`, `y`: logical pixels in the tile).
    pub fn zoom_wheel(&mut self, index: usize, delta_y: f32, x: f32, y: f32) {
        self.zoom_wheel_at(index, delta_y, (x, y), Instant::now());
    }

    pub(super) fn zoom_wheel_at(&mut self, index: usize, delta_y: f32, at: (f32, f32), now: Instant) {
        if !self.zoomable(index) || !delta_y.is_finite() || delta_y == 0.0 {
            return;
        }
        // Add up to whole notches; a change of direction starts over.
        if self.zoom_wheel_accum * delta_y < 0.0 {
            self.zoom_wheel_accum = 0.0;
        }
        self.zoom_wheel_accum += delta_y;
        let steps = (self.zoom_wheel_accum.abs() / WHEEL_STEP_DELTA).floor() as u32;
        if steps == 0 {
            return;
        }
        let up = self.zoom_wheel_accum > 0.0;
        self.zoom_wheel_accum -= self.zoom_wheel_accum.signum() * WHEEL_STEP_DELTA * steps as f32;
        let mut anchor = (self.logical_to_physical(at.0), self.logical_to_physical(at.1));
        let prefs = zoom_prefs::current();
        let Some(zoom) = self.tiles[index].zoom.as_mut() else {
            return;
        };
        // The pointer is reported inside the room while the picture is zoomed, else inside the tile.
        if let (false, Some(home)) = (zoom.is_active(), zoom.home()) {
            anchor = (anchor.0 + f64::from(home.x), anchor.1 + f64::from(home.y));
        }
        for _ in 0..steps.min(8) {
            zoom.zoom_step(up, prefs.wheel_step, Some(anchor), &prefs, now);
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// A drag of the picture of tile `index` by `dx`, `dy` logical pixels.
    pub fn zoom_pan(&mut self, index: usize, dx: f32, dy: f32) {
        if !self.zoomable(index) {
            return;
        }
        let (dx, dy) = (self.logical_to_physical(dx), self.logical_to_physical(dy));
        let (prefs, now) = (zoom_prefs::current(), Instant::now());
        if let Some(zoom) = self.tiles[index].zoom.as_mut() {
            zoom.pan_pixels(dx, dy, &prefs, now);
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// The minimap of tile `index` was pressed (`phase` 0), dragged (1) or released (2) at (`nx`, `ny`), shares of the image;
    /// a middle click (3) resets the zoom at once, a right click (4) opens or closes the slider bubble on the minimap.
    pub fn zoom_minimap(&mut self, index: usize, nx: f32, ny: f32, phase: i32) {
        if !self.zoomable(index) {
            return;
        }
        if phase == 4 {
            let key = self.tiles[index].key;
            let open = !self.zoom_minimap_bubble.contains(&key);
            if open {
                self.zoom_minimap_bubble.push(key);
            } else {
                self.zoom_minimap_bubble.retain(|k| *k != key);
            }
            tracing::info!(open, "[Zoom: Minimap] Slider bubble toggled");
            self.poll_zoom_at(Instant::now());
            self.bump();
            return;
        }
        if phase == 3 {
            let (prefs, now) = (zoom_prefs::current(), Instant::now());
            if let Some(zoom) = self.tiles[index].zoom.as_mut() {
                zoom.reset_now(&prefs, now);
            }
            self.poll_zoom_at(now);
            self.bump();
            return;
        }
        let (nx, ny) = (f64::from(nx).clamp(0.0, 1.0), f64::from(ny).clamp(0.0, 1.0));
        let (prefs, now) = (zoom_prefs::current(), Instant::now());
        if let Some(zoom) = self.tiles[index].zoom.as_mut() {
            match phase {
                0 => zoom.minimap_press(nx, ny, &prefs, now),
                1 => zoom.minimap_move(nx, ny, &prefs, now),
                _ => zoom.minimap_release(),
            }
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// The pointer, locked to the minimap of tile `index` while it is dragged, moved by (`dx`, `dy`) device pixels.
    pub fn zoom_minimap_delta(&mut self, index: usize, dx: i32, dy: i32) {
        if !self.zoomable(index) {
            return;
        }
        let mm_px = self.minimap_px(index);
        let (prefs, now) = (zoom_prefs::current(), Instant::now());
        if let Some(zoom) = self.tiles[index].zoom.as_mut() {
            zoom.minimap_drag_by(f64::from(dx), f64::from(dy), mm_px, &prefs, now);
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// The slider bubble of the minimap of tile `index` asked for something (the same codes as the slider of the bar).
    pub fn zoom_minimap_slider(&mut self, index: usize, code: i32) {
        let Some(key) = self.tiles.get(index).map(|t| t.key) else {
            return;
        };
        if let Some(click) = BarClick::from_code(code) {
            self.zoom_slider_click(key, click);
        }
    }

    /// Back to 100% for tile `index`.
    pub fn zoom_reset(&mut self, index: usize) {
        if !self.zoomable(index) {
            return;
        }
        let (prefs, now) = (zoom_prefs::current(), Instant::now());
        if let Some(zoom) = self.tiles[index].zoom.as_mut() {
            zoom.reset(&prefs, now);
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// Ctrl+0 (0), Ctrl++ (1) and Ctrl+- (2): the image under the pointer, else the only image of the Multiview.
    pub fn zoom_key(&mut self, key: i32) {
        let index = self
            .hovered
            .filter(|&i| self.zoomable(i))
            .or_else(|| (self.tiles.len() == 1 && self.zoomable(0)).then_some(0));
        let Some(index) = index else { return };
        let (prefs, now) = (zoom_prefs::current(), Instant::now());
        if let Some(zoom) = self.tiles[index].zoom.as_mut() {
            match key {
                0 => zoom.reset(&prefs, now),
                1 => zoom.zoom_step(true, prefs.wheel_step, None, &prefs, now),
                2 => zoom.zoom_step(false, prefs.wheel_step, None, &prefs, now),
                _ => return,
            }
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// The zoom of tile `index` in percent (for the status), if it can be zoomed.
    pub fn zoom_percent(&self, index: usize) -> Option<f64> {
        self.tiles.get(index)?.zoom.as_ref().map(|z| z.zoom() * 100.0)
    }

    /// Whether any picture is gliding to a new zoom (the host redraws every frame meanwhile).
    pub fn zoom_animating(&self) -> bool {
        self.tiles.iter().any(|t| {
            t.zoom
                .as_ref()
                .is_some_and(super::super::zoom::state::ZoomState::animating)
        })
    }

    // ---- layout, time and work ------------------------------------------------------------------------------------------------

    /// The room tile `index` may zoom in, in host pixels: its column (the full height of the row), and for the first and the last
    /// tile of a row also the free space the layout leaves at the edge of the container, so a picture that is narrower than the
    /// screen can grow into all of it. Never more than the column of a neighbor.
    pub(super) fn host_room(&self, index: usize) -> Option<Rect> {
        let layout = self.layout.tiles.get(index)?;
        let container = self.solving_container().rect();
        let mut area = layout.column;
        if let Some(row) = self.layout.rows.iter().find(|row| row.contains(&index)) {
            if row.first() == Some(&index) {
                area.w += area.x - container.x;
                area.x = container.x;
            }
            if row.last() == Some(&index) {
                area.w = container.x + container.w - area.x;
            }
        }
        // The fitted picture is always inside its room.
        let (x0, y0) = (area.x.min(layout.content.x), area.y.min(layout.content.y));
        let x1 = (area.x + area.w).max(layout.content.x + layout.content.w);
        let y1 = (area.y + area.h).max(layout.content.y + layout.content.h);
        Some(self.host.projection.rect(Rect::new(x0, y0, x1 - x0, y1 - y0)))
    }

    /// The rectangle that counts as tile `index` for the pointer and the bar: its room while its picture is zoomed (it fills it),
    /// else the place of the medium.
    pub(super) fn host_tile_rect(&self, index: usize) -> Option<Rect> {
        let zoomed = self
            .tiles
            .get(index)?
            .zoom
            .as_ref()
            .is_some_and(super::super::zoom::state::ZoomState::is_active);
        if zoomed {
            self.host_room(index)
        } else {
            self.host_content(index)
        }
    }

    /// Gives every zoom the room of its tile (called after the layout or the projection changed).
    pub(super) fn sync_zoom_frames(&mut self) {
        if !self.zoom_host_is_interactive() {
            return;
        }
        let areas: Vec<Option<Area>> = (0..self.tiles.len())
            .map(|i| {
                let (room, home) = (self.host_room(i)?, self.host_content(i)?);
                Some(Area::new(
                    (room.w, room.h),
                    IRect {
                        x: home.x - room.x,
                        y: home.y - room.y,
                        w: home.w,
                        h: home.h,
                    },
                ))
            })
            .collect();
        let prefs = zoom_prefs::current();
        for (tile, area) in self.tiles.iter_mut().zip(areas) {
            if let (Some(zoom), Some(area)) = (tile.zoom.as_mut(), area) {
                zoom.set_area(area, &prefs);
            }
        }
    }

    /// Everything time driven: the glide, the render debounce, the minimap's hover and timeout, the slider's validity. Called
    /// on every tick of the host (and by the host's frame timer while a zoom glides).
    pub fn poll_zoom(&mut self) {
        self.poll_zoom_at(Instant::now());
    }

    pub(super) fn poll_zoom_at(&mut self, now: Instant) {
        if !self.zoom_host_is_interactive() {
            return;
        }
        self.validate_zoom_slider();
        self.ensure_zoom_thumbs();
        let prefs = zoom_prefs::current();
        let dpi = self.host.surface.dpi;
        let pointer = self.pointer;
        let rects: Vec<Option<Rect>> = (0..self.tiles.len()).map(|i| self.host_tile_rect(i)).collect();
        let bubbles = self.zoom_minimap_bubble.clone();
        let mut changed = false;
        let mut work: Vec<(TileKey, std::path::PathBuf, Vec<Request>)> = Vec::new();
        for (index, tile) in self.tiles.iter_mut().enumerate() {
            let Some(zoom) = tile.zoom.as_mut() else { continue };
            // The minimap is revealed by the pointer on or around its place.
            let hover = match (pointer, rects[index]) {
                (Some((px, py)), Some(rect)) if zoom.can_pan() => {
                    let (tw, th) = (rect.w, rect.h);
                    MinimapLayout::compute((tw, th), zoom.image(), prefs.minimap_corner, dpi)
                        .is_some_and(|m| m.reveals(f64::from(px - rect.x), f64::from(py - rect.y)))
                }
                _ => false,
            };
            zoom.set_minimap_hover(hover);
            zoom.set_minimap_pinned(bubbles.contains(&tile.key));
            let was_animating = zoom.animating();
            let requests = zoom.poll(tile.pixels.as_ref(), &prefs, now);
            changed |= was_animating || zoom.animating();
            let shown = zoom.minimap_shown(&prefs, now);
            if shown != tile.zoom_minimap_shown {
                tile.zoom_minimap_shown = shown;
                changed = true;
            }
            if !requests.is_empty() {
                work.push((tile.key, tile.path.clone(), requests));
            }
        }
        for (key, path, requests) in work {
            for request in requests {
                self.run_zoom_request(key, &path, request);
            }
        }
        if changed {
            self.bump();
        }
    }

    fn run_zoom_request(&self, key: TileKey, path: &std::path::Path, request: Request) {
        let (sink, me) = (self.sink.clone(), self.me.clone());
        match request {
            Request::LoadSource => {
                let path = path.to_path_buf();
                let spawned = std::thread::Builder::new().name("zoom-source".into()).spawn(move || {
                    let result = HighResImageLoader::load_full_resolution(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|(w, h, pixels)| {
                            SourceImage::new(w, h, pixels)
                                .map(Arc::new)
                                .ok_or_else(|| "the original has an unexpected size".to_string())
                        });
                    sink.post(me, PlayerEvent::ZoomSource { tile: key, result });
                });
                if let Err(e) = spawned {
                    tracing::error!(error = %e, "[Zoom: Source] The decoder thread could not be started");
                }
            }
            Request::Render(job) => {
                let spawned = std::thread::Builder::new().name("zoom-render".into()).spawn(move || {
                    let done = job.run();
                    sink.post(me, PlayerEvent::ZoomRendered { tile: key, done });
                });
                if let Err(e) = spawned {
                    tracing::error!(error = %e, "[Zoom: Render] The render thread could not be started");
                }
            }
        }
    }

    /// The original of a zoomed image arrived (or could not be decoded).
    pub fn on_zoom_source(&mut self, key: TileKey, result: Result<Arc<SourceImage>, String>) {
        let Some(index) = self.tile_index(key) else { return };
        if let Some(zoom) = self.tiles[index].zoom.as_mut() {
            if let Err(why) = &result {
                tracing::warn!(error = %why, "[Zoom: Source] The original could not be decoded; the zoom stays a preview");
            }
            zoom.source_loaded(result);
        }
        self.poll_zoom_at(Instant::now());
        self.bump();
    }

    /// A zoomed picture was resampled.
    pub fn on_zoom_rendered(&mut self, key: TileKey, done: RenderDone) {
        let Some(index) = self.tile_index(key) else { return };
        let prefs = zoom_prefs::current();
        if let Some(zoom) = self.tiles[index].zoom.as_mut() {
            zoom.rendered(done, &prefs);
        }
        self.poll_zoom_at(Instant::now());
        self.bump();
    }

    // ---- what the host draws ----------------------------------------------------------------------------------------------

    /// The zoomed picture, the minimap and the pannable flag of tile `index` for `view()`.
    pub(super) fn zoom_views(&self, index: usize) -> (Option<ZoomPictureView>, Option<MinimapView>, bool) {
        if !self.zoomable(index) {
            return (None, None, false);
        }
        let tile = &self.tiles[index];
        let Some(zoom) = tile.zoom.as_ref() else {
            return (None, None, false);
        };
        let prefs = zoom_prefs::current();
        let display_size = tile.pixels.as_ref().map(|p| (p.width(), p.height()));
        let area = self.zoom_area_in_tile(index).unwrap_or_default();
        let picture = zoom.picture(display_size, &prefs).map(|picture| ZoomPictureView {
            area,
            dest: Rect::new(picture.dest.x, picture.dest.y, picture.dest.w, picture.dest.h),
            content: match picture.content {
                PictureContent::Rendered { pixels, clip } => ZoomContentView::Rendered {
                    pixels,
                    clip: Rect::new(clip.x, clip.y, clip.w, clip.h),
                },
                PictureContent::Preview { clip } => ZoomContentView::Preview {
                    clip: Rect::new(clip.x, clip.y, clip.w, clip.h),
                },
            },
            pixelated: prefs.resample.is_pixelated(),
        });
        let minimap = self.minimap_view(index, &prefs);
        (picture, minimap, zoom.can_pan())
    }

    /// The room of tile `index` relative to the top left corner of the tile (so it starts at zero or below).
    fn zoom_area_in_tile(&self, index: usize) -> Option<Rect> {
        let (room, content) = (self.host_room(index)?, self.host_content(index)?);
        Some(Rect::new(room.x - content.x, room.y - content.y, room.w, room.h))
    }

    /// The size of the minimap of tile `index` in pixels (zero while it has none).
    fn minimap_px(&self, index: usize) -> (f64, f64) {
        let layout = self
            .zoom_area_in_tile(index)
            .zip(self.tiles.get(index).and_then(|t| t.zoom.as_ref()))
            .and_then(|(area, zoom)| {
                MinimapLayout::compute(
                    (area.w, area.h),
                    zoom.image(),
                    zoom_prefs::current().minimap_corner,
                    self.host.surface.dpi,
                )
            });
        layout.map_or((0.0, 0.0), |l| (f64::from(l.rect.w), f64::from(l.rect.h)))
    }

    fn minimap_view(&self, index: usize, prefs: &zoom_prefs::ZoomPrefs) -> Option<MinimapView> {
        let tile = &self.tiles[index];
        let zoom = tile.zoom.as_ref()?;
        // A minimap that carries the slider bubble stays, even at 100% (the user may be dragging the handle through it).
        let pinned = zoom.minimap_pinned();
        if !pinned && (!zoom.is_active() || !zoom.can_pan()) {
            return None;
        }
        let visible = zoom.minimap_shown(prefs, Instant::now());
        let area = self.zoom_area_in_tile(index)?;
        let layout = MinimapLayout::compute(
            (area.w, area.h),
            zoom.image(),
            prefs.minimap_corner,
            self.host.surface.dpi,
        )?;
        let thumb = zoom.thumb()?.clone();
        let viewport = zoom.viewport_norm()?;
        Some(MinimapView {
            visible,
            area,
            rect: Rect::new(
                area.x + layout.rect.x,
                area.y + layout.rect.y,
                layout.rect.w,
                layout.rect.h,
            ),
            thumb,
            viewport: (viewport.x, viewport.y, viewport.w, viewport.h),
            bubble: if pinned { self.bubble_for(index) } else { None },
        })
    }

    /// Makes the thumbnails of the minimaps that are about to be shown (once per image, from the tile's own picture).
    pub(super) fn ensure_zoom_thumbs(&mut self) {
        for tile in &mut self.tiles {
            if let (Some(zoom), Some(pixels)) = (tile.zoom.as_mut(), tile.pixels.as_ref())
                && (zoom.is_active() || zoom.minimap_pinned())
            {
                zoom.ensure_thumb(pixels);
            }
        }
    }
}
