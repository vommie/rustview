//! The video tiles of the tab grid (UI thread only): while the grid is shown, a tick looks where the pointer is, shows the
//! small control bar of the video under it (`video_bar`, owner `TabGrid`) and keeps the time badges of every other video up to
//! date. The tick runs only while the grid is shown and stops itself when it is not.

use super::bar_overlay::BarOwner;
use super::video_bar::{self, VideoBarHost};
use crate::ui::MainWindow;
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::route::HostMode;
use crate::viewer::multiview::video_bar::prefs::{BarStyle, VideoBarPrefs};
use crate::viewer::multiview::video_bar::view::VideoBarInput;
use crate::viewer::{PointerProbe, ViewerTabManager, get_slint_window_xid};
use rustview_core::settings::schema::{MULTIVIEW_TICK_MS, MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

struct GridBars {
    window: slint::Weak<MainWindow>,
    tabs: Weak<ViewerTabManager>,
    probe: Option<PointerProbe>,
    main_xid: u64,
    timer: Timer,
}

thread_local! {
    static GRID: RefCell<Option<GridBars>> = const { RefCell::new(None) };
}

/// The grid is shown (or was synced): the tick runs from now on until the grid goes.
pub fn ensure_running(window: &MainWindow, tabs: &Arc<ViewerTabManager>) {
    let Some(main_xid) = get_slint_window_xid(window.window()) else {
        return;
    };
    GRID.with(|cell| {
        let mut grid = cell.borrow_mut();
        let state = grid.get_or_insert_with(|| GridBars {
            window: window.as_weak(),
            tabs: Arc::downgrade(tabs),
            probe: PointerProbe::new()
                .map_err(
                    |e| tracing::warn!(error = %e, "[Multiview: GridBars] No pointer probe; no small bars in the grid"),
                )
                .ok(),
            main_xid,
            timer: Timer::default(),
        });
        state.main_xid = main_xid;
        if !state.timer.running() {
            state
                .timer
                .start(TimerMode::Repeated, Duration::from_millis(MULTIVIEW_TICK_MS), tick);
            tracing::debug!("[Multiview: GridBars] Started");
        }
    });
}

/// The preferences of the small bars: the user's opacity and fade, a fixed height, always a bar.
fn mini_prefs() -> VideoBarPrefs {
    let mut prefs = video_bar::effective_prefs();
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

fn tick() {
    let Some((window, pointer, main_xid)) = GRID.with(|cell| {
        let grid = cell.borrow();
        let grid = grid.as_ref()?;
        let window = grid.window.upgrade()?;
        let pointer = grid.probe.as_ref().and_then(|p| p.position_in(grid.main_xid));
        Some((window, pointer, grid.main_xid))
    }) else {
        stop();
        return;
    };
    if !window.get_is_tab_grid_active() {
        stop();
        return;
    }
    // While a dialog locks the main window no bar shows (the badges stay).
    let pointer = pointer.filter(|_| !window.get_is_modal_locked());
    update(pointer, main_xid);
}

/// Shows the small bars for `pointer` and syncs the badges of every video in the grid.
fn update(pointer: Option<(i32, i32)>, main_xid: u64) {
    let prefs = mini_prefs();
    let now = Instant::now();
    let mut targets = Vec::new();
    for instance in grid_instances() {
        let mut guard = instance.lock();
        let found = guard.grid_video_bar_targets(pointer, &prefs, now);
        let bar_keys: Vec<TileKey> = found.iter().map(|t| t.key).collect();
        guard.sync_time_badges(&bar_keys);
        targets.extend(found);
    }
    let host = VideoBarHost {
        owner: BarOwner::TabGrid,
        host_xid: main_xid,
        on_input: Rc::new(on_input),
    };
    video_bar::update(&host, &targets, &prefs);
}

/// A click, drag or wheel on the small bar of tile `key` (tile keys are unique across all Multiviews).
fn on_input(key: TileKey, input: VideoBarInput) {
    let prefs = mini_prefs();
    let changed = grid_instances()
        .into_iter()
        .find(|instance| instance.lock().has_tile(key))
        .is_some_and(|instance| instance.lock().video_bar_input(key, input, &prefs));
    if changed {
        let target = GRID.with(|cell| {
            let grid = cell.borrow();
            let grid = grid.as_ref()?;
            Some((
                grid.probe.as_ref().and_then(|p| p.position_in(grid.main_xid)),
                grid.main_xid,
            ))
        });
        if let Some((pointer, main_xid)) = target {
            update(pointer, main_xid);
        }
    }
}

/// The grid went: its small bars hide; the badges go with the next change of each Multiview's host.
fn stop() {
    GRID.with(|cell| {
        if let Some(grid) = cell.borrow().as_ref() {
            grid.timer.stop();
        }
    });
    for instance in grid_instances() {
        instance.lock().release_video_bars();
    }
    video_bar::hide(BarOwner::TabGrid);
    tracing::debug!("[Multiview: GridBars] Stopped");
}
