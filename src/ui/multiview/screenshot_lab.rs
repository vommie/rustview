//! Headless screenshots of the fullscreen Multiview (test scaffolding).
//!
//! The real `MultiviewFullscreenWindow` is rendered with Slint's software renderer; the tiles come from a real
//! `MultiviewInstance` whose images are decoded from generated files exactly like in the application. Pointer events are
//! injected into the window, the clock of the platform is controlled by the test, so animations (button feedback, tooltip
//! delay) are deterministic. Native video windows cannot be drawn here; their geometry is covered by the layout tests.
//! Set `RUSTVIEW_SCREENSHOT_DIR` to keep the PNGs; the assertions run either way.

use super::views::{ImageCache, ViewModels};
use crate::ui::MultiviewFullscreenWindow;
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::actions::ActionId;
use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
use crate::viewer::multiview::follow::Wanted;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::model::TileKind;
use crate::viewer::multiview::route::UiAction;
use crate::viewer::multiview::tile::TileSeed;
use parking_lot::Mutex;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, PlatformError, Rgb8Pixel, SharedPixelBuffer};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PALETTE: [[u8; 3]; 6] = [
    [226, 74, 74],
    [74, 178, 98],
    [74, 118, 226],
    [226, 196, 56],
    [186, 88, 218],
    [56, 196, 196],
];
const BACKGROUND: [u8; 3] = [0x11, 0x11, 0x1b];

thread_local! {
    static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
    static CLOCK: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

struct LabPlatform;

impl Platform for LabPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        WINDOW.with(|slot| *slot.borrow_mut() = Some(window.clone()));
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        CLOCK.with(Cell::get)
    }
}

/// Sink that keeps the events of decode workers until the lab hands them to the instance.
#[derive(Default)]
struct CollectSink(Mutex<Vec<(WeakInstance, PlayerEvent)>>);

impl MultiviewEventSink for CollectSink {
    fn post(&self, instance: WeakInstance, event: PlayerEvent) {
        self.0.lock().push((instance, event));
    }
}

pub struct Shot {
    pub width: usize,
    pub pixels: SharedPixelBuffer<Rgb8Pixel>,
}

impl Shot {
    pub fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let p = self.pixels.as_slice()[y as usize * self.width + x as usize];
        [p.r, p.g, p.b]
    }

    /// True if the pixel is (almost) the given color.
    pub fn is(&self, x: i32, y: i32, color: [u8; 3]) -> bool {
        self.at(x, y)
            .iter()
            .zip(color)
            .all(|(&a, b)| (i32::from(a) - i32::from(b)).abs() <= 6)
    }

    pub fn same_as(&self, other: &Shot) -> bool {
        self.pixels.as_bytes() == other.pixels.as_bytes()
    }
}

pub fn color_of(id: u64) -> [u8; 3] {
    PALETTE[(id as usize) % PALETTE.len()]
}

pub fn background() -> [u8; 3] {
    BACKGROUND
}

struct Media {
    path: PathBuf,
    width: u32,
    height: u32,
}

pub struct Lab {
    win: MultiviewFullscreenWindow,
    adapter: Rc<MinimalSoftwareWindow>,
    sink: Arc<CollectSink>,
    pub instance: SharedInstance,
    models: ViewModels,
    images: ImageCache,
    media: HashMap<u64, Media>,
    pub size: (u32, u32),
    shots: Option<PathBuf>,
    /// Actions the window asked the application to run (the lab plays the session and executes the instance part).
    pub requested: Rc<RefCell<Vec<UiAction>>>,
    /// Where the window drew the hover bar, and the tooltip bubble (logical = physical pixels at scale factor 1).
    pub overlay: Rc<Cell<Option<Rect>>>,
    pub tooltip: Rc<Cell<Option<Rect>>>,
}

/// Installs the platform once per thread; a second call (same thread) keeps the first.
fn install_platform() {
    let _ = slint::platform::set_platform(Box::new(LabPlatform));
}

fn write_test_image(path: &std::path::Path, id: u64, width: u32, height: u32) {
    let [r, g, b] = color_of(id);
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        let border = x < 4 || y < 4 || x + 4 >= width || y + 4 >= height;
        if border {
            image::Rgb([255, 255, 255])
        } else {
            let shade = 1.0 - 0.0 * (f64::from(y) / f64::from(height));
            image::Rgb([
                (f64::from(r) * shade) as u8,
                (f64::from(g) * shade) as u8,
                (f64::from(b) * shade) as u8,
            ])
        }
    });
    image.save(path).expect("the test image can be written");
}

impl Lab {
    /// A window of `width` x `height` physical pixels at scale factor 1 and generated images `(id, width, height)`.
    pub fn new(width: u32, height: u32, media: &[(u64, u32, u32)]) -> Self {
        install_platform();
        let win = MultiviewFullscreenWindow::new().expect("the Multiview window can be created");
        let adapter = WINDOW
            .with(|slot| slot.borrow().clone())
            .expect("the platform created a window adapter");
        adapter.set_size(PhysicalSize::new(width, height));

        let dir = std::env::temp_dir().join(format!("rustview-lab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let media = media
            .iter()
            .map(|&(id, w, h)| {
                let path = dir.join(format!("{id}.png"));
                write_test_image(&path, id, w, h);
                (
                    id,
                    Media {
                        path,
                        width: w,
                        height: h,
                    },
                )
            })
            .collect();

        let sink = Arc::new(CollectSink::default());
        let instance = MultiviewInstance::create(sink.clone(), Vec::new());
        instance
            .lock()
            .set_host(&HostSurface::fullscreen(0, width as i32, height as i32, 1.0));
        let models = ViewModels::default();
        win.set_tiles(ModelRc::from(models.tiles.clone()));
        win.set_gutters(ModelRc::from(models.gutters.clone()));
        win.set_bar_actions(ModelRc::from(models.bar_actions.clone()));
        win.set_is_ready(true);
        win.set_background_mode(0);

        let requested = Rc::new(RefCell::new(Vec::new()));
        let overlay = Rc::new(Cell::new(None));
        let tooltip = Rc::new(Cell::new(None));
        {
            let (for_actions, requested) = (instance.clone(), requested.clone());
            win.on_action_clicked(move |tile, action| {
                let Some(action) = ActionId::from_code(action) else {
                    return;
                };
                let mut guard = for_actions.lock();
                let Some(id) = usize::try_from(tile)
                    .ok()
                    .and_then(|i| guard.member_ids().get(i).copied())
                else {
                    return;
                };
                let outcome = guard.perform_action(id, action);
                requested.borrow_mut().push(outcome);
            });
            let (for_overlay, overlay) = (instance.clone(), overlay.clone());
            win.on_overlay_changed(move |visible, x, y, w, h| {
                let rect = visible.then(|| Rect::new(x as i32, y as i32, w as i32, h as i32));
                overlay.set(rect);
                for_overlay.lock().set_bar_overlay(rect);
            });
            let tooltip = tooltip.clone();
            win.on_tooltip_changed(move |visible, x, y, w, h| {
                tooltip.set(visible.then(|| Rect::new(x as i32, y as i32, w as i32, h as i32)));
            });
        }
        win.show().expect("the window can be shown");

        let shots = std::env::var_os("RUSTVIEW_SCREENSHOT_DIR").map(PathBuf::from);
        if let Some(dir) = &shots {
            std::fs::create_dir_all(dir).expect("the screenshot directory");
        }
        Self {
            win,
            adapter,
            sink,
            instance,
            models,
            images: ImageCache::default(),
            media,
            size: (width, height),
            shots,
            requested,
            overlay,
            tooltip,
        }
    }

    fn seed(&self, id: u64) -> TileSeed {
        let media = &self.media[&id];
        TileSeed {
            media_id: id,
            kind: TileKind::Image,
            aspect: media.width as f32 / media.height as f32,
            path: media.path.clone(),
            filename: format!("{id}.png"),
        }
    }

    /// The selection became `ids` (view order): the Multiview follows it like it does for a user selection.
    pub fn follow(&mut self, ids: &[u64]) {
        let wanted: Vec<Wanted> = ids
            .iter()
            .map(|&id| Wanted {
                id,
                is_video: false,
            })
            .collect();
        let plan = self.instance.lock().plan_follow(&wanted);
        let seeds = plan.add.iter().map(|&id| (id, self.seed(id))).collect();
        self.instance.lock().apply_follow(&plan, seeds);
        self.settle();
    }

    /// Waits for the decode workers and shows what they produced.
    pub fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let events: Vec<_> = std::mem::take(&mut *self.sink.0.lock());
            for (weak, event) in events {
                if let (Some(instance), PlayerEvent::ImageDecoded { media_id, result }) =
                    (weak.upgrade(), event)
                {
                    instance.lock().on_image_loaded(media_id, result);
                }
            }
            let loading = self.instance.lock().is_loading();
            if !loading || Instant::now() > deadline {
                assert!(!loading, "the images did not finish decoding");
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.refresh();
    }

    pub fn refresh(&mut self) {
        let view = self.instance.lock().view();
        self.models.sync(&view, &mut self.images);
        self.win.set_bar_tile(super::views::bar_tile_index(&view));
    }

    /// Moves the pointer: the window sees it (tooltips) and the instance probes it (hovered tile, hover bar).
    pub fn pointer(&mut self, x: i32, y: i32) {
        self.adapter
            .window()
            .dispatch_event(WindowEvent::PointerMoved {
                position: LogicalPosition::new(x as f32, y as f32),
            });
        self.instance.lock().tick(|| Some((x, y)));
        self.refresh();
        self.advance(Duration::from_millis(20));
    }

    pub fn click(&mut self, x: i32, y: i32) {
        let position = LogicalPosition::new(x as f32, y as f32);
        let window = self.adapter.window();
        window.dispatch_event(WindowEvent::PointerMoved { position });
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        self.refresh();
    }

    pub fn advance(&mut self, by: Duration) {
        CLOCK.with(|clock| clock.set(clock.get() + by));
        slint::platform::update_timers_and_animations();
    }

    /// Renders the window after letting 100 ms of animations run and optionally writes `<name>.png`.
    pub fn shot(&mut self, name: &str) -> Shot {
        self.advance(Duration::from_millis(100));
        self.shot_now(name)
    }

    /// Renders the window at the current time of the platform clock.
    pub fn shot_now(&mut self, name: &str) -> Shot {
        let (width, height) = self.size;
        let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
        self.adapter.request_redraw();
        self.adapter.draw_if_needed(|renderer| {
            renderer.render(pixels.make_mut_slice(), width as usize);
        });
        let shot = Shot {
            width: width as usize,
            pixels,
        };
        if let Some(dir) = &self.shots {
            let image = image::RgbImage::from_raw(width, height, shot.pixels.as_bytes().to_vec())
                .expect("the buffer matches the size");
            image
                .save(dir.join(format!("{name}.png")))
                .expect("the screenshot can be written");
        }
        shot
    }

    pub fn center_of(&self, tile: usize) -> (i32, i32) {
        let rect = self.instance.lock().tile_rect(tile);
        (rect.x + rect.w / 2, rect.y + rect.h / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 1280;
    const H: u32 = 720;
    const MEDIA: [(u64, u32, u32); 7] = [
        (1, 400, 300),
        (2, 640, 360),
        (3, 360, 640),
        (4, 640, 360),
        (5, 500, 500),
        (6, 640, 360),
        (7, 640, 360),
    ];

    fn paints_tile(shot: &Shot, lab: &Lab, tile: usize, id: u64) {
        let (x, y) = lab.center_of(tile);
        assert!(
            shot.is(x, y, color_of(id)),
            "tile {tile} should show medium {id}, got {:?} at ({x}, {y})",
            shot.at(x, y)
        );
    }

    #[test]
    fn media_come_and_go_and_the_arrangement_follows() {
        let mut lab = Lab::new(W, H, &MEDIA);

        lab.follow(&[1]);
        let one = lab.shot("01-one-image-fills-the-display");
        paints_tile(&one, &lab, 0, 1);
        let rect = lab.instance.lock().tile_rect(0);
        assert_eq!((rect.h, rect.w), (720, 960), "a 4:3 image is fitted into 16:9");
        assert!(one.is(40, 360, background()), "the rest is background");

        lab.follow(&[1, 2]);
        let two = lab.shot("02-two-images-side-by-side");
        paints_tile(&two, &lab, 0, 1);
        paints_tile(&two, &lab, 1, 2);

        lab.follow(&[1, 2, 3]);
        let three = lab.shot("03-three-images");
        for (tile, id) in [(0, 1), (1, 2), (2, 3)] {
            paints_tile(&three, &lab, tile, id);
        }

        lab.follow(&[1, 2, 3, 4]);
        let four = lab.shot("04-four-images");
        for (tile, id) in [(0, 1), (1, 2), (2, 3), (3, 4)] {
            paints_tile(&four, &lab, tile, id);
        }

        lab.follow(&[1, 3, 4]);
        let removed = lab.shot("05-the-second-image-was-deselected");
        assert_eq!(lab.instance.lock().member_ids(), vec![1, 3, 4]);
        for (tile, id) in [(0, 1), (1, 3), (2, 4)] {
            paints_tile(&removed, &lab, tile, id);
        }

        lab.follow(&[3]);
        let back = lab.shot("06-back-to-one-image");
        paints_tile(&back, &lab, 0, 3);
        let rect = lab.instance.lock().tile_rect(0);
        assert_eq!((rect.w, rect.h), (405, 720), "a portrait image is fitted by height");
    }

    #[test]
    fn four_widescreen_images_form_a_grid_and_leave_it_again() {
        let mut lab = Lab::new(W, H, &MEDIA);
        lab.follow(&[2, 4, 6]);
        assert_eq!(lab.instance.lock().row_count(), 1);
        lab.follow(&[2, 4, 6, 7]);
        let grid = lab.shot("07-four-widescreen-images-form-a-grid");
        assert_eq!(lab.instance.lock().row_count(), 2);
        for (tile, id) in [(0, 2), (1, 4), (2, 6), (3, 7)] {
            paints_tile(&grid, &lab, tile, id);
        }
        lab.follow(&[2, 4, 7]);
        let row = lab.shot("08-one-image-less-the-row-is-back");
        assert_eq!(lab.instance.lock().row_count(), 1);
        for (tile, id) in [(0, 2), (1, 4), (2, 7)] {
            paints_tile(&row, &lab, tile, id);
        }
    }

    #[test]
    fn a_selection_that_did_not_change_leaves_the_picture_untouched() {
        let mut lab = Lab::new(W, H, &MEDIA);
        lab.follow(&[1, 2]);
        let before = lab.shot("09-before-the-folder-changed");
        lab.follow(&[]);
        lab.follow(&[1, 2]);
        let after = lab.shot("10-after-a-folder-change-and-a-query");
        assert!(before.same_as(&after), "an empty or unchanged wish redraws nothing");
        lab.follow(&[2]);
        assert_eq!(lab.instance.lock().member_ids(), vec![2]);
    }

    fn column_of(lab: &Lab, tile: usize) -> i32 {
        lab.instance.lock().tile_rect(tile).w
    }

    #[test]
    fn a_dragged_width_survives_when_media_come_and_go_and_the_rest_adapts() {
        let mut lab = Lab::new(W, H, &MEDIA);
        lab.follow(&[2, 4, 6]);
        let automatic = lab.shot("11-three-widescreen-images-automatic");
        let widths: Vec<i32> = (0..3).map(|t| column_of(&lab, t)).collect();
        assert!(widths.windows(2).all(|w| (w[0] - w[1]).abs() <= 2), "equal without customization");
        paints_tile(&automatic, &lab, 0, 2);

        {
            let mut mv = lab.instance.lock();
            mv.drag_started(0);
            mv.dragged(160.0);
            mv.drag_finished();
        }
        lab.refresh();
        let dragged = lab.shot("12-the-first-gutter-was-dragged");
        let custom = column_of(&lab, 0);
        assert!(custom > widths[0] + 100, "{custom} vs {}", widths[0]);
        paints_tile(&dragged, &lab, 0, 2);

        let (id, _) = (2, 0);
        lab.instance.lock().perform_action(id, ActionId::Sticky);
        assert!(lab.instance.lock().is_sticky(id));
        lab.follow(&[5, 7]);
        let sticky = lab.shot("13-sticky-keeps-its-width-and-the-newcomers-share-the-rest");
        let members = lab.instance.lock().member_ids();
        assert_eq!(members, vec![2, 5, 7]);
        assert_eq!(column_of(&lab, 0), custom, "the sticky tile keeps the width the user gave it");
        paints_tile(&sticky, &lab, 0, 2);
        paints_tile(&sticky, &lab, 1, 5);
        paints_tile(&sticky, &lab, 2, 7);

        lab.follow(&[1]);
        let two = lab.shot("14-the-selection-moved-on-the-sticky-image-stays");
        assert_eq!(lab.instance.lock().member_ids(), vec![2, 1]);
        paints_tile(&two, &lab, 0, 2);
        paints_tile(&two, &lab, 1, 1);

        lab.instance.lock().perform_action(2, ActionId::Sticky);
        lab.follow(&[3]);
        assert_eq!(
            lab.instance.lock().member_ids(),
            vec![3],
            "without stickiness the selection replaces everything"
        );
        let _ = lab.shot("15-released-and-replaced");
    }

    #[test]
    fn the_action_bar_floats_over_the_hovered_tile_and_its_buttons_work() {
        let mut lab = Lab::new(W, H, &MEDIA);
        lab.follow(&[2, 4]);
        let (hx, hy) = lab.center_of(1);
        lab.pointer(hx, hy);
        lab.pointer(hx + 2, hy);
        let shot = lab.shot("16-action-bar-on-the-hovered-tile");
        let tile = lab.instance.lock().tile_rect(1);
        let bar = lab.overlay.get().expect("the window reports the bar");
        assert_eq!(bar.y, tile.y + 8, "the bar sits in the top right corner");
        assert_eq!(bar.x + bar.w, tile.x + tile.w - 8);
        assert_eq!(bar.w, 3 * 28 + 2 * 2 + 6, "sticky, open as tab and remove");
        assert!(!shot.is(bar.x + 3, bar.y + 3, color_of(4)), "the bar covers the image");

        let buttons: Vec<(i32, i32)> = (0..3)
            .map(|i| (bar.x + 3 + i * 30 + 14, bar.y + 3 + 14))
            .collect();

        lab.click(buttons[0].0, buttons[0].1);
        assert!(lab.instance.lock().is_sticky(4), "the sticky button toggled the tile");
        let sticky = lab.shot("17-sticky-is-on");
        assert!(!sticky.same_as(&shot));

        lab.click(buttons[1].0, buttons[1].1);
        assert_eq!(lab.requested.borrow().last(), Some(&UiAction::OpenTileAsTab(4)));
        lab.instance.lock().action_finished(4, ActionId::OpenAsTab, true);
        lab.refresh();
        lab.advance(Duration::from_millis(1));
        lab.advance(Duration::from_millis(60));
        let flash = lab.shot_now("18-open-as-tab-feedback-success");
        assert!(
            flash.is(buttons[1].0 - 10, buttons[1].1 - 10, [0xa6, 0xe3, 0xa1]) || flash.at(buttons[1].0 - 10, buttons[1].1 - 10)[1] > 150,
            "the button flashes green: {:?}",
            flash.at(buttons[1].0 - 10, buttons[1].1 - 10)
        );
        lab.advance(Duration::from_millis(250));
        let _ = lab.shot_now("19-feedback-fading");
        lab.advance(Duration::from_millis(900));
        let gone = lab.shot_now("20-feedback-gone");
        assert!(!gone.is(buttons[1].0 - 10, buttons[1].1 - 10, [0xa6, 0xe3, 0xa1]));

        lab.instance.lock().action_finished(4, ActionId::OpenAsTab, false);
        lab.refresh();
        lab.advance(Duration::from_millis(1));
        lab.advance(Duration::from_millis(60));
        let failed = lab.shot_now("21-open-as-tab-feedback-error");
        let px = failed.at(buttons[1].0 - 10, buttons[1].1 - 10);
        assert!(px[0] > 180 && px[1] < 160, "the button flashes red: {px:?}");
    }

    #[test]
    fn hovering_a_button_shows_its_tooltip_above_everything() {
        let mut lab = Lab::new(W, H, &MEDIA);
        lab.follow(&[2, 4]);
        let (hx, hy) = lab.center_of(0);
        lab.pointer(hx, hy);
        lab.pointer(hx + 2, hy);
        lab.shot("22-bar-before-the-tooltip");
        let bar = lab.overlay.get().expect("the bar is shown");
        let open_tab = (bar.x + 3 + 30 + 14, bar.y + 3 + 14);
        lab.pointer(open_tab.0, open_tab.1);
        assert!(lab.tooltip.get().is_none(), "the tooltip waits for its delay");
        lab.advance(Duration::from_millis(900));
        lab.refresh();
        lab.advance(Duration::from_millis(50));
        let shot = lab.shot("23-tooltip-of-the-open-as-tab-button");
        let bubble = lab.tooltip.get().expect("the tooltip bubble was reported");
        assert!(bubble.y >= bar.y + bar.h, "the bubble is below the bar: {bubble:?} vs {bar:?}");
        assert!(!shot.is(bubble.x + 6, bubble.y + 6, color_of(2)), "the bubble covers the image");
    }

    #[test]
    fn the_last_remaining_medium_offers_close_instead_of_remove() {
        let mut lab = Lab::new(W, H, &MEDIA);
        lab.follow(&[2, 4]);
        let (hx, hy) = lab.center_of(1);
        lab.pointer(hx, hy);
        lab.pointer(hx + 2, hy);
        lab.shot("24-two-images-with-remove");
        let bar = lab.overlay.get().expect("the bar is shown");
        let remove = (bar.x + 3 + 2 * 30 + 14, bar.y + 3 + 14);
        lab.click(remove.0, remove.1);
        assert_eq!(lab.requested.borrow().last(), Some(&UiAction::DeselectInBrowse(4)));
        assert_eq!(lab.instance.lock().member_ids(), vec![2]);
        lab.refresh();
        let (hx, hy) = lab.center_of(0);
        lab.pointer(hx, hy);
        lab.pointer(hx + 2, hy);
        let last = lab.shot("25-the-last-medium-has-a-close-button");
        let bar = lab.overlay.get().expect("the bar is shown on the single image");
        let close = (bar.x + 3 + 2 * 30 + 14, bar.y + 3 + 14);
        assert!(last.at(close.0, close.1) != color_of(2));
        lab.click(close.0, close.1);
        assert_eq!(lab.requested.borrow().last(), Some(&UiAction::CloseFullscreen));
        assert_eq!(lab.instance.lock().member_ids(), vec![2], "closing is the application's job");
    }
}
