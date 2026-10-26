//! The video tiles of the tab grid (UI thread only): while the grid is shown, a tick looks where the pointer is, shows the
//! small control bar of the video under it (`video_bar`: a model per Multiview, drawn by the canvas of its grid item) and keeps
//! the time badges of every other video up to date (`time_badges`, another model per Multiview). The tick runs only while the grid is shown and stops itself when it is not.

use super::time_badges::TimeBadgeModel;
use super::video_bar::VideoBarModel;
use crate::ui::{MainWindow, MultiviewTimeBadgeView, MultiviewVideoBarView};
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::route::HostMode;
use crate::viewer::multiview::video_bar::prefs::{self, BarStyle, VideoBarPrefs};
use crate::viewer::multiview::video_bar::view::VideoBarInput;
use crate::viewer::{PointerProbe, ViewerTabManager, get_slint_window_xid};
use rustview_core::settings::schema::{MULTIVIEW_TICK_MS, MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode};
use std::cell::RefCell;
use std::collections::HashMap;
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

fn tick() {
    let Some((window, pointer)) = GRID.with(|cell| {
        let grid = cell.borrow();
        let grid = grid.as_ref()?;
        let window = grid.window.upgrade()?;
        let pointer = grid.probe.as_ref().and_then(|p| p.position_in(grid.main_xid));
        Some((window, pointer))
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
    update(pointer);
}

/// Shows the small bars for `pointer` and syncs the badges of every video in the grid.
fn update(pointer: Option<(i32, i32)>) {
    let prefs = mini_prefs();
    let now = Instant::now();
    let mut shown = Vec::new();
    for instance in grid_instances() {
        let mut guard = instance.lock();
        let targets = guard.grid_video_bar_targets(pointer, &prefs, now);
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
        let pointer = GRID.with(|cell| {
            let grid = cell.borrow();
            let grid = grid.as_ref()?;
            Some(grid.probe.as_ref().and_then(|p| p.position_in(grid.main_xid)))
        });
        if let Some(pointer) = pointer {
            update(pointer);
        }
    }
}

/// The grid went: its small bars and badges hide.
fn stop() {
    GRID.with(|cell| {
        if let Some(grid) = cell.borrow().as_ref() {
            grid.timer.stop();
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
