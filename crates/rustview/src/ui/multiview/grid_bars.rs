//! The video tiles of the tab grid (UI thread only): the small control bar of the video under the pointer (`video_bar`: a model
//! per Multiview, drawn by the canvas of its grid item) and the time badges of every other video (`time_badges`, another model per
//! Multiview). The pointer comes from the main window's own events (`pointer_events`, forwarded by the tab host), a scroll of the
//! grid moves it over the content; a tick keeps the badges' times and the bars' fade going while the grid is shown and a video in
//! it plays or a bar fades, and stops when nothing moves or the grid goes (`active_changed`).

use super::time_badges::TimeBadgeModel;
use super::video_bar::VideoBarModel;
use super::wake_timer::WakeTimer;
use crate::ui::tabs::grid::canvas::visible_grid;
use crate::ui::{MainWindow, MultiviewTimeBadgeView, MultiviewVideoBarView};
use crate::viewer::ViewerTabManager;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::instance_tick::TICK;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::route::HostMode;
use crate::viewer::multiview::video_bar::prefs::{self, BarStyle, VideoBarPrefs};
use crate::viewer::multiview::video_bar::view::VideoBarInput;
use rustview_core::settings::schema::MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX;
use rustview_core::util::cast::FloatToInt;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

struct GridBars {
    window: slint::Weak<MainWindow>,
    tabs: Weak<ViewerTabManager>,
    /// The grid is shown: pointer movements and scrolling update the bars.
    active: bool,
    timer: WakeTimer,
    /// Brings the bars in line with the pointer once per turn of the event loop, however many movements arrived in it.
    pointer_sync: Timer,
}

thread_local! {
    /// The pointer over the main window (physical window pixels), from its pointer events; `None` outside it.
    static POINTER: std::cell::Cell<Option<(i32, i32)>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    static GRID: RefCell<Option<GridBars>> = const { RefCell::new(None) };
    /// The small bars of every Multiview of the grid, by instance id (its grid item draws them).
    static BARS: RefCell<HashMap<u64, VideoBarModel>> = RefCell::new(HashMap::new());
    /// The time badges of every Multiview of the grid, by instance id.
    static BADGES: RefCell<HashMap<u64, TimeBadgeModel>> = RefCell::new(HashMap::new());
}

/// The bars of the Multiview `instance_id` for its grid item (the same model for as long as the Multiview exists).
pub fn model_for(instance_id: u64) -> ModelRc<MultiviewVideoBarView> {
    BARS.with(|bars| bars.borrow_mut().entry(instance_id).or_default().model())
}

/// The time badges of the Multiview `instance_id` for its grid item (the same model for as long as the Multiview exists).
pub fn badges_for(instance_id: u64) -> ModelRc<MultiviewTimeBadgeView> {
    BADGES.with(|badges| badges.borrow_mut().entry(instance_id).or_default().model())
}

/// The grid is shown (or was synced): the bars and badges follow it from now on until it goes.
pub fn ensure_running(window: &MainWindow, tabs: &Arc<ViewerTabManager>) {
    let started = GRID.with(|cell| {
        let mut grid = cell.borrow_mut();
        let state = grid.get_or_insert_with(|| GridBars {
            window: window.as_weak(),
            tabs: Arc::downgrade(tabs),
            active: false,
            timer: WakeTimer::default(),
            pointer_sync: Timer::default(),
        });
        !std::mem::replace(&mut state.active, true)
    });
    if started {
        tracing::debug!("[Multiview: GridBars] Started");
    }
    request_update();
}

/// The tab grid was shown or left (`is-tab-grid-active` of the main window): leaving it hides its bars and badges at once.
pub fn active_changed() {
    let shown = GRID
        .with(|cell| cell.borrow().as_ref().and_then(|g| g.window.upgrade()))
        .is_some_and(|window| window.get_is_tab_grid_active());
    if shown {
        request_update();
    } else if GRID.with(|cell| cell.borrow().as_ref().is_some_and(|g| g.active)) {
        stop();
    }
}

/// The preferences of the small bars: the user's opacity and fade, a fixed height, always a bar.
fn mini_prefs() -> VideoBarPrefs {
    let mut prefs = prefs::current();
    prefs.style = BarStyle::Bar;
    prefs.height = MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX;
    prefs
}

/// The Multiviews the grid shows.
fn grid_instances() -> Vec<SharedInstance> {
    let tabs = GRID.with(|cell| cell.borrow().as_ref().and_then(|g| g.tabs.upgrade()));
    tabs.map(|tabs| tabs.instances())
        .unwrap_or_default()
        .into_iter()
        .filter(|instance| instance.lock().host_mode() == HostMode::TabGrid)
        .collect()
}

/// The pointer moved over the main window or left it: the small bars follow on this turn of the event loop (only while the grid
/// is shown).
pub fn on_pointer(pointer: Option<(i32, i32)>) {
    POINTER.set(pointer);
    request_update();
}

/// The grid scrolled: the content moved under the resting pointer.
pub fn on_scrolled() {
    request_update();
}

fn request_update() {
    GRID.with(|cell| {
        if let Some(grid) = cell.borrow().as_ref()
            && grid.active
            && !grid.pointer_sync.running()
        {
            grid.pointer_sync.start(TimerMode::SingleShot, Duration::ZERO, tick);
        }
    });
}

fn tick() {
    let Some(window) = GRID.with(|cell| cell.borrow().as_ref().and_then(|grid| grid.window.upgrade())) else {
        stop();
        return;
    };
    if !window.get_is_tab_grid_active() {
        stop();
        return;
    }
    // While a dialog locks the main window no bar shows (the badges stay).
    let pointer = POINTER.get().filter(|_| !window.get_is_modal_locked());
    update(&window, pointer);
    rearm();
}

/// Arms the tick while something in the grid moves by itself: the time badge of a playing video, a bar that fades.
fn rearm() {
    let now = Instant::now();
    let deadline = grid_instances()
        .iter()
        .filter_map(|instance| {
            let guard = instance.lock();
            if guard.has_playing_video() {
                Some(now + TICK)
            } else {
                guard.tick_deadline(now)
            }
        })
        .min();
    GRID.with(|cell| {
        if let Some(grid) = cell.borrow_mut().as_mut() {
            grid.timer.arm(deadline, now, on_timer);
        }
    });
}

fn on_timer() {
    GRID.with(|cell| {
        if let Some(grid) = cell.borrow_mut().as_mut() {
            grid.timer.fired();
        }
    });
    tick();
}

/// The visible part of the grid and `pointer` in grid content coordinates (physical pixels; the Multiviews of the grid are placed
/// at scroll position 0).
fn content_view(window: &MainWindow, pointer: Option<(i32, i32)>) -> (Rect, Option<(i32, i32)>) {
    let scale = window.window().scale_factor();
    let size = window.window().size();
    // `tab-grid-scroll-y` is the viewport's offset: 0 at the top, negative further down.
    let offset = (window.get_tab_grid_scroll_y() * scale).round().to_int::<i32>();
    to_content(visible_grid(size.width, size.height, scale), offset, pointer)
}

/// `visible` (window pixels) and `pointer` moved into the content of a grid scrolled by `offset`. A pointer outside the visible
/// part (over a toolbar or the status bar) is over no item, even where an item lies scrolled below it.
fn to_content(visible: Rect, offset: i32, pointer: Option<(i32, i32)>) -> (Rect, Option<(i32, i32)>) {
    let pointer = pointer
        .filter(|&(x, y)| visible.contains(x, y))
        .map(|(x, y)| (x, y - offset));
    (Rect::new(visible.x, visible.y - offset, visible.w, visible.h), pointer)
}

/// Shows the small bars for `pointer` (window coordinates) and syncs the badges of every video in the grid.
fn update(window: &MainWindow, pointer: Option<(i32, i32)>) {
    let (visible, pointer) = content_view(window, pointer);
    let prefs = mini_prefs();
    let now = Instant::now();
    let mut shown = Vec::new();
    for instance in grid_instances() {
        let mut guard = instance.lock();
        let targets = guard.grid_video_bar_targets(pointer, visible, &prefs, now);
        let bar_keys: Vec<_> = targets.iter().map(|t| t.key).collect();
        let badges = guard.grid_time_badges(&bar_keys);
        let (id, dpi) = (guard.id(), guard.host_dpi());
        drop(guard);
        shown.push(id);
        BADGES.with(|models| models.borrow_mut().entry(id).or_default().sync(&badges, dpi));
        BARS.with(|bars| {
            let mut bars = bars.borrow_mut();
            let model = bars.entry(id).or_default();
            if targets.is_empty() {
                model.clear();
            } else {
                model.sync(&targets, &prefs);
            }
        });
    }
    // A Multiview that left the grid (or closed) keeps no bar and no badge.
    BADGES.with(|models| {
        models.borrow_mut().retain(|id, model| {
            if !shown.contains(id) {
                model.clear();
            }
            shown.contains(id)
        });
    });
    BARS.with(|bars| {
        bars.borrow_mut().retain(|id, model| {
            if !shown.contains(id) {
                model.clear();
            }
            shown.contains(id)
        });
    });
}

/// A click, drag or wheel on the small bar that reported `key` (tile keys are unique across all Multiviews).
pub(super) fn on_input(key: i32, input: VideoBarInput) {
    let Some((instance_id, tile)) = BARS.with(|bars| {
        bars.borrow()
            .iter()
            .find_map(|(id, model)| model.tile(key).map(|tile| (*id, tile)))
    }) else {
        return;
    };
    let prefs = mini_prefs();
    let changed = grid_instances()
        .into_iter()
        .find(|instance| instance.lock().id() == instance_id)
        .is_some_and(|instance| instance.lock().video_bar_input(tile, input, &prefs));
    if changed {
        tick();
    }
}

/// The grid went: its small bars and badges hide.
fn stop() {
    GRID.with(|cell| {
        if let Some(grid) = cell.borrow_mut().as_mut() {
            grid.active = false;
            grid.timer.stop();
            grid.pointer_sync.stop();
        }
    });
    for instance in grid_instances() {
        instance.lock().release_video_bars();
    }
    BARS.with(|bars| {
        for model in bars.borrow_mut().values_mut() {
            model.clear();
        }
    });
    BADGES.with(|models| {
        for model in models.borrow_mut().values_mut() {
            model.clear();
        }
    });
    tracing::debug!("[Multiview: GridBars] Stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scrolled_grid_moves_pointer_and_visible_part_into_the_content() {
        let visible = Rect::new(0, 80, 1600, 700);
        let (content, pointer) = to_content(visible, -300, Some((400, 200)));
        assert_eq!(content, Rect::new(0, 380, 1600, 700));
        assert_eq!(pointer, Some((400, 500)));
    }

    #[test]
    fn a_pointer_over_the_toolbar_is_over_no_item() {
        let visible = Rect::new(0, 80, 1600, 700);
        assert_eq!(to_content(visible, -300, Some((400, 40))).1, None);
        assert_eq!(to_content(visible, 0, None).1, None);
    }
}
