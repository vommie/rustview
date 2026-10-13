//! The running Multiview: one persistent Slint window plus the session state, both living on the UI thread.
//!
//! Every event (Slint callback, timer, finished image decode) goes through [`with_session`] / [`run`], which serializes
//! access to the thread-local session and skips re-entrant calls instead of panicking. The behavior is split over
//! `session_layout` (geometry, gutters), `session_tiles` (tile lifecycle, live selection), `session_sound` (mute state)
//! and `session_hover` (pointer, mute badge, aspect probing); this file owns the lifecycle: open, close, window wiring.

use super::audio::{self, TileAudio};
use super::eligibility::OpenPlan;
use super::handle::{self, MultiviewHandle};
use super::layout::{Layout, LayoutParams, Rect, Split};
use super::session_layout::DragState;
use super::tiles::{TileRuntime, VideoTile, teardown_videos};
use super::views;
use crate::library::models::MediaId;
use crate::settings::SettingsManager;
use crate::settings::schema::{
    MULTIVIEW_GUTTER_LOGICAL_PX, MULTIVIEW_MIN_COLUMN_LOGICAL_PX, MULTIVIEW_OPEN_POLL_MS,
    MULTIVIEW_OPEN_XID_ATTEMPTS, MULTIVIEW_SELECTION_DEBOUNCE_MS, MULTIVIEW_TICK_MS,
    MULTIVIEW_WRAP_MIN_ASPECT,
};
use crate::system::{MonitorInfo, SystemDisplayService};
use crate::ui::bridge_state::UiState;
use crate::ui::{MainWindow, MultiviewFullscreenWindow, MultiviewGutterView, MultiviewTileView};
use crate::viewer::{PointerProbe, force_x11_fullscreen_on_monitor, get_raw_window_xid};
use parking_lot::RwLock;
use slint::{
    CloseRequestResponse, ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, Timer,
    TimerMode, VecModel,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

thread_local! {
    static WINDOW: RefCell<Option<MultiviewFullscreenWindow>> = const { RefCell::new(None) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// What to do after an event handler returns (closing needs the session borrow to be released first).
pub(super) enum After {
    Nothing,
    Close,
}

pub(super) struct Session {
    pub(super) generation: u64,
    pub(super) handle: MultiviewHandle,
    pub(super) window: slint::Weak<MultiviewFullscreenWindow>,
    pub(super) state: Arc<RwLock<UiState>>,
    pub(super) monitor: MonitorInfo,
    pub(super) fs_xid: u64,
    pub(super) scale: f32,
    pub(super) params: LayoutParams,
    pub(super) tiles: Vec<TileRuntime>,
    pub(super) split: Split,
    pub(super) layout: Layout,
    pub(super) drag: Option<DragState>,
    pub(super) hovered: Option<usize>,
    pub(super) last_hovered: Option<MediaId>,
    /// Tile index and window rectangle of the visible mute badge.
    pub(super) badge: Option<(usize, Rect)>,
    pub(super) hole_pending: bool,
    pub(super) probe: Option<PointerProbe>,
    pub(super) tile_model: Rc<VecModel<MultiviewTileView>>,
    pub(super) gutter_model: Rc<VecModel<MultiviewGutterView>>,
    pub(super) debounce: Timer,
    pub(super) tick: Timer,
    pub(super) last_click: Option<(Instant, bool)>,
    pub(super) populated: bool,
    pub(super) ticks: u32,
    pending: Option<OpenPlan>,
}

/// Runs `f` on the session if there is one. Re-entrant calls are skipped instead of panicking.
pub(super) fn run(f: impl FnOnce(&mut Session) -> After) {
    let after = SESSION.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => {
            tracing::debug!("[Multiview: Session] Skipped a re-entrant event");
            None
        }
    });
    if matches!(after, Some(After::Close)) {
        close(true);
    }
}

pub(super) fn with_session(f: impl FnOnce(&mut Session)) {
    run(|s| {
        f(s);
        After::Nothing
    });
}

fn session_generation() -> Option<u64> {
    SESSION.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.generation))
    })
}

/// Selection hub hook (debounced): the live selection is reconciled once the selection has been quiet for a moment.
pub(super) fn schedule_reconcile() {
    with_session(|s| {
        if s.populated {
            s.debounce.start(
                TimerMode::SingleShot,
                Duration::from_millis(MULTIVIEW_SELECTION_DEBOUNCE_MS),
                || with_session(Session::reconcile_now),
            );
        }
    });
}

fn background_mode_index(mode: &str) -> i32 {
    match mode.to_lowercase().as_str() {
        "deep black" | "black" => 1,
        "transparent checkerboard" | "checkerboard" => 2,
        _ => 0,
    }
}

fn create_window() -> Result<MultiviewFullscreenWindow, slint::PlatformError> {
    let win = MultiviewFullscreenWindow::new()?;
    win.on_request_close(|| close(true));
    win.on_request_rename(|| with_session(|s| s.rename_hovered()));
    win.on_tile_clicked(|_, is_ctrl| run(|s| s.on_click(is_ctrl)));
    win.on_background_clicked(|is_ctrl| run(|s| s.on_click(is_ctrl)));
    win.on_middle_clicked(|| close(true));
    win.on_gutter_drag_started(|index| with_session(|s| s.drag_started(index.max(0) as usize)));
    win.on_gutter_dragged(|_, distance| with_session(|s| s.dragged(distance)));
    win.on_gutter_drag_finished(|_| with_session(|s| s.drag = None));
    win.on_gutter_reset(|| with_session(Session::reset_split));
    win.on_gutter_swap(|index| with_session(|s| s.swap(index.max(0) as usize)));
    win.on_badge_clicked(|| with_session(Session::toggle_badge_mute));
    win.on_size_changed(|| with_session(Session::on_size_changed));
    win.window().on_close_requested(|| {
        close(true);
        CloseRequestResponse::HideWindow
    });
    Ok(win)
}

/// The persistent window (created on first use); Slint component handles are not `Clone`, so callers upgrade the weak handle.
fn window_weak() -> Option<slint::Weak<MultiviewFullscreenWindow>> {
    WINDOW.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            match create_window() {
                Ok(win) => *slot = Some(win),
                Err(e) => tracing::error!(error = %e, "[Multiview: Window] Failed to create the Slint window"),
            }
        }
        slot.as_ref().map(ComponentHandle::as_weak)
    })
}

fn resolve_monitor(
    settings: &SettingsManager,
    main_window: &slint::Weak<MainWindow>,
) -> MonitorInfo {
    let main_geom = main_window.upgrade().map(|w| {
        let pos = w.window().position();
        let size = w.window().size();
        (pos.x, pos.y, size.width, size.height)
    });
    SystemDisplayService::resolve_target_monitor(&settings.get().fullscreen, main_geom)
}

fn prepare_window(
    win: &MultiviewFullscreenWindow,
    settings: &SettingsManager,
    monitor: &MonitorInfo,
    tile_model: &Rc<VecModel<MultiviewTileView>>,
    gutter_model: &Rc<VecModel<MultiviewGutterView>>,
) {
    win.set_is_ready(false);
    win.set_background_mode(background_mode_index(
        &settings.get().viewer.background_mode,
    ));
    win.set_tiles(ModelRc::from(tile_model.clone()));
    win.set_gutters(ModelRc::from(gutter_model.clone()));
    win.set_badge(views::badge_view(None, false, 1.0));
    win.window()
        .set_position(PhysicalPosition::new(monitor.x, monitor.y));
    win.window()
        .set_size(PhysicalSize::new(monitor.width, monitor.height));
}

/// Opens the Multiview with the tiles of `plan`. A Multiview that is already open is replaced. UI thread only.
pub(super) fn open(
    handle: MultiviewHandle,
    plan: OpenPlan,
    settings: Arc<SettingsManager>,
    state: Arc<RwLock<UiState>>,
    main_window: slint::Weak<MainWindow>,
) -> bool {
    if session_generation().is_some() {
        close(false);
    }
    let Some(window) = window_weak() else {
        return false;
    };
    let Some(win) = window.upgrade() else {
        return false;
    };

    let monitor = resolve_monitor(&settings, &main_window);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let tile_count = plan.tiles.len();
    let session = Session::new(generation, handle, window, state, monitor, plan);
    prepare_window(
        &win,
        &settings,
        &session.monitor,
        &session.tile_model,
        &session.gutter_model,
    );
    if SESSION.with(|cell| {
        cell.try_borrow_mut()
            .map(|mut g| *g = Some(session))
            .is_err()
    }) {
        tracing::error!("[Multiview: Open] The session slot is busy; not opening");
        return false;
    }
    handle::set_open(true);

    if let Some(mw) = main_window.upgrade() {
        mw.set_is_modal_locked(true);
    }
    if let Err(e) = win.show() {
        tracing::error!(error = %e, "[Multiview: Open] Failed to show the window");
        close(true);
        return false;
    }
    Timer::single_shot(Duration::from_millis(MULTIVIEW_OPEN_POLL_MS), move || {
        finish_open(generation, 1)
    });
    tracing::info!(
        generation,
        tiles = tile_count,
        "[Multiview: Open] Window shown; waiting for the native handle"
    );
    true
}

/// Waits for the native window handle (it appears shortly after `show()`), then builds the tiles.
fn finish_open(generation: u64, attempt: u32) {
    if session_generation() != Some(generation) {
        return;
    }
    let xid = window_weak()
        .and_then(|w| w.upgrade())
        .and_then(|w| get_raw_window_xid(w.window()))
        .unwrap_or(0);
    if xid == 0 {
        if attempt >= MULTIVIEW_OPEN_XID_ATTEMPTS {
            tracing::error!(
                attempt,
                "[Multiview: Open] The window never received a native handle; closing"
            );
            close(true);
        } else {
            Timer::single_shot(Duration::from_millis(MULTIVIEW_OPEN_POLL_MS), move || {
                finish_open(generation, attempt + 1)
            });
        }
        return;
    }
    with_session(|s| s.populate(xid));
}

/// Closes the Multiview. With `notify` the application's close handler runs (re-syncs the Browse view); without it another
/// fullscreen is replacing this one. Safe to call when nothing is open.
pub(super) fn close(notify: bool) {
    let taken = SESSION.with(|cell| cell.try_borrow_mut().map(|mut g| g.take()));
    let session = match taken {
        Ok(Some(session)) => session,
        Ok(None) => return,
        Err(_) => {
            let _ = slint::invoke_from_event_loop(move || close(notify));
            return;
        }
    };
    handle::set_open(false);
    let handle = session.handle.clone();
    session.shutdown();
    tracing::info!(notify, "[Multiview: Close] Multiview closed");
    if notify {
        handle.notify_closed();
    }
}

impl Session {
    fn new(
        generation: u64,
        handle: MultiviewHandle,
        window: slint::Weak<MultiviewFullscreenWindow>,
        state: Arc<RwLock<UiState>>,
        monitor: MonitorInfo,
        plan: OpenPlan,
    ) -> Self {
        let params = LayoutParams {
            width: monitor.width.max(1) as i32,
            height: monitor.height.max(1) as i32,
            gutter: MULTIVIEW_GUTTER_LOGICAL_PX.round() as i32,
            min_column: MULTIVIEW_MIN_COLUMN_LOGICAL_PX.round() as i32,
            min_wide_aspect: MULTIVIEW_WRAP_MIN_ASPECT,
        };
        Self {
            generation,
            handle,
            window,
            state,
            monitor,
            fs_xid: 0,
            scale: 1.0,
            params,
            tiles: Vec::new(),
            split: Split::Auto,
            layout: Layout::default(),
            drag: None,
            hovered: None,
            last_hovered: None,
            badge: None,
            hole_pending: false,
            probe: None,
            tile_model: Rc::new(VecModel::default()),
            gutter_model: Rc::new(VecModel::default()),
            debounce: Timer::default(),
            tick: Timer::default(),
            last_click: None,
            populated: false,
            ticks: 0,
            pending: Some(plan),
        }
    }

    /// Builds the tiles once the native window exists: positions it on the target monitor, lays the tiles out, starts
    /// every video (same recipe as the single quick fullscreen) and decodes the images in the background.
    pub(super) fn populate(&mut self, xid: u64) {
        let Some(plan) = self.pending.take() else {
            return;
        };
        self.fs_xid = xid;
        let mon = &self.monitor;
        force_x11_fullscreen_on_monitor(xid, mon.index, mon.x, mon.y, mon.width, mon.height);
        self.refresh_geometry();

        for planned in plan.tiles {
            let tile = self.new_tile(planned);
            self.tiles.push(tile);
        }
        let mut slots: Vec<Option<TileAudio>> = self.tiles.iter().map(|t| t.audio).collect();
        audio::assign_on_open(&mut slots);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }

        self.apply_layout();
        for index in 0..self.tiles.len() {
            self.start_tile(index);
        }
        self.apply_audio();

        self.probe = match PointerProbe::new() {
            Ok(probe) => Some(probe),
            Err(e) => {
                tracing::warn!(error = %e, "[Multiview: Hover] No pointer probe; the mute badge is disabled");
                None
            }
        };
        self.populated = true;
        if let Some(win) = self.window.upgrade() {
            win.set_is_ready(true);
        }
        self.tick.start(
            TimerMode::Repeated,
            Duration::from_millis(MULTIVIEW_TICK_MS),
            || with_session(Session::on_tick),
        );
        tracing::info!(
            generation = self.generation,
            tiles = self.tiles.len(),
            width = self.params.width,
            height = self.params.height,
            scale = self.scale,
            monitor = self.monitor.index,
            "[Multiview: Open] Tiles created"
        );
    }

    fn shutdown(mut self) {
        self.debounce.stop();
        self.tick.stop();
        let videos: Vec<VideoTile> = self
            .tiles
            .iter_mut()
            .filter_map(|t| t.video.take())
            .collect();
        if let Some(win) = self.window.upgrade() {
            win.set_is_ready(false);
            let _ = win.hide();
        }
        teardown_videos(videos);
    }
}
