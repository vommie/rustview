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
        Self { zoom: view.zoom.to_bits(), tile, algorithm }
    }
}

/// How much more than the room is rendered on every side (a share of the room, within these limits in pixels), and how close to the
/// edge of that margin the window may come before a new render is made.
const OVERSCAN_SHARE: f64 = 0.25;
const OVERSCAN_MIN_PX: i32 = 64;
const OVERSCAN_MAX_PX: i32 = 512;
const OVERSCAN_REFRESH_AT: f64 = 0.4;

fn overscan_px(room: f64) -> i32 {
    ((room * OVERSCAN_SHARE).round() as i32).clamp(OVERSCAN_MIN_PX, OVERSCAN_MAX_PX)
}

/// The part of the virtual image (`w` x `h`) a render covers around `window`.
fn render_region(placement: &Placement, room: (f64, f64)) -> IRect {
    let (mx, my) = (overscan_px(room.0), overscan_px(room.1));
    let x0 = (placement.window.x - mx).max(0);
    let y0 = (placement.window.y - my).max(0);
    let x1 = (placement.window.x + placement.window.w + mx).min(placement.w);
    let y1 = (placement.window.y + placement.window.h + my).min(placement.h);
    IRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

fn contains(outer: IRect, inner: IRect) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.w <= outer.x + outer.w && inner.y + inner.h <= outer.y + outer.h
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
        RenderDone { generation: self.generation, key: self.key, region: self.region, pixels }
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
    Rendered { pixels: SharedPixelBuffer<Rgb8Pixel>, clip: IRect },
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
}

impl ZoomState {
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
        self.target = if (to.zoom - 1.0).abs() < FIT_EPS { ZoomView::FIT } else { to };
        self.changed_at = Some(now);
        self.minimap.reveal_until = Some(now + prefs.minimap_timeout);
        self.release_at = self.target.is_fit().then_some(now + RELEASE_AFTER);
        if animate && prefs.smooth && from != self.target {
            self.animation = Some(Animation { from, to: self.target, start: now });
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
        self.minimap.drag = Some(Drag { grab, at: now, speed: 0.0 });
        self.minimap_move(nx, ny, prefs, now);
    }

    /// The pointer moved while the minimap is pressed and the pointer is not locked: the rectangle stays under it (1:1).
    pub fn minimap_move(&mut self, nx: f64, ny: f64, prefs: &ZoomPrefs, now: Instant) {
        let (Some(frame), Some(drag)) = (self.frame(), self.minimap.drag.as_ref()) else { return };
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
        let (Some(frame), Some(drag)) = (self.frame(), self.minimap.drag.as_mut()) else { return };
        if mm_px.0 <= 0.0 || mm_px.1 <= 0.0 {
            return;
        }
        let seconds = now.saturating_duration_since(drag.at).as_secs_f64().max(0.001);
        let instant = dx.hypot(dy) / seconds;
        // Smoothed, so one slow or fast event does not flip the gain.
        drag.speed = if drag.speed == 0.0 { instant * 0.5 } else { drag.speed * 0.65 + instant * 0.35 };
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
    pub fn poll(&mut self, display: Option<&SharedPixelBuffer<Rgb8Pixel>>, prefs: &ZoomPrefs, now: Instant) -> Vec<Request> {
        let mut requests = Vec::new();
        let Some(frame) = self.frame() else { return requests };
        let limits = prefs.limits();

        if let Some(animation) = &self.animation {
            let t = (now.saturating_duration_since(animation.start).as_secs_f64() / ANIMATION.as_secs_f64()).clamp(0.0, 1.0);
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

        if self.animation.is_some() {
            return requests;
        }
        let Some(key) = self.key_for(self.target, prefs) else { return requests };
        let Some(placement) = frame.placement(self.target) else { return requests };
        if self.failed_key == Some(key) {
            return requests;
        }
        // The picture we have is good while the window is well inside what it covers.
        let room = (f64::from(self.tile.map_or(0, |a| a.size.0)), f64::from(self.tile.map_or(0, |a| a.size.1)));
        if self.hq.as_ref().is_some_and(|hq| hq.key == key && Self::covers_with_reserve(hq.region, &placement, room)) {
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
        let resting = same_grid || self.changed_at.is_none_or(|at| now.saturating_duration_since(at) >= RENDER_DEBOUNCE);
        if self.in_flight.is_some() || !resting {
            return requests;
        }
        let region = render_region(&placement, room);
        let s = frame.source_per_pixel(self.target.zoom);
        let visible = Visible {
            dest: region,
            src: RectF::new(f64::from(region.x) * s, f64::from(region.y) * s, f64::from(region.w) * s, f64::from(region.h) * s),
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
        let need = |margin: i32| (f64::from(margin) * OVERSCAN_REFRESH_AT).round() as i32;
        let w = placement.window;
        (region.x == 0 || w.x - region.x >= need(mx))
            && (region.y == 0 || w.y - region.y >= need(my))
            && (region.x + region.w >= placement.w || region.x + region.w - (w.x + w.w) >= need(mx))
            && (region.y + region.h >= placement.h || region.y + region.h - (w.y + w.h) >= need(my))
    }

    /// The tile's own picture as the source of a render: the visible rectangle is given in pixels of the original, the picture
    /// is a downscaled copy of it.
    fn display_source(display: &SharedPixelBuffer<Rgb8Pixel>, image: (u32, u32), visible: Visible) -> Option<(Arc<SourceImage>, Visible)> {
        let source = SourceImage::new(display.width(), display.height(), display.clone())?;
        let fx = f64::from(display.width()) / f64::from(image.0);
        let fy = f64::from(display.height()) / f64::from(image.1);
        let src = RectF::new(visible.src.x * fx, visible.src.y * fy, visible.src.w * fx, visible.src.h * fy);
        Some((Arc::new(source), Visible { dest: visible.dest, src }))
    }

    fn interpolate(from: ZoomView, to: ZoomView, t: f64) -> ZoomView {
        // The zoom glides geometrically (the same ratio per moment), the centre linearly.
        let zoom = (from.zoom.ln() + (to.zoom.ln() - from.zoom.ln()) * t).exp();
        ZoomView { zoom, cx: from.cx + (to.cx - from.cx) * t, cy: from.cy + (to.cy - from.cy) * t }
    }

    pub fn source_loaded(&mut self, result: Result<Arc<SourceImage>, String>) {
        self.source = match result {
            Ok(source) if (source.width, source.height) == self.image => SourceState::Ready(source),
            Ok(_) => SourceState::Failed("the original does not have the size of the picture".to_string()),
            Err(why) => SourceState::Failed(why),
        };
    }

    pub fn rendered(&mut self, done: RenderDone, prefs: &ZoomPrefs) {
        if self.in_flight.is_some_and(|(generation, key)| generation == done.generation && key == done.key) {
            self.in_flight = None;
        }
        let current = self.key_for(self.target, prefs);
        match done.pixels {
            Ok(pixels) if current == Some(done.key) => self.hq = Some(Hq { key: done.key, pixels, region: done.region }),
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
            && let (Some(hq), Some(key), Some(placement)) = (&self.hq, self.key_for(self.shown, prefs), frame.placement(self.shown))
            && hq.key == key
            && contains(hq.region, placement.window)
        {
            let clip = IRect {
                x: placement.window.x - hq.region.x,
                y: placement.window.y - hq.region.y,
                w: placement.window.w,
                h: placement.window.h,
            };
            return Some(Picture { dest: placement.dest(), content: PictureContent::Rendered { pixels: hq.pixels.clone(), clip } });
        }
        let (dw, dh) = display_size?;
        let fx = f64::from(dw) / f64::from(self.image.0);
        let fy = f64::from(dh) / f64::from(self.image.1);
        let x = (visible.src.x * fx).floor().clamp(0.0, f64::from(dw - 1));
        let y = (visible.src.y * fy).floor().clamp(0.0, f64::from(dh - 1));
        let w = ((visible.src.right() * fx).ceil() - x).clamp(1.0, f64::from(dw) - x);
        let h = ((visible.src.bottom() * fy).ceil() - y).clamp(1.0, f64::from(dh) - y);
        Some(Picture { dest: visible.dest, content: PictureContent::Preview { clip: IRect { x: x as i32, y: y as i32, w: w as i32, h: h as i32 } } })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::zoom::prefs::MinimapCorner;

    fn prefs() -> ZoomPrefs {
        ZoomPrefs { smooth: false, min_zoom: 0.1, wheel_step: 0.1, minimap_timeout: Duration::from_millis(2500), ..ZoomPrefs::DEFAULT }
    }

    fn buffer(w: u32, h: u32) -> SharedPixelBuffer<Rgb8Pixel> {
        SharedPixelBuffer::<Rgb8Pixel>::new(w, h)
    }

    fn state() -> (ZoomState, Instant) {
        let mut state = ZoomState::new((4000, 3000));
        assert!(state.set_tile((1000, 750), &prefs()));
        (state, Instant::now())
    }

    fn ms(base: Instant, millis: u64) -> Instant {
        base + Duration::from_millis(millis)
    }

    fn only_render(requests: Vec<Request>) -> RenderJob {
        let mut jobs = requests.into_iter().filter_map(|r| match r {
            Request::Render(job) => Some(job),
            Request::LoadSource => None,
        });
        let job = jobs.next().expect("a render job");
        assert!(jobs.next().is_none());
        job
    }

    #[test]
    fn a_new_state_shows_the_plain_picture() {
        let (state, _) = state();
        assert!(!state.is_active());
        assert!(state.picture(Some((1000, 750)), &prefs()).is_none());
        assert!(!state.can_pan());
        assert!((state.zoom() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn nothing_works_before_the_tile_is_known() {
        let mut state = ZoomState::new((100, 100));
        let now = Instant::now();
        state.zoom_to(2.0, None, &prefs(), now);
        assert!(!state.is_active());
        assert!(state.poll(None, &prefs(), now).is_empty());
        assert!(!state.set_tile((0, 10), &prefs()));
    }

    #[test]
    fn zooming_in_is_visible_at_once_as_a_preview_cut_from_the_tiles_picture() {
        let (mut state, now) = state();
        state.zoom_to(2.0, None, &prefs(), now);
        assert!(state.is_active() && state.can_pan());
        let picture = state.picture(Some((1000, 750)), &prefs()).unwrap();
        assert_eq!(picture.dest, IRect { x: 0, y: 0, w: 1000, h: 750 });
        match picture.content {
            // The window is 500x375 at (250, 187.5) of the 1000x750 picture; the preview rounds outwards.
            PictureContent::Preview { clip } => assert_eq!(clip, IRect { x: 250, y: 187, w: 500, h: 376 }),
            PictureContent::Rendered { .. } => panic!("nothing rendered yet"),
        }
    }

    #[test]
    fn the_original_is_asked_for_once_when_a_zoom_needs_it() {
        let (mut state, now) = state();
        let display = buffer(1000, 750);
        state.zoom_to(3.0, None, &prefs(), now);
        let first = state.poll(Some(&display), &prefs(), now);
        assert!(first.iter().any(|r| matches!(r, Request::LoadSource)));
        let second = state.poll(Some(&display), &prefs(), ms(now, 100));
        assert!(second.iter().all(|r| !matches!(r, Request::LoadSource)), "asked only once");
        assert!(second.iter().all(|r| !matches!(r, Request::Render(_))), "nothing to render from yet");
    }

    #[test]
    fn a_zoom_out_or_a_picture_that_is_the_original_needs_no_decode() {
        let (mut state, now) = state();
        let display = buffer(1000, 750);
        state.zoom_to(0.5, None, &prefs(), now);
        let requests = state.poll(Some(&display), &prefs(), ms(now, 100));
        assert!(requests.iter().all(|r| !matches!(r, Request::LoadSource)));
        assert!(requests.iter().any(|r| matches!(r, Request::Render(_))));

        let mut state = ZoomState::new((1000, 750));
        state.set_tile((1000, 750), &prefs());
        state.zoom_to(4.0, None, &prefs(), now);
        let requests = state.poll(Some(&buffer(1000, 750)), &prefs(), ms(now, 100));
        assert!(requests.iter().all(|r| !matches!(r, Request::LoadSource)), "the tile's picture is already the original");
        assert!(requests.iter().any(|r| matches!(r, Request::Render(_))));
    }

    #[test]
    fn the_render_waits_for_the_view_to_rest() {
        let (mut state, now) = state();
        let source = Arc::new(SourceImage::new(4000, 3000, buffer(4000, 3000)).unwrap());
        state.zoom_to(2.0, None, &prefs(), now);
        state.source_loaded(Ok(source));
        assert!(state.poll(None, &prefs(), ms(now, 10)).is_empty(), "debounce");
        let job = only_render(state.poll(None, &prefs(), ms(now, 100)));
        assert_eq!(job.generation, 1);
        // While it is on its way no second one starts.
        assert!(state.poll(None, &prefs(), ms(now, 200)).is_empty());
    }

    #[test]
    fn a_finished_render_replaces_the_preview_and_a_stale_one_is_dropped() {
        let (mut state, now) = state();
        let source = Arc::new(SourceImage::new(4000, 3000, buffer(4000, 3000)).unwrap());
        state.zoom_to(2.0, None, &prefs(), now);
        state.source_loaded(Ok(source));
        let job = only_render(state.poll(None, &prefs(), ms(now, 100)));
        // The user zooms on before the job is done.
        state.zoom_to(3.0, None, &prefs(), ms(now, 120));
        state.rendered(job.run(), &prefs());
        match state.picture(Some((1000, 750)), &prefs()).unwrap().content {
            PictureContent::Preview { .. } => {}
            PictureContent::Rendered { .. } => panic!("the stale render must not be shown"),
        }
        // The next poll renders the current view and its result is shown.
        let job = only_render(state.poll(None, &prefs(), ms(now, 300)));
        assert_eq!(job.generation, 2);
        state.rendered(job.run(), &prefs());
        match state.picture(Some((1000, 750)), &prefs()).unwrap().content {
            PictureContent::Rendered { pixels, clip } => {
                assert_eq!((clip.w, clip.h), (1000, 750));
                assert!(pixels.width() >= 1000 && pixels.height() >= 750);
            }
            PictureContent::Preview { .. } => panic!("the sharp picture is ready"),
        }
        // Nothing more to do.
        assert!(state.poll(None, &prefs(), ms(now, 400)).is_empty());
    }

    #[test]
    fn panning_keeps_the_sharp_picture_and_only_moves_the_window_over_it() {
        let (mut state, now) = state();
        let p = prefs();
        let source = Arc::new(SourceImage::new(4000, 3000, buffer(4000, 3000)).unwrap());
        state.zoom_to(3.0, None, &p, now);
        state.source_loaded(Ok(source));
        let job = only_render(state.poll(None, &p, ms(now, 100)));
        // More than the room is rendered (a margin around it), at the pixels of the zoomed image (3000 x 2250).
        assert!(job.region.w > 1000 && job.region.h > 750, "{:?}", job.region);
        state.rendered(job.run(), &p);
        let sharp = |state: &ZoomState| match state.picture(Some((1000, 750)), &ZoomPrefs::DEFAULT).unwrap().content {
            PictureContent::Rendered { clip, .. } => Some(clip),
            PictureContent::Preview { .. } => None,
        };
        let first = sharp(&state).expect("sharp after the render");
        // A small drag: no preview, no new render, the window just moved over the same pixels.
        state.pan_pixels(-60.0, -40.0, &p, ms(now, 400));
        let moved = sharp(&state).expect("still sharp while panning");
        assert_eq!((moved.x - first.x, moved.y - first.y), (60, 40));
        assert!(state.poll(None, &p, ms(now, 500)).is_empty(), "nothing to render for a small move");
        // Dragging on towards the end of the margin makes a new render before the window leaves the old one, and the old one stays
        // on screen until it is there.
        // The worker needs a moment: the next drag step is made while it renders, then its picture arrives.
        let mut job = None;
        let mut renders = 0;
        for step in 0..12 {
            state.pan_pixels(-40.0, 0.0, &p, ms(now, 600 + step));
            assert!(sharp(&state).is_some(), "sharp at step {step}");
            if let Some(done) = job.take().map(|j: RenderJob| j.run()) {
                state.rendered(done, &p);
                renders += 1;
            }
            let next = state.poll(None, &p, ms(now, 600 + step)).into_iter().find_map(|r| match r {
                Request::Render(job) => Some(job),
                Request::LoadSource => None,
            });
            job = job.or(next);
        }
        assert!(renders >= 1, "a new render starts before the window runs out of the old one");
    }

    #[test]
    fn a_failed_render_is_not_tried_again_for_the_same_view() {
        let (mut state, now) = state();
        state.zoom_to(0.5, None, &prefs(), now);
        let display = buffer(1000, 750);
        let job = only_render(state.poll(Some(&display), &prefs(), ms(now, 100)));
        state.rendered(RenderDone { generation: job.generation, key: job.key, region: IRect::default(), pixels: Err("broken".into()) }, &prefs());
        assert!(state.poll(Some(&display), &prefs(), ms(now, 300)).is_empty());
        // A different view tries again.
        state.zoom_to(0.4, None, &prefs(), ms(now, 400));
        assert!(!state.poll(Some(&display), &prefs(), ms(now, 600)).is_empty());
    }

    #[test]
    fn a_wrong_sized_original_is_refused() {
        let (mut state, now) = state();
        state.zoom_to(2.0, None, &prefs(), now);
        state.source_loaded(Ok(Arc::new(SourceImage::new(10, 10, buffer(10, 10)).unwrap())));
        assert!(state.failed().is_some());
        state.source_loaded(Err("gone".to_string()));
        assert_eq!(state.failed(), Some("gone"));
    }

    #[test]
    fn a_reset_goes_back_to_the_plain_picture_and_lets_go_of_the_original_later() {
        let (mut state, now) = state();
        let source = Arc::new(SourceImage::new(4000, 3000, buffer(4000, 3000)).unwrap());
        state.zoom_to(4.0, Some((200.0, 200.0)), &prefs(), now);
        state.source_loaded(Ok(source));
        state.reset(&prefs(), ms(now, 500));
        assert!(!state.is_active());
        assert!(state.picture(Some((1000, 750)), &prefs()).is_none());
        // After the release time the original is gone: a new zoom asks for it again.
        state.poll(None, &prefs(), ms(now, 500 + 9_000));
        state.zoom_to(2.0, None, &prefs(), ms(now, 20_000));
        assert!(state.poll(Some(&buffer(1000, 750)), &prefs(), ms(now, 20_000)).iter().any(|r| matches!(r, Request::LoadSource)));
    }

    #[test]
    fn a_zoom_back_to_exactly_100_percent_is_the_fit() {
        let (mut state, now) = state();
        state.zoom_to(2.0, Some((900.0, 100.0)), &prefs(), now);
        state.zoom_to(1.0, Some((100.0, 100.0)), &prefs(), ms(now, 10));
        assert!(!state.is_active(), "{:?}", state.target);
        assert_eq!(state.target, ZoomView::FIT);
    }

    #[test]
    fn panning_moves_the_view_without_animating_and_stops_at_the_edge() {
        let mut p = prefs();
        p.smooth = true;
        let (mut state, now) = state();
        state.zoom_to(2.0, None, &p, now);
        state.poll(None, &p, ms(now, 1000));
        state.pan_pixels(100.0, 0.0, &p, ms(now, 1100));
        assert!(!state.animating());
        let vp = state.viewport_norm().unwrap();
        assert!(vp.x < 0.25);
        state.pan_pixels(99999.0, 99999.0, &p, ms(now, 1200));
        let vp = state.viewport_norm().unwrap();
        assert!(vp.x.abs() < 1e-9 && vp.y.abs() < 1e-9);
        // Nothing to pan at 100%.
        let (mut plain, now) = self::state();
        plain.pan_pixels(50.0, 50.0, &p, now);
        assert!(!plain.is_active());
    }

    #[test]
    fn a_smooth_zoom_glides_and_ends_exactly_at_the_target() {
        let mut p = prefs();
        p.smooth = true;
        let (mut state, now) = state();
        state.zoom_to(4.0, None, &p, now);
        assert!(state.animating());
        state.poll(None, &p, ms(now, 80));
        let mid = state.shown.zoom;
        assert!(mid > 1.0 && mid < 4.0, "{mid}");
        // No render while it glides.
        assert!(state.poll(Some(&buffer(1000, 750)), &p, ms(now, 90)).iter().all(|r| !matches!(r, Request::Render(_))));
        state.poll(None, &p, ms(now, 400));
        assert!(!state.animating());
        assert!((state.shown.zoom - 4.0).abs() < 1e-9);
        // The preview during the glide is the intermediate view.
        let (mut state, now) = self::state();
        state.zoom_to(4.0, None, &p, now);
        state.poll(None, &p, ms(now, 80));
        assert!(state.picture(Some((1000, 750)), &p).is_some());
    }

    #[test]
    fn without_smoothing_a_zoom_is_instant() {
        let (mut state, now) = state();
        state.zoom_to(4.0, None, &prefs(), now);
        assert!(!state.animating());
        assert!((state.shown.zoom - 4.0).abs() < 1e-9);
    }

    #[test]
    fn the_wheel_steps_follow_the_ladder_and_respect_the_limits() {
        let (mut state, now) = state();
        state.zoom_step(true, 0.10, None, &prefs(), now);
        assert!((state.zoom() - 1.10).abs() < 1e-9);
        state.zoom_step(false, 0.10, None, &prefs(), now);
        assert!(!state.is_active());
        for _ in 0..200 {
            state.zoom_step(true, 0.25, None, &prefs(), now);
        }
        assert!((state.zoom() - prefs().limits().max).abs() < 1e-9);
    }

    #[test]
    fn the_minimap_shows_when_zoomed_in_and_wanted_and_goes_away() {
        let (mut state, now) = state();
        let p = prefs();
        assert!(!state.minimap_shown(&p, now), "not at 100%");
        state.zoom_to(2.0, None, &p, now);
        assert!(state.minimap_shown(&p, ms(now, 100)), "right after a zoom");
        assert!(!state.minimap_shown(&p, ms(now, 3000)), "a few seconds later it is gone");
        state.set_minimap_hover(true);
        assert!(state.minimap_shown(&p, ms(now, 3000)), "the pointer brings it back");
        state.set_minimap_hover(false);
        let mut always = p;
        always.minimap_always = true;
        always.minimap_corner = MinimapCorner::BottomRight;
        assert!(state.minimap_shown(&always, ms(now, 9000)));
        // Zoomed out there is nothing to move: never.
        state.zoom_to(0.5, None, &always, ms(now, 9100));
        assert!(!state.minimap_shown(&always, ms(now, 9100)));
    }

    #[test]
    fn dragging_the_minimap_moves_the_view_and_keeps_it_inside_the_image() {
        let (mut state, now) = state();
        let p = prefs();
        state.zoom_to(4.0, None, &p, now);
        // Press in the middle of the rectangle: it is grabbed where it is.
        state.minimap_press(0.5, 0.5, &p, now);
        assert!(state.minimap_dragging());
        assert!(state.minimap_shown(&p, ms(now, 60_000)), "visible while dragged");
        state.minimap_move(0.8, 0.5, &p, ms(now, 10));
        let vp = state.viewport_norm().unwrap();
        assert!((vp.x + vp.w / 2.0 - 0.8).abs() < 1e-9, "{vp:?}");
        state.minimap_move(5.0, -5.0, &p, ms(now, 20));
        let vp = state.viewport_norm().unwrap();
        assert!((vp.right() - 1.0).abs() < 1e-9 && vp.y.abs() < 1e-9);
        state.minimap_release();
        assert!(!state.minimap_dragging());
        // A press outside the rectangle jumps there.
        state.minimap_press(0.1, 0.1, &p, ms(now, 30));
        let vp = state.viewport_norm().unwrap();
        assert!(vp.x.abs() < 1e-9 && vp.y.abs() < 1e-9);
    }

    #[test]
    fn a_slow_drag_of_the_minimap_is_finer_the_further_in_the_picture_is_and_a_flick_is_not() {
        let mm = (200.0, 150.0);
        let p = prefs();
        let slow_drag = |zoom: f64| {
            let (mut state, now) = state();
            state.zoom_to(zoom, None, &p, now);
            let before = state.viewport_norm().unwrap();
            let start = (before.x + before.w / 2.0, before.y + before.h / 2.0);
            state.minimap_press(start.0, start.1, &p, ms(now, 10));
            // The pointer crawls 20 minimap pixels to the right (2 px every 60 ms).
            for i in 1..=10 {
                state.minimap_drag_by(2.0, 0.0, mm, &p, ms(now, 10 + 60 * i as u64));
            }
            let after = state.viewport_norm().unwrap();
            (after.x + after.w / 2.0) - start.0
        };
        let (near, mid, far) = (slow_drag(1.5), slow_drag(5.0), slow_drag(10.0));
        let pointer = 20.0 / 200.0;
        assert!(near > mid && mid > far && far > 0.0, "{near} {mid} {far}");
        assert!((near - pointer).abs() < 0.2 * pointer, "barely zoomed in: nearly 1:1 ({near})");
        assert!(far < 0.2 * pointer, "far in: much finer ({far})");
        // The same distance as a flick (20 px in 8 ms) is followed 1:1, so one can still get across.
        let (mut state, now) = state();
        state.zoom_to(10.0, None, &p, now);
        let vp = state.viewport_norm().unwrap();
        let start = (vp.x + vp.w / 2.0, vp.y + vp.h / 2.0);
        state.minimap_press(start.0, start.1, &p, ms(now, 10));
        for i in 1..=10 {
            state.minimap_drag_by(2.0, 0.0, mm, &p, ms(now, 10 + i as u64));
        }
        let vp = state.viewport_norm().unwrap();
        assert!((vp.x + vp.w / 2.0 - start.0 - pointer).abs() < 0.15 * pointer, "flick");
    }

    #[test]
    fn a_locked_drag_goes_on_for_as_long_as_the_pointer_moves_and_stops_at_the_edge() {
        let (mut state, now) = state();
        let p = prefs();
        let mm = (200.0, 150.0);
        state.zoom_to(4.0, None, &p, now);
        state.minimap_press(0.5, 0.5, &p, now);
        // Far more movement than the minimap is wide: the view ends at the edge, not beyond it.
        for i in 1..=60 {
            state.minimap_drag_by(10.0, 0.0, mm, &p, ms(now, 20 * i as u64));
        }
        assert!((state.viewport_norm().unwrap().right() - 1.0).abs() < 1e-9);
        // Coming back moves the view at once (nothing is owed for the way beyond the edge).
        state.minimap_drag_by(-5.0, 0.0, mm, &p, ms(now, 20 * 61));
        assert!(state.viewport_norm().unwrap().right() < 1.0 - 1e-6);
        // Without a press nothing happens.
        state.minimap_release();
        let before = state.viewport_norm().unwrap();
        state.minimap_drag_by(-50.0, 0.0, mm, &p, ms(now, 20 * 62));
        assert_eq!(state.viewport_norm().unwrap(), before);
    }

    #[test]
    fn a_resized_tile_keeps_the_view_and_asks_for_a_new_render() {
        let (mut state, now) = state();
        let p = prefs();
        state.zoom_to(3.0, Some((300.0, 300.0)), &p, now);
        let before = state.target;
        assert!(state.set_tile((500, 375), &p));
        assert!((state.target.zoom - before.zoom).abs() < 1e-9);
        assert!((state.target.cx - before.cx).abs() < 1e-9);
        assert!(!state.set_tile((500, 375), &p), "the same size is no change");
        assert!(state.hq.is_none());
    }

    #[test]
    fn the_thumbnail_is_made_once() {
        let (mut state, _) = state();
        state.ensure_thumb(&buffer(1000, 750));
        let first = state.thumb().unwrap().as_bytes().as_ptr();
        state.ensure_thumb(&buffer(1000, 750));
        assert_eq!(state.thumb().unwrap().as_bytes().as_ptr(), first);
        assert!(state.thumb().unwrap().width() <= 480);
    }

    #[test]
    fn a_display_source_render_maps_the_rectangle_into_the_downscaled_picture() {
        let (mut state, now) = state();
        let p = prefs();
        let display = buffer(1000, 750);
        state.zoom_to(0.5, None, &p, now);
        let job = only_render(state.poll(Some(&display), &p, ms(now, 100)));
        // The whole image is visible: the source rectangle is the whole display picture.
        assert!((job.visible.src.w - 1000.0).abs() < 2.0 && (job.visible.src.h - 750.0).abs() < 3.0, "{:?}", job.visible.src);
        assert_eq!(job.visible.dest, IRect { x: 0, y: 0, w: 500, h: 375 }, "the whole image, in the pixels of the zoomed image");
        let done = job.run();
        let pixels = done.pixels.unwrap();
        assert_eq!((pixels.width() as i32, pixels.height() as i32), (500, 375));
    }
}
