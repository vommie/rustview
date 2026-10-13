//! The running fullscreen Multiview: one persistent Slint window plus the session state, both living on the UI thread.
//!
//! The Multiview itself (tiles, arrangement, sound, interaction) is a `MultiviewInstance`; the session is its presenter in
//! the fullscreen window: it owns the Slint window and models, the timers and the pointer probe, and syncs the models
//! whenever the instance changed. Every event (Slint callback, timer, finished image decode) goes through [`run`] /
//! [`with_session`], which serialize access to the thread-local session and skip re-entrant calls instead of panicking.
//! Input handling lives in `session_input`; this file owns the lifecycle: open, close, window wiring.

use super::eligibility::OpenPlan;
use super::handle::{self, MultiviewHandle};
use super::router::{UiEventSink, execute};
use super::views::{self, ImageCache};
use crate::settings::SettingsManager;
use crate::settings::schema::{
    MULTIVIEW_OPEN_POLL_MS, MULTIVIEW_OPEN_XID_ATTEMPTS, MULTIVIEW_SELECTION_DEBOUNCE_MS,
    MULTIVIEW_TICK_MS,
};
use crate::system::{MonitorInfo, SystemDisplayService};
use crate::ui::bridge_state::UiState;
use crate::ui::{MainWindow, MultiviewFullscreenWindow, MultiviewGutterView, MultiviewTileView};
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance};
use crate::viewer::multiview::route::UiAction;
use crate::viewer::{PointerProbe, force_x11_fullscreen_on_monitor, get_raw_window_xid};
use parking_lot::RwLock;
use slint::{
    CloseRequestResponse, ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, Timer,
    TimerMode, VecModel,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

thread_local! {
    static WINDOW: RefCell<Option<MultiviewFullscreenWindow>> = const { RefCell::new(None) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

pub(super) struct Session {
    /// Id of the instance; timers that outlive a session compare it to notice that the session was replaced.
    pub(super) generation: u64,
    pub(super) handle: MultiviewHandle,
    pub(super) window: slint::Weak<MultiviewFullscreenWindow>,
    pub(super) state: Arc<RwLock<UiState>>,
    pub(super) monitor: MonitorInfo,
    pub(super) fs_xid: u64,
    pub(super) instance: SharedInstance,
    pub(super) probe: Option<PointerProbe>,
    pub(super) tile_model: Rc<VecModel<MultiviewTileView>>,
    pub(super) gutter_model: Rc<VecModel<MultiviewGutterView>>,
    pub(super) images: ImageCache,
    shown_revision: Option<u64>,
    pub(super) debounce: Timer,
    pub(super) tick: Timer,
    pub(super) populated: bool,
}

/// Runs `f` on the session if there is one. Re-entrant calls are skipped instead of panicking. The action `f` returns runs
/// after the session borrow is released, so it may close the session.
pub(super) fn run(f: impl FnOnce(&mut Session) -> UiAction) {
    let outcome = SESSION.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard
            .as_mut()
            .map(|session| (f(session), session.handle.clone())),
        Err(_) => {
            tracing::debug!("[Multiview: Session] Skipped a re-entrant event");
            None
        }
    });
    if let Some((action, handle)) = outcome {
        execute(action, &handle);
    }
}

pub(super) fn with_session(f: impl FnOnce(&mut Session)) {
    run(|s| {
        f(s);
        UiAction::None
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
    win.on_request_rename(|| run(Session::rename_hovered));
    win.on_tile_clicked(|_, is_ctrl| run(|s| s.on_click(is_ctrl)));
    win.on_background_clicked(|is_ctrl| run(|s| s.on_click(is_ctrl)));
    win.on_middle_clicked(|| close(true));
    win.on_gutter_drag_started(|index| with_session(|s| s.drag_started(index.max(0) as usize)));
    win.on_gutter_dragged(|_, distance| with_session(|s| s.dragged(distance)));
    win.on_gutter_drag_finished(|_| with_session(Session::drag_finished));
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
    win.set_badge(views::badge_view(None, 1.0));
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
    let tile_count = plan.tiles.len();
    let session = Session::new(handle, window, state, monitor, plan);
    let generation = session.generation;
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
    let session = match take_session() {
        Ok(Some(session)) => session,
        Ok(None) => return,
        Err(()) => {
            let _ = slint::invoke_from_event_loop(move || close(notify));
            return;
        }
    };
    finish_close(session, notify);
}

/// Takes the session out of its slot. `Err` means the slot is borrowed right now (an event handler is running).
pub(super) fn take_session() -> Result<Option<Session>, ()> {
    SESSION.with(|cell| {
        cell.try_borrow_mut()
            .map(|mut slot| slot.take())
            .map_err(|_| ())
    })
}

/// Shuts a session that was taken out of its slot down; its instance is destroyed with it unless somebody else holds it.
pub(super) fn finish_close(session: Session, notify: bool) {
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
        handle: MultiviewHandle,
        window: slint::Weak<MultiviewFullscreenWindow>,
        state: Arc<RwLock<UiState>>,
        monitor: MonitorInfo,
        plan: OpenPlan,
    ) -> Self {
        let instance =
            MultiviewInstance::create(Arc::new(UiEventSink::new(handle.clone())), plan.tiles);
        handle.bind_instance(&instance);
        let generation = instance.lock().id();
        Self {
            generation,
            handle,
            window,
            state,
            monitor,
            fs_xid: 0,
            instance,
            probe: None,
            tile_model: Rc::new(VecModel::default()),
            gutter_model: Rc::new(VecModel::default()),
            images: ImageCache::default(),
            shown_revision: None,
            debounce: Timer::default(),
            tick: Timer::default(),
            populated: false,
        }
    }

    /// Builds the tiles once the native window exists: positions it on the target monitor, lays the tiles out, starts
    /// every video (same recipe as the single quick fullscreen) and decodes the images in the background.
    pub(super) fn populate(&mut self, xid: u64) {
        if self.populated {
            return;
        }
        self.fs_xid = xid;
        let mon = &self.monitor;
        force_x11_fullscreen_on_monitor(xid, mon.index, mon.x, mon.y, mon.width, mon.height);
        let Some(surface) = self.host_surface() else {
            return;
        };
        {
            let mut instance = self.instance.lock();
            instance.set_host(&surface);
            instance.start_all();
        }

        self.probe = match PointerProbe::new() {
            Ok(probe) => Some(probe),
            Err(e) => {
                tracing::warn!(error = %e, "[Multiview: Hover] No pointer probe; the mute badge is disabled");
                None
            }
        };
        self.populated = true;
        self.refresh();
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
            monitor = self.monitor.index,
            "[Multiview: Open] Session populated"
        );
    }

    /// Brings the Slint models and the badge in line with the instance when it changed since the last sync.
    pub(super) fn refresh(&mut self) {
        let view = {
            let instance = self.instance.lock();
            if Some(instance.revision()) == self.shown_revision {
                return;
            }
            instance.view()
        };
        self.shown_revision = Some(view.revision);
        views::sync_models(
            &self.tile_model,
            &self.gutter_model,
            &view,
            &mut self.images,
        );
        if let Some(win) = self.window.upgrade() {
            win.set_badge(views::badge_view(view.badge.as_ref(), view.dpi));
        }
    }

    /// Hides the window and empties its models (the images would otherwise stay in memory until the next open); dropping
    /// the session then releases the instance, whose last reference shuts the players down before their windows are
    /// destroyed.
    pub(super) fn shutdown(self) {
        self.debounce.stop();
        self.tick.stop();
        if let Some(win) = self.window.upgrade() {
            win.set_is_ready(false);
            let _ = win.hide();
        }
        self.tile_model.set_vec(Vec::new());
        self.gutter_model.set_vec(Vec::new());
    }
}
