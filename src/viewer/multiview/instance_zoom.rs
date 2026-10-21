//! Instance behavior around zooming images: the zoom of a tile (`TileState::zoom`), the slider that hangs below the bar,
//! the wheel, dragging, the minimap, and the decoding and rendering that zooming needs.
//!
//! The model lives in `viewer::zoom` (pure); this file connects it to tiles: it gives each zoom its tile size when the layout
//! changes, runs what `ZoomState::poll` asks for on worker threads (the events come back through the sink) and describes the
//! result for the host (`TileView::zoom`, `TileView::minimap`, `zoom_bubble`).

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
        self.zoomable(index).then(|| self.zoom_slider == Some(self.tiles[index].key))
    }

    pub(super) fn toggle_zoom_slider(&mut self, index: usize) {
        let Some(key) = self.tiles.get(index).map(|t| t.key) else { return };
        self.zoom_slider = if self.zoom_slider == Some(key) { None } else { Some(key) };
        tracing::info!(open = self.zoom_slider.is_some(), "[Zoom: Slider] Toggled");
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
        let Some(zoom) = self.tiles[index].zoom.as_mut() else { return };
        match click {
            BarClick::ZoomTo(permille) => zoom.zoom_to(scale::from_permille(permille), None, &prefs, now),
            BarClick::ZoomPos(position) => {
                // The handle: pulled to the snap points that are close (the track is `SLIDER_TRACK_PX` long), then a zoom.
                let limits = prefs.limits();
                let p = scale::snap_position(f64::from(position) / f64::from(BarClick::MAX_ZOOM_POS), SLIDER_TRACK_PX, limits);
                zoom.zoom_to(scale::zoom_at(p, limits), None, &prefs, now);
            }
            BarClick::ZoomStep { up, shift, ctrl } => zoom.zoom_step(up, scale::slider_wheel_step(shift, ctrl), None, &prefs, now),
            BarClick::ZoomReset => zoom.reset(&prefs, now),
            BarClick::Action(_) | BarClick::MenuItem(_) => return,
        }
        self.poll_zoom_at(now);
        self.bump();
    }

    /// What the slider bubble shows: the tile it belongs to and its numbers; `None` while it is closed (or its tile can no
    /// longer be zoomed).
    pub fn zoom_bubble(&self) -> Option<ZoomBubbleView> {
        let index = self.tile_index(self.zoom_slider?)?;
        self.bubble_for(index)
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
            permille: scale::to_permille(zoom.zoom()),
            min_permille: scale::to_permille(limits.min),
            max_permille: scale::to_permille(limits.max),
            snaps: scale::ticks(limits, SLIDER_TRACK_PX).into_iter().map(|t| scale::to_permille(t.zoom) * 4 + i32::from(t.rank)).collect(),
        })
    }

    /// Closes the slider if its tile is gone or cannot be zoomed any more (a tick checks it).
    fn validate_zoom_slider(&mut self) {
        if let Some(key) = self.zoom_slider
            && self.tile_index(key).is_none_or(|index| !self.zoomable(index))
        {
            self.zoom_slider = None;
            self.bump();
        }
        if let Some(key) = self.zoom_minimap_bubble
            && self.tile_index(key).is_none_or(|index| !self.zoomable(index))
        {
            self.zoom_minimap_bubble = None;
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
        let Some(zoom) = self.tiles[index].zoom.as_mut() else { return };
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
            self.zoom_minimap_bubble = if self.zoom_minimap_bubble == Some(key) { None } else { Some(key) };
            tracing::info!(open = self.zoom_minimap_bubble.is_some(), "[Zoom: Minimap] Slider bubble toggled");
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

    /// The slider bubble of the minimap of tile `index` asked for something (the same codes as the slider of the bar).
    pub fn zoom_minimap_slider(&mut self, index: usize, code: i32) {
        let Some(key) = self.tiles.get(index).map(|t| t.key) else { return };
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
        let index = self.hovered.filter(|&i| self.zoomable(i)).or_else(|| (self.tiles.len() == 1 && self.zoomable(0)).then_some(0));
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
        self.tiles.iter().any(|t| t.zoom.as_ref().is_some_and(|z| z.animating()))
    }

    // ---- layout, time and work ------------------------------------------------------------------------------------------------

    /// The room tile `index` may zoom in, in host pixels: its column (the full height of the row), and for the first and the last
    /// tile of a row also the free space the layout leaves at the edge of the container, so a picture that is narrower than the
    /// screen can grow into all of it. Never more than the column of a neighbor.
    pub(super) fn host_zoom_area(&self, index: usize) -> Option<Rect> {
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
        let zoomed = self.tiles.get(index)?.zoom.as_ref().is_some_and(|z| z.is_active());
        if zoomed { self.host_zoom_area(index) } else { self.host_content(index) }
    }

    /// Gives every zoom the room of its tile (called after the layout or the projection changed).
    pub(super) fn sync_zoom_frames(&mut self) {
        if !self.zoom_host_is_interactive() {
            return;
        }
        let areas: Vec<Option<Area>> = (0..self.tiles.len())
            .map(|i| {
                let (room, home) = (self.host_zoom_area(i)?, self.host_content(i)?);
                Some(Area::new((room.w, room.h), IRect { x: home.x - room.x, y: home.y - room.y, w: home.w, h: home.h }))
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
        let bubble = self.zoom_minimap_bubble;
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
            zoom.set_minimap_pinned(bubble == Some(tile.key));
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
                        .and_then(|(w, h, pixels)| SourceImage::new(w, h, pixels).map(Arc::new).ok_or_else(|| "the original has an unexpected size".to_string()));
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
        let Some(zoom) = tile.zoom.as_ref() else { return (None, None, false) };
        let prefs = zoom_prefs::current();
        let display_size = tile.pixels.as_ref().map(|p| (p.width(), p.height()));
        let area = self.zoom_area_in_tile(index).unwrap_or_default();
        let picture = zoom.picture(display_size, &prefs).map(|picture| ZoomPictureView {
            area,
            dest: Rect::new(picture.dest.x, picture.dest.y, picture.dest.w, picture.dest.h),
            content: match picture.content {
                PictureContent::Rendered { pixels, clip } => {
                    ZoomContentView::Rendered { pixels, clip: Rect::new(clip.x, clip.y, clip.w, clip.h) }
                }
                PictureContent::Preview { clip } => ZoomContentView::Preview { clip: Rect::new(clip.x, clip.y, clip.w, clip.h) },
            },
            pixelated: prefs.resample.is_pixelated(),
        });
        let minimap = self.minimap_view(index, &prefs);
        (picture, minimap, zoom.can_pan())
    }

    /// The room of tile `index` relative to the top left corner of the tile (so it starts at zero or below).
    fn zoom_area_in_tile(&self, index: usize) -> Option<Rect> {
        let (room, content) = (self.host_zoom_area(index)?, self.host_content(index)?);
        Some(Rect::new(room.x - content.x, room.y - content.y, room.w, room.h))
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
        let layout = MinimapLayout::compute((area.w, area.h), zoom.image(), prefs.minimap_corner, self.host.surface.dpi)?;
        let thumb = zoom.thumb()?.clone();
        let viewport = zoom.viewport_norm()?;
        Some(MinimapView {
            visible,
            area,
            rect: Rect::new(area.x + layout.rect.x, area.y + layout.rect.y, layout.rect.w, layout.rect.h),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
    use crate::viewer::multiview::instance_host::HostSurface;
    use crate::viewer::multiview::tile::TileSeed;
    use crate::viewer::zoom::minimap::MinimapLayout;
    use slint::{Rgb8Pixel, SharedPixelBuffer};
    use std::path::PathBuf;
    use std::time::Duration;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
    }

    const AREA: Rect = Rect::new(100, 50, 1200, 900);

    fn image_seed(id: u64) -> TileSeed {
        let mut seed = TileSeed::new(id, TileKind::Image, 4.0 / 3.0, PathBuf::from(format!("/x/{id}.jpg")), format!("{id}.jpg"));
        seed.pixels = Some(SharedPixelBuffer::<Rgb8Pixel>::new(1200, 900));
        seed.orig_size = Some((4000, 3000));
        seed
    }

    fn video_seed(id: u64) -> TileSeed {
        TileSeed::new(id, TileKind::Video, 16.0 / 9.0, PathBuf::from(format!("/x/{id}.mp4")), format!("{id}.mp4"))
    }

    /// The preferences the tests expect (the global is shared by all tests, so every one sets the same).
    fn test_prefs() {
        let mut settings = crate::settings::schema::FullscreenSettings::default();
        settings.zoom.smooth = false;
        settings.zoom.step_percent = 10;
        settings.zoom.out_percent = 900;
        settings.zoom.minimap_secs = 3;
        zoom_prefs::apply(&settings);
    }

    fn instance(seeds: Vec<TileSeed>, surface: HostSurface) -> SharedInstance {
        test_prefs();
        let instance = MultiviewInstance::create(Arc::new(NullSink), seeds);
        instance.lock().set_host(&surface);
        instance
    }

    fn tab_view() -> HostSurface {
        HostSurface::tab_view(1, AREA, 1.0, Vec::new())
    }

    fn first_tile(instance: &SharedInstance) -> crate::viewer::multiview::view::TileView {
        instance.lock().view().tiles.into_iter().next().unwrap()
    }

    #[test]
    fn the_zoom_button_exists_for_images_that_are_shown_in_an_interactive_host() {
        let tab = instance(vec![image_seed(1)], tab_view());
        assert_eq!(tab.lock().zoom_button_state(0), Some(false));
        let fullscreen = instance(vec![image_seed(1)], HostSurface::fullscreen(1, 1600, 900, 1.0));
        assert_eq!(fullscreen.lock().zoom_button_state(0), Some(false));
        let grid = instance(
            vec![image_seed(1)],
            HostSurface::tab_grid(1, Rect::new(0, 0, 400, 300), 1.0, Rect::new(0, 0, 400, 300), Vec::new()),
        );
        assert_eq!(grid.lock().zoom_button_state(0), None, "the miniature is a picture");
        let video = instance(vec![video_seed(2)], tab_view());
        assert_eq!(video.lock().zoom_button_state(0), None, "mpv zooms videos itself");
        let loading = instance(vec![TileSeed::new(3, TileKind::Image, 1.0, PathBuf::from("/x/3.jpg"), "3.jpg".into())], tab_view());
        assert_eq!(loading.lock().zoom_button_state(0), None, "not before the picture is there");
    }

    #[test]
    fn the_slider_toggles_and_closes_by_itself_when_it_cannot_belong_to_its_tile_any_more() {
        let instance = instance(vec![image_seed(1)], tab_view());
        assert!(instance.lock().zoom_bubble().is_none());
        instance.lock().toggle_zoom_slider(0);
        let bubble = instance.lock().zoom_bubble().unwrap();
        assert_eq!((bubble.tile, bubble.permille, bubble.min_permille, bubble.max_permille), (0, 1000, 100, 10_000));
        let marks: Vec<i32> = bubble.snaps.iter().map(|tick| tick / 4).collect();
        assert!(marks.contains(&1000) && marks.contains(&500) && marks.contains(&2000), "{marks:?}");
        assert_eq!(instance.lock().zoom_button_state(0), Some(true));
        instance.lock().toggle_zoom_slider(0);
        assert!(instance.lock().zoom_bubble().is_none());
        // Open, then the host becomes the tab grid: it closes.
        instance.lock().toggle_zoom_slider(0);
        instance.lock().set_host(&HostSurface::tab_grid(1, Rect::new(0, 0, 400, 300), 1.0, Rect::new(0, 0, 400, 300), Vec::new()));
        assert!(instance.lock().zoom_bubble().is_none());
    }

    #[test]
    fn slider_clicks_zoom_the_picture_and_the_view_describes_it() {
        let instance = instance(vec![image_seed(1)], tab_view());
        let key = {
            let mut guard = instance.lock();
            guard.toggle_zoom_slider(0);
            guard.tiles[0].key
        };
        assert!(first_tile(&instance).zoom.is_none() && !first_tile(&instance).pannable);
        instance.lock().perform_click(key, BarClick::ZoomTo(2000));
        let tile = first_tile(&instance);
        let zoom = tile.zoom.expect("zoomed");
        assert!(tile.pannable);
        // The tile is the whole area (a lone image), and the zoomed picture covers all of it.
        assert_eq!((zoom.dest.w, zoom.dest.h), (1200, 900));
        assert!(matches!(zoom.content, ZoomContentView::Preview { .. }), "nothing is rendered in a test");
        assert_eq!(instance.lock().zoom_bubble().unwrap().permille, 2000);
        // A step up with the slider's 5%, one with Ctrl 25%.
        instance.lock().perform_click(key, BarClick::ZoomStep { up: true, shift: false, ctrl: false });
        assert!(instance.lock().zoom_bubble().unwrap().permille > 2000);
        instance.lock().perform_click(key, BarClick::ZoomReset);
        assert!(first_tile(&instance).zoom.is_none());
        assert_eq!(instance.lock().zoom_bubble().unwrap().permille, 1000);
        // Beyond the limits is held at the limits.
        instance.lock().perform_click(key, BarClick::ZoomTo(900_000));
        assert_eq!(instance.lock().zoom_bubble().unwrap().permille, 10_000);
        instance.lock().perform_click(key, BarClick::ZoomTo(1));
        assert_eq!(instance.lock().zoom_bubble().unwrap().permille, 100);
    }

    #[test]
    fn a_portrait_picture_in_the_fullscreen_zooms_into_the_free_room_beside_it() {
        let mut seed = image_seed(1);
        seed.aspect = 9.0 / 16.0;
        seed.orig_size = Some((900, 1600));
        let instance = instance(vec![seed], HostSurface::fullscreen(1, 1600, 900, 1.0));
        let (room, content) = {
            let guard = instance.lock();
            (guard.host_zoom_area(0).unwrap(), guard.host_content(0).unwrap())
        };
        assert_eq!((room.x, room.w, room.h), (0, 1600, 900), "a lone picture may use the whole screen: {room:?}");
        assert!(content.w < 600, "the fit is narrow: {content:?}");
        // 400% is wider than the screen: the picture fills its whole width.
        let key = instance.lock().tiles[0].key;
        instance.lock().perform_click(key, BarClick::ZoomTo(4000));
        let tile = first_tile(&instance);
        let picture = tile.zoom.expect("zoomed");
        assert_eq!((picture.dest.x, picture.dest.w), (0, 1600), "{:?}", picture.dest);
        assert_eq!(picture.area.w, 1600);
        // The bar stays at the corner of the medium itself, zoomed or not.
        instance.lock().poll_pointer(Some((content.x + content.w - 10, content.y + 10)));
        assert_eq!(instance.lock().media_bar_target().map(|t| t.rect), Some(content));
        // Pointer and hover count the room as the tile.
        let inside_room_only = (room.x + 20, room.y + 450);
        assert!(!content.contains(inside_room_only.0, inside_room_only.1));
        instance.lock().poll_pointer(Some(inside_room_only));
        assert_eq!(instance.lock().hovered, Some(0));
    }

    #[test]
    fn two_pictures_keep_to_their_own_columns_and_use_the_outer_edges() {
        let mut a = image_seed(1);
        a.aspect = 9.0 / 16.0;
        a.orig_size = Some((900, 1600));
        let mut b = image_seed(2);
        b.aspect = 9.0 / 16.0;
        b.orig_size = Some((900, 1600));
        let instance = instance(vec![a, b], HostSurface::fullscreen(1, 1600, 900, 1.0));
        let guard = instance.lock();
        let (first, second) = (guard.host_zoom_area(0).unwrap(), guard.host_zoom_area(1).unwrap());
        assert_eq!(first.x, 0);
        assert_eq!(second.x + second.w, 1600);
        assert!(first.x + first.w <= second.x, "the rooms do not overlap: {first:?} {second:?}");
    }

    #[test]
    fn a_right_click_on_the_minimap_opens_its_slider_bubble_and_a_middle_click_resets_at_once() {
        let instance = instance(vec![image_seed(1)], tab_view());
        let key = instance.lock().tiles[0].key;
        instance.lock().perform_click(key, BarClick::ZoomTo(3000));
        assert!(first_tile(&instance).minimap.is_some_and(|m| m.bubble.is_none()));
        instance.lock().zoom_minimap(0, 0.0, 0.0, 4);
        let bubble = first_tile(&instance).minimap.and_then(|m| m.bubble).expect("the bubble is open");
        assert_eq!(bubble.permille, 3000);
        // It is the same slider as the bar's: a click on it zooms the tile, and the bar's slider shows the same number.
        instance.lock().zoom_minimap_slider(0, BarClick::ZoomTo(2000).zoom_code().expect("a slider code"));
        assert_eq!(first_tile(&instance).minimap.and_then(|m| m.bubble).map(|b| b.permille), Some(2000));
        instance.lock().toggle_zoom_slider(0);
        assert_eq!(instance.lock().zoom_bubble().map(|b| b.permille), Some(2000));
        // A second right click closes it.
        instance.lock().zoom_minimap(0, 0.0, 0.0, 4);
        assert!(first_tile(&instance).minimap.is_some_and(|m| m.bubble.is_none()));
        // A middle click is back to 100% at once, smooth zooms or not.
        instance.lock().zoom_minimap(0, 0.0, 0.0, 3);
        assert!(first_tile(&instance).zoom.is_none());
        assert_eq!(instance.lock().zoom_percent(0).map(|p| p.round() as i32), Some(100));
    }

    #[test]
    fn the_wheel_zooms_in_whole_notches_around_the_pointer() {
        let instance = instance(vec![image_seed(1)], tab_view());
        let now = Instant::now();
        // Half a notch is nothing yet.
        instance.lock().zoom_wheel_at(0, 60.0, (900.0, 200.0), now);
        assert!(first_tile(&instance).zoom.is_none());
        // The second half makes one step (10% by default).
        instance.lock().zoom_wheel_at(0, 60.0, (900.0, 200.0), now);
        assert_eq!(instance.lock().zoom_percent(0).map(|p| p.round() as i32), Some(110));
        // Wheel down goes back; a direction change does not carry over what was left.
        instance.lock().zoom_wheel_at(0, -120.0, (900.0, 200.0), now);
        assert!(first_tile(&instance).zoom.is_none());
        // Zooming at a corner keeps that corner of the picture there: the window starts at the top left.
        instance.lock().zoom_wheel_at(0, 120.0 * 20.0, (0.0, 0.0), now);
        let z = instance.lock().tiles[0].zoom.as_ref().unwrap().viewport_norm().unwrap();
        assert!(z.x.abs() < 1e-9 && z.y.abs() < 1e-9, "{z:?}");
    }

    #[test]
    fn dragging_moves_the_picture_and_the_minimap_moves_the_window() {
        let instance = instance(vec![image_seed(1)], tab_view());
        let key = instance.lock().tiles[0].key;
        instance.lock().perform_click(key, BarClick::ZoomTo(3000));
        let before = instance.lock().tiles[0].zoom.as_ref().unwrap().viewport_norm().unwrap();
        instance.lock().zoom_pan(0, 120.0, 0.0);
        let after = instance.lock().tiles[0].zoom.as_ref().unwrap().viewport_norm().unwrap();
        assert!(after.x < before.x, "the picture followed the pointer to the right: {before:?} {after:?}");
        instance.lock().zoom_minimap(0, 0.9, 0.9, 0);
        let moved = instance.lock().tiles[0].zoom.as_ref().unwrap().viewport_norm().unwrap();
        assert!((moved.right() - 1.0).abs() < 1e-6 || moved.x > after.x, "{moved:?}");
        instance.lock().zoom_minimap(0, -4.0, -4.0, 1);
        let corner = instance.lock().tiles[0].zoom.as_ref().unwrap().viewport_norm().unwrap();
        assert!(corner.x.abs() < 1e-9 && corner.y.abs() < 1e-9);
        instance.lock().zoom_minimap(0, 0.0, 0.0, 2);
        assert!(!instance.lock().tiles[0].zoom.as_ref().unwrap().minimap_dragging());
    }

    #[test]
    fn the_minimap_is_in_the_view_after_a_zoom_and_comes_back_with_the_pointer() {
        let instance = instance(vec![image_seed(1)], tab_view());
        let key = instance.lock().tiles[0].key;
        instance.lock().perform_click(key, BarClick::ZoomTo(2000));
        instance.lock().poll_zoom();
        let tile = first_tile(&instance);
        let minimap = tile.minimap.expect("shown right after a zoom");
        assert!(minimap.visible && minimap.rect.x > 0 && minimap.rect.y + minimap.rect.h <= 900);
        assert!((minimap.viewport.2 - 0.5).abs() < 1e-6, "a window of half the width: {:?}", minimap.viewport);
        // Later (a poll in the future) it is gone, unless the pointer is where it sits.
        let later = Instant::now() + Duration::from_secs(10);
        instance.lock().poll_zoom_at(later);
        let layout = MinimapLayout::compute((1200, 900), (4000, 3000), zoom_prefs::current().minimap_corner, 1.0).unwrap();
        let spot = (AREA.x + layout.rect.x + 10, AREA.y + layout.rect.y + 10);
        instance.lock().tick(|| Some(spot));
        assert!(instance.lock().tiles[0].zoom.as_ref().unwrap().minimap_shown(&zoom_prefs::current(), Instant::now()));
        instance.lock().tick(|| Some((AREA.x + 600, AREA.y + 100)));
        // The pointer is gone and the time is over.
        assert!(!instance.lock().tiles[0].zoom.as_ref().unwrap().minimap_shown(&zoom_prefs::current(), later));
    }

    #[test]
    fn the_zoom_follows_the_tile_when_the_layout_changes() {
        let instance = instance(vec![image_seed(1)], tab_view());
        let key = instance.lock().tiles[0].key;
        instance.lock().perform_click(key, BarClick::ZoomTo(2500));
        let before = instance.lock().tiles[0].zoom.as_ref().unwrap().viewport_norm().unwrap();
        instance.lock().set_host(&HostSurface::tab_view(1, Rect::new(0, 0, 600, 450), 1.0, Vec::new()));
        let tile = first_tile(&instance);
        assert_eq!((tile.zoom.unwrap().dest.w, tile.pixels.is_some()), (600, true));
        let after = instance.lock().tiles[0].zoom.as_ref().unwrap().viewport_norm().unwrap();
        assert!((after.w - before.w).abs() < 1e-9, "the same part of the image: {before:?} {after:?}");
    }

    #[test]
    fn a_stale_slider_click_after_the_tile_left_does_nothing() {
        let instance = instance(vec![image_seed(1), image_seed(2)], tab_view());
        let key = instance.lock().tiles[1].key;
        instance.lock().toggle_zoom_slider(1);
        let keep = instance.lock().tiles[0].key;
        let plan = crate::viewer::multiview::follow::FollowPlan {
            remove: vec![key],
            slots: vec![crate::viewer::multiview::follow::Slot::Keep(keep)],
            ..Default::default()
        };
        instance.lock().apply_follow(&plan, Vec::new());
        instance.lock().poll_zoom();
        assert!(instance.lock().zoom_bubble().is_none());
        let _ = instance.lock().perform_click(key, BarClick::ZoomTo(3000));
        assert!(first_tile(&instance).zoom.is_none());
    }

    #[test]
    fn a_broken_or_missing_picture_is_not_zoomable_and_calls_are_harmless() {
        let instance = instance(vec![image_seed(1)], tab_view());
        instance.lock().zoom_wheel_at(7, 120.0, (0.0, 0.0), Instant::now());
        instance.lock().zoom_pan(7, 5.0, 5.0);
        instance.lock().zoom_minimap(7, 0.5, 0.5, 0);
        instance.lock().zoom_reset(7);
        instance.lock().zoom_wheel_at(0, f32::NAN, (0.0, 0.0), Instant::now());
        instance.lock().tiles[0].error = Some("gone".into());
        assert_eq!(instance.lock().zoom_button_state(0), None);
        assert!(instance.lock().zoom_percent(7).is_none());
    }
}
