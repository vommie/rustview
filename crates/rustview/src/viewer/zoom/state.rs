//! The zoom state of one image (pure): the view the user wants, the animation towards it, the original and the rendered
//! picture, and the minimap.
//!
//! The owner (a Multiview instance, the image tab) feeds it the input events and the time and gets back what has to be done:
//! `poll` asks for the original to be decoded and for pictures to be rendered; the owner runs that on worker threads and hands
//! the results back (`source_loaded`, `rendered`). Until a rendered picture for exactly the current view exists the screen shows a
//! *preview*: the picture the tile already has, cut out and scaled by the graphics card, so every change is visible at once and
//! the sharp picture follows a moment later (the debounce keeps a burst of wheel steps from rendering every step).

use super::geometry::{Area, Frame, IRect, Limits, Placement, RectF, Visible, ZoomView};
use super::minimap::Grab;
use super::prefs::ZoomPrefs;
use super::render::{Resample, SourceImage, render_view};
use super::scale;
use rustview_core::util::cast::FloatToInt;

use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a smooth zoom glides.
const ANIMATION: Duration = Duration::from_millis(160);
/// How long a view has to rest before the sharp picture is rendered.
const RENDER_DEBOUNCE: Duration = Duration::from_millis(60);
/// The minimap stays this long after the last change of the view.
/// The original is kept this long after the view went back to 100% (a zoom right after does not decode again).
const RELEASE_AFTER: Duration = Duration::from_secs(8);
const FIT_EPS: f64 = 1e-4;

/// Which grid of pixels a render belongs to: the zoom, the room, the filter. Not the centre: a render covers more than is visible
/// (`OVERSCAN`), and panning only moves the window over it, so a picture stays valid while the view is dragged around.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderKey {
    zoom: u64,
    tile: Area,
    algorithm: Resample,
}

impl RenderKey {
    fn of(view: ZoomView, tile: Area, algorithm: Resample) -> Self {
        Self {
            zoom: view.zoom.to_bits(),
            tile,
            algorithm,
        }
    }
}

/// How much more than the room is rendered on every side (a share of the room, within these limits in pixels), and how close to the
/// edge of that margin the window may come before a new render is made.
const OVERSCAN_SHARE: f64 = 0.25;
const OVERSCAN_MIN_PX: i32 = 64;
const OVERSCAN_MAX_PX: i32 = 512;
const OVERSCAN_REFRESH_AT: f64 = 0.4;

fn overscan_px(room: f64) -> i32 {
    (room * OVERSCAN_SHARE)
        .round()
        .to_int::<i32>()
        .clamp(OVERSCAN_MIN_PX, OVERSCAN_MAX_PX)
}

/// The part of the virtual image (`w` x `h`) a render covers around `window`.
fn render_region(placement: &Placement, room: (f64, f64)) -> IRect {
    let (mx, my) = (overscan_px(room.0), overscan_px(room.1));
    let x0 = (placement.window.x - mx).max(0);
    let y0 = (placement.window.y - my).max(0);
    let x1 = (placement.window.x + placement.window.w + mx).min(placement.w);
    let y1 = (placement.window.y + placement.window.h + my).min(placement.h);
    IRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

fn contains(outer: IRect, inner: IRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.w <= outer.x + outer.w
        && inner.y + inner.h <= outer.y + outer.h
}

/// One picture to resample, with everything it needs; `run` is what a worker thread does.
pub struct RenderJob {
    pub generation: u64,
    pub key: RenderKey,
    source: Arc<SourceImage>,
    visible: Visible,
    /// `visible.dest` as a part of the virtual image (the pixels' place in it).
    region: IRect,
    algorithm: Resample,
}

/// The result of a job.
pub struct RenderDone {
    pub generation: u64,
    pub key: RenderKey,
    /// The part of the virtual image the pixels are.
    pub region: IRect,
    pub pixels: Result<SharedPixelBuffer<Rgb8Pixel>, String>,
}

impl RenderJob {
    pub fn run(self) -> RenderDone {
        let pixels = render_view(&self.source, &self.visible, self.algorithm);
        RenderDone {
            generation: self.generation,
            key: self.key,
            region: self.region,
            pixels,
        }
    }
}

/// What `poll` asks the owner to do.
pub enum Request {
    /// Decode the original of the image (full resolution) and hand it back with `source_loaded`.
    LoadSource,
    Render(RenderJob),
}

enum SourceState {
    None,
    Loading,
    Ready(Arc<SourceImage>),
    Failed(String),
}

struct Animation {
    from: ZoomView,
    to: ZoomView,
    start: Instant,
}

struct Hq {
    key: RenderKey,
    pixels: SharedPixelBuffer<Rgb8Pixel>,
    /// The part of the virtual image the pixels are.
    region: IRect,
}

/// What is drawn instead of the plain, fitted picture.
pub struct Picture {
    /// Where it goes in the tile, whole pixels.
    pub dest: IRect,
    pub content: PictureContent,
}

pub enum PictureContent {
    /// The sharp picture, larger than `dest`; `clip` (its pixels) is the part that goes to `dest`.
    Rendered {
        pixels: SharedPixelBuffer<Rgb8Pixel>,
        clip: IRect,
    },
    /// The tile's own picture, this rectangle of it, scaled by the graphics card.
    Preview { clip: IRect },
}

/// A drag on the minimap: where it grabbed the rectangle, when the pointer last moved and how fast it moves (smoothed, minimap
/// pixels per second).
struct Drag {
    /// Where in the rectangle the press was (the absolute drag keeps it under the pointer).
    grab: Grab,
    at: Instant,
    speed: f64,
}

#[derive(Default)]
struct MinimapState {
    hover: bool,
    /// The slider bubble on the minimap is open: the minimap stays, whatever the zoom.
    pinned: bool,
    drag: Option<Drag>,
    reveal_until: Option<Instant>,
}

pub struct ZoomState {
    image: (u32, u32),
    tile: Option<Area>,
    target: ZoomView,
    shown: ZoomView,
    animation: Option<Animation>,
    source: SourceState,
    generation: u64,
    in_flight: Option<(u64, RenderKey)>,
    /// A render that failed is not tried again for the same view.
    failed_key: Option<RenderKey>,
    hq: Option<Hq>,
    changed_at: Option<Instant>,
    release_at: Option<Instant>,
    thumb: Option<SharedPixelBuffer<Rgb8Pixel>>,
    minimap: MinimapState,
    /// A picture is resampled here; a video is drawn by mpv (`mpv_view`), so its zoom never asks for decoding or rendering.
    renders: bool,
}

impl ZoomState {
    /// The zoom of a video of `size` pixels (its display size): the same model, without the picture work.
    pub fn new_video(size: (u32, u32)) -> Self {
        Self {
            renders: false,
            ..Self::new(size)
        }
    }

    /// A fresh zoom of the same medium (a tile that moved to another Multiview).
    pub fn restarted(&self) -> Self {
        Self {
            renders: self.renders,
            ..Self::new(self.image)
        }
    }

    pub fn new(image: (u32, u32)) -> Self {
        Self {
            image,
            tile: None,
            target: ZoomView::FIT,
            shown: ZoomView::FIT,
            animation: None,
            source: SourceState::None,
            generation: 0,
            in_flight: None,
            failed_key: None,
            hq: None,
            changed_at: None,
            release_at: None,
            thumb: None,
            minimap: MinimapState::default(),
            renders: true,
        }
    }

    pub fn image(&self) -> (u32, u32) {
        self.image
    }

    /// Where the tile (the place of the fitted picture) is inside the room, if it is known.
    pub fn home(&self) -> Option<IRect> {
        self.tile.map(|a| a.home)
    }

    /// The size of the room the picture may use, if the tile is known.
    pub fn room(&self) -> Option<(i32, i32)> {
        self.tile.map(|a| a.size)
    }

    fn frame(&self) -> Option<Frame> {
        Frame::in_area(self.image, self.tile?)
    }

    /// Where the whole picture is in its room as it is shown now (physical pixels, relative to the room; it may reach out of the
    /// room when zoomed in). The crop tool maps the pointer through it, so cropping works the same at every zoom.
    pub fn image_rect(&self) -> Option<super::geometry::RectF> {
        Some(self.frame()?.image_rect(self.shown))
    }

    /// The zoom the user chose, as a factor of the fitted size (the animation may still be on its way).
    pub fn zoom(&self) -> f64 {
        self.target.zoom
    }

    /// Whether the picture is anything but the plain fitted one (or on its way back to it): then `picture` has something to draw.
    pub fn is_active(&self) -> bool {
        !self.shown.is_fit() || !self.target.is_fit() || self.animation.is_some()
    }

    /// Dragging the picture moves something.
    pub fn can_pan(&self) -> bool {
        self.frame().is_some_and(|f| f.is_pannable(self.target))
    }

    pub fn animating(&self) -> bool {
        self.animation.is_some()
    }

    pub fn failed(&self) -> Option<&str> {
        match &self.source {
            SourceState::Failed(why) => Some(why),
            _ => None,
        }
    }

    // ---- the tile -------------------------------------------------------------------------------------------------------

    /// The tile the picture is shown in, with no room around it (physical pixels); see `set_area`.
    pub fn set_tile(&mut self, tile: (i32, i32), prefs: &ZoomPrefs) -> bool {
        self.set_area(Area::plain(tile), prefs)
    }

    /// The room the picture may use (physical pixels): the tile and the free space around it. A different room re-fits the view
    /// and asks for a new render. Returns whether anything changed.
    pub fn set_area(&mut self, area: Area, prefs: &ZoomPrefs) -> bool {
        if !area.is_valid() || self.tile == Some(area) {
            return false;
        }
        self.tile = Some(area);
        if let Some(frame) = self.frame() {
            let limits = prefs.limits();
            self.target = frame.normalize(self.target, limits);
            self.shown = frame.normalize(self.shown, limits);
        }
        self.hq = None;
        self.minimap.hover = false;
        true
    }

    // ---- input ------------------------------------------------------------------------------------------------------------

    fn start_view_change(&mut self, to: ZoomView, prefs: &ZoomPrefs, now: Instant, animate: bool) {
        let from = self.shown;
        self.target = if (to.zoom - 1.0).abs() < FIT_EPS {
            ZoomView::FIT
        } else {
            to
        };
        self.changed_at = Some(now);
        self.minimap.reveal_until = Some(now + prefs.minimap_timeout);
        self.release_at = self.target.is_fit().then_some(now + RELEASE_AFTER);
        if animate && prefs.smooth && from != self.target {
            self.animation = Some(Animation {
                from,
                to: self.target,
                start: now,
            });
        } else {
            self.animation = None;
            self.shown = self.target;
        }
    }

    /// Zooms to `zoom` (a factor of the fitted size). `anchor` is a point of the tile in pixels that stays where it is.
    pub fn zoom_to(&mut self, zoom: f64, anchor: Option<(f64, f64)>, prefs: &ZoomPrefs, now: Instant) {
        let Some(frame) = self.frame() else { return };
        let next = frame.zoom_to(self.shown, zoom, anchor, prefs.limits());
        if next != self.target {
            self.start_view_change(next, prefs, now, true);
        }
    }

    /// One step of the wheel or of the slider's buttons: up or down the ladder of `step` (0.1 = 10%).
    pub fn zoom_step(&mut self, up: bool, step: f64, anchor: Option<(f64, f64)>, prefs: &ZoomPrefs, now: Instant) {
        let next = scale::step(self.target.zoom, up, step, prefs.limits());
        self.zoom_to(next, anchor, prefs, now);
    }

    /// Back to the fitted picture (100%).
    pub fn reset(&mut self, prefs: &ZoomPrefs, now: Instant) {
        if self.target != ZoomView::FIT || self.animation.is_some() {
            self.start_view_change(ZoomView::FIT, prefs, now, true);
        }
    }

    /// Back to the fitted picture at once, whatever the setting for smooth zooms says.
    pub fn reset_now(&mut self, prefs: &ZoomPrefs, now: Instant) {
        if self.target != ZoomView::FIT || self.animation.is_some() {
            self.start_view_change(ZoomView::FIT, prefs, now, false);
        }
    }

    /// The picture follows a drag of the pointer by `dx`, `dy` screen pixels.
    pub fn pan_pixels(&mut self, dx: f64, dy: f64, prefs: &ZoomPrefs, now: Instant) {
        let Some(frame) = self.frame() else { return };
        if !frame.is_pannable(self.shown) {
            return;
        }
        let next = frame.pan_pixels(self.shown, dx, dy);
        if next != self.target || self.animation.is_some() {
            self.start_view_change(next, prefs, now, false);
        }
    }

    /// The slider bubble of the minimap is open (or not): the minimap is shown as long as it is.
    pub fn set_minimap_pinned(&mut self, pinned: bool) {
        self.minimap.pinned = pinned;
    }

    pub fn minimap_pinned(&self) -> bool {
        self.minimap.pinned
    }

    /// The pointer is on (or just around) the place of the minimap.
    pub fn set_minimap_hover(&mut self, hover: bool) {
        self.minimap.hover = hover;
    }

    /// A press on the minimap at (`nx`, `ny`), shares of the image: the rectangle is grabbed where it is, or jumps to the press
    /// (which is meant: a click on the minimap goes there).
    pub fn minimap_press(&mut self, nx: f64, ny: f64, prefs: &ZoomPrefs, now: Instant) {
        let Some(frame) = self.frame() else { return };
        let grab = Grab::at(nx, ny, frame.viewport_norm(self.shown));
        self.minimap.drag = Some(Drag {
            grab,
            at: now,
            speed: 0.0,
        });
        self.minimap_move(nx, ny, prefs, now);
    }

    /// The pointer moved while the minimap is pressed and the pointer is not locked: the rectangle stays under it (1:1).
    pub fn minimap_move(&mut self, nx: f64, ny: f64, prefs: &ZoomPrefs, now: Instant) {
        let (Some(frame), Some(drag)) = (self.frame(), self.minimap.drag.as_ref()) else {
            return;
        };
        let (mx, my) = drag.grab.middle_for(nx, ny);
        let next = frame.center_for_viewport(self.shown, mx, my);
        if next != self.target || self.animation.is_some() {
            self.start_view_change(next, prefs, now, false);
        }
    }

    /// The pointer moved by (`dx`, `dy`) pixels while the minimap is pressed and the pointer is locked to it (hidden, held in place,
    /// only its movement is read), `mm_px` being the size of the minimap in pixels.
    ///
    /// The rectangle moves relative to the pointer, by a share of its movement (`minimap::drag_gain`): the further in the picture
    /// is zoomed, the finer a slow drag, so the picture does not leap by hundreds of pixels per pointer pixel; a fast flick is
    /// followed 1:1 to get across. The rectangle may lag behind where the pointer would be; at an edge it simply stops.
    pub fn minimap_drag_by(&mut self, dx: f64, dy: f64, mm_px: (f64, f64), prefs: &ZoomPrefs, now: Instant) {
        let (Some(frame), Some(drag)) = (self.frame(), self.minimap.drag.as_mut()) else {
            return;
        };
        if mm_px.0 <= 0.0 || mm_px.1 <= 0.0 {
            return;
        }
        let seconds = now.saturating_duration_since(drag.at).as_secs_f64().max(0.001);
        let instant = dx.hypot(dy) / seconds;
        // Smoothed, so one slow or fast event does not flip the gain.
        drag.speed = if drag.speed == 0.0 {
            instant * 0.5
        } else {
            drag.speed * 0.65 + instant * 0.35
        };
        drag.at = now;
        let (ex, ey) = frame.extent_px(self.shown);
        let gain = super::minimap::drag_gain((ex / mm_px.0).max(ey / mm_px.1), drag.speed);
        let next = frame.center_for_viewport(
            self.shown,
            self.shown.cx + dx / mm_px.0 * gain,
            self.shown.cy + dy / mm_px.1 * gain,
        );
        if next != self.target || self.animation.is_some() {
            self.start_view_change(next, prefs, now, false);
        }
    }

    pub fn minimap_release(&mut self) {
        self.minimap.drag = None;
    }

    pub fn minimap_dragging(&self) -> bool {
        self.minimap.drag.is_some()
    }

    // ---- time and the work -------------------------------------------------------------------------------------------------

    fn key_for(&self, view: ZoomView, prefs: &ZoomPrefs) -> Option<RenderKey> {
        Some(RenderKey::of(view, self.tile?, prefs.resample))
    }

    /// Advances the animation and says what has to be done now. `display` is the picture the tile has (it is the source of
    /// every view that is not enlarged, and of the preview). Cheap when nothing is going on.
    pub fn poll(
        &mut self,
        display: Option<&SharedPixelBuffer<Rgb8Pixel>>,
        prefs: &ZoomPrefs,
        now: Instant,
    ) -> Vec<Request> {
        let mut requests = Vec::new();
        let Some(frame) = self.frame() else { return requests };
        let limits = prefs.limits();

        if let Some(animation) = &self.animation {
            let t = (now.saturating_duration_since(animation.start).as_secs_f64() / ANIMATION.as_secs_f64())
                .clamp(0.0, 1.0);
            if t >= 1.0 {
                self.shown = self.target;
                self.animation = None;
            } else {
                self.shown = frame.normalize(Self::interpolate(animation.from, animation.to, ease_out(t)), limits);
            }
        }

        // Back at 100% for a while: the original and the sharp picture are not needed any more.
        if self.target.is_fit() && self.animation.is_none() {
            if self.release_at.is_some_and(|at| now >= at) {
                self.source = SourceState::None;
                self.hq = None;
                self.release_at = None;
            }
            return requests;
        }

        if self.animation.is_some() || !self.renders {
            return requests;
        }
        let Some(key) = self.key_for(self.target, prefs) else {
            return requests;
        };
        let Some(placement) = frame.placement(self.target) else {
            return requests;
        };
        if self.failed_key == Some(key) {
            return requests;
        }
        // The picture we have is good while the window is well inside what it covers.
        let room = (
            f64::from(self.tile.map_or(0, |a| a.size.0)),
            f64::from(self.tile.map_or(0, |a| a.size.1)),
        );
        if self
            .hq
            .as_ref()
            .is_some_and(|hq| hq.key == key && Self::covers_with_reserve(hq.region, &placement, room))
        {
            return requests;
        }

        let display_is_original = display.is_some_and(|d| (d.width(), d.height()) == self.image);
        let needs_original = self.target.zoom > 1.0 + FIT_EPS && !display_is_original;
        if needs_original && matches!(self.source, SourceState::None) {
            self.source = SourceState::Loading;
            requests.push(Request::LoadSource);
        }
        // A new zoom waits for the view to rest (the slider may be dragged); a window that nears the end of what is rendered (a drag)
        // gets its picture at once, the old one is still on screen meanwhile.
        let same_grid = self.hq.as_ref().is_some_and(|hq| hq.key == key);
        let resting = same_grid
            || self
                .changed_at
                .is_none_or(|at| now.saturating_duration_since(at) >= RENDER_DEBOUNCE);
        if self.in_flight.is_some() || !resting {
            return requests;
        }
        let region = render_region(&placement, room);
        let s = frame.source_per_pixel(self.target.zoom);
        let visible = Visible {
            dest: region,
            src: RectF::new(
                f64::from(region.x) * s,
                f64::from(region.y) * s,
                f64::from(region.w) * s,
                f64::from(region.h) * s,
            ),
        };
        let job_source = match (&self.source, needs_original, display) {
            (SourceState::Ready(source), true, _) => Some((source.clone(), visible)),
            (_, false, Some(display)) => Self::display_source(display, self.image, visible),
            _ => None,
        };
        if let Some((source, visible)) = job_source {
            self.generation += 1;
            self.in_flight = Some((self.generation, key));
            requests.push(Request::Render(RenderJob {
                generation: self.generation,
                key,
                source,
                visible,
                region,
                algorithm: prefs.resample,
            }));
        }
        requests
    }

    /// Whether a render of `region` shows the window of `placement` and still has a reserve on every side that has more image
    /// behind it (otherwise a new render is due before the window runs out of it).
    fn covers_with_reserve(region: IRect, placement: &Placement, room: (f64, f64)) -> bool {
        if !contains(region, placement.window) {
            return false;
        }
        let (mx, my) = (overscan_px(room.0), overscan_px(room.1));
        let need = |margin: i32| (f64::from(margin) * OVERSCAN_REFRESH_AT).round().to_int::<i32>();
        let w = placement.window;
        (region.x == 0 || w.x - region.x >= need(mx))
            && (region.y == 0 || w.y - region.y >= need(my))
            && (region.x + region.w >= placement.w || region.x + region.w - (w.x + w.w) >= need(mx))
            && (region.y + region.h >= placement.h || region.y + region.h - (w.y + w.h) >= need(my))
    }

    /// The tile's own picture as the source of a render: the visible rectangle is given in pixels of the original, the picture
    /// is a downscaled copy of it.
    fn display_source(
        display: &SharedPixelBuffer<Rgb8Pixel>,
        image: (u32, u32),
        visible: Visible,
    ) -> Option<(Arc<SourceImage>, Visible)> {
        let source = SourceImage::new(display.width(), display.height(), display.clone())?;
        let fx = f64::from(display.width()) / f64::from(image.0);
        let fy = f64::from(display.height()) / f64::from(image.1);
        let src = RectF::new(
            visible.src.x * fx,
            visible.src.y * fy,
            visible.src.w * fx,
            visible.src.h * fy,
        );
        Some((
            Arc::new(source),
            Visible {
                dest: visible.dest,
                src,
            },
        ))
    }

    fn interpolate(from: ZoomView, to: ZoomView, t: f64) -> ZoomView {
        // The zoom glides geometrically (the same ratio per moment), the centre linearly.
        let zoom = (from.zoom.ln() + (to.zoom.ln() - from.zoom.ln()) * t).exp();
        ZoomView {
            zoom,
            cx: from.cx + (to.cx - from.cx) * t,
            cy: from.cy + (to.cy - from.cy) * t,
        }
    }

    pub fn source_loaded(&mut self, result: Result<Arc<SourceImage>, String>) {
        self.source = match result {
            Ok(source) if (source.width, source.height) == self.image => SourceState::Ready(source),
            Ok(_) => SourceState::Failed("the original does not have the size of the picture".to_string()),
            Err(why) => SourceState::Failed(why),
        };
    }

    pub fn rendered(&mut self, done: RenderDone, prefs: &ZoomPrefs) {
        if self
            .in_flight
            .is_some_and(|(generation, key)| generation == done.generation && key == done.key)
        {
            self.in_flight = None;
        }
        let current = self.key_for(self.target, prefs);
        match done.pixels {
            Ok(pixels) if current == Some(done.key) => {
                self.hq = Some(Hq {
                    key: done.key,
                    pixels,
                    region: done.region,
                });
            }
            Ok(_) => {}
            Err(why) => {
                tracing::warn!(error = %why, "[Zoom: Render] A zoomed picture could not be made");
                self.failed_key = Some(done.key);
            }
        }
    }

    // ---- drawing ----------------------------------------------------------------------------------------------------------

    /// What to draw now, or `None` for the plain fitted picture. `display_size` is the size of the picture the tile has.
    pub fn picture(&self, display_size: Option<(u32, u32)>, prefs: &ZoomPrefs) -> Option<Picture> {
        if !self.is_active() {
            return None;
        }
        let frame = self.frame()?;
        let visible = frame.visible(self.shown)?;
        if self.animation.is_none()
            && let (Some(hq), Some(key), Some(placement)) =
                (&self.hq, self.key_for(self.shown, prefs), frame.placement(self.shown))
            && hq.key == key
            && contains(hq.region, placement.window)
        {
            let clip = IRect {
                x: placement.window.x - hq.region.x,
                y: placement.window.y - hq.region.y,
                w: placement.window.w,
                h: placement.window.h,
            };
            return Some(Picture {
                dest: placement.dest(),
                content: PictureContent::Rendered {
                    pixels: hq.pixels.clone(),
                    clip,
                },
            });
        }
        let (dw, dh) = display_size?;
        let fx = f64::from(dw) / f64::from(self.image.0);
        let fy = f64::from(dh) / f64::from(self.image.1);
        let x = (visible.src.x * fx).floor().clamp(0.0, f64::from(dw - 1));
        let y = (visible.src.y * fy).floor().clamp(0.0, f64::from(dh - 1));
        let w = ((visible.src.right() * fx).ceil() - x).clamp(1.0, f64::from(dw) - x);
        let h = ((visible.src.bottom() * fy).ceil() - y).clamp(1.0, f64::from(dh) - y);
        Some(Picture {
            dest: visible.dest,
            content: PictureContent::Preview {
                clip: IRect {
                    x: x.to_int::<i32>(),
                    y: y.to_int::<i32>(),
                    w: w.to_int::<i32>(),
                    h: h.to_int::<i32>(),
                },
            },
        })
    }

    /// The visible part of the image as shares of it (the rectangle of the minimap).
    pub fn viewport_norm(&self) -> Option<RectF> {
        self.frame().map(|f| f.viewport_norm(self.shown))
    }

    /// Whether the minimap is shown: the picture is zoomed in (there is something to move) and the user wants to see it (always,
    /// the pointer is near it, it is dragged, or the view changed a moment ago).
    pub fn minimap_shown(&self, prefs: &ZoomPrefs, now: Instant) -> bool {
        let zoomed_in = self.shown.zoom > 1.0 + FIT_EPS && self.frame().is_some_and(|f| f.is_pannable(self.shown));
        self.minimap.pinned
            || zoomed_in
                && (prefs.minimap_always
                    || self.minimap.hover
                    || self.minimap.drag.is_some()
                    || self.minimap.reveal_until.is_some_and(|until| now < until))
    }

    /// When the minimap will go away if nothing happens (so the owner knows to look again then).
    pub fn minimap_deadline(&self) -> Option<Instant> {
        self.minimap.reveal_until
    }

    pub fn thumb(&self) -> Option<&SharedPixelBuffer<Rgb8Pixel>> {
        self.thumb.as_ref()
    }

    /// Makes the thumbnail of the minimap from the tile's picture, once.
    /// The minimap of a video shows a still of it; a newer still replaces the old one.
    pub fn replace_thumb(&mut self, still: &SharedPixelBuffer<Rgb8Pixel>) {
        self.thumb = None;
        self.ensure_thumb(still);
    }

    pub fn ensure_thumb(&mut self, display: &SharedPixelBuffer<Rgb8Pixel>) {
        if self.thumb.is_none() {
            match super::render::thumbnail(display, 480, 360) {
                Ok(thumb) => self.thumb = Some(thumb),
                Err(why) => tracing::warn!(error = %why, "[Zoom: Minimap] No thumbnail"),
            }
        }
    }

    /// The limits as the preferences say (for the owner's slider).
    pub fn limits(prefs: &ZoomPrefs) -> Limits {
        prefs.limits()
    }
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}
