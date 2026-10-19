//! The running fullscreen Multiview: one persistent Slint window plus the session state, both living on the UI thread.
//!
//! The Multiview itself (tiles, arrangement, sound, interaction) is a `MultiviewInstance`; the session is its presenter in
//! the fullscreen window: it owns the Slint window and models, the timers and the pointer probe, and syncs the models
//! whenever the instance changed. Every event (Slint callback, timer, finished image decode) goes through [`run`] /
//! [`with_session`], which serialize access to the thread-local session and skip re-entrant calls instead of panicking.
//! Input handling lives in `session_input`; this file owns the lifecycle: open, close, window wiring.

use super::bar_overlay::{self, BarOwner};
use super::cursor;
use super::eligibility::OpenPlan;
use super::handle::{self, MultiviewHandle, Pace};
use super::router::{UiEventSink, execute};
use super::session_convert::park_below_main_window;
use super::tab_host;
use super::views::{self, ImageCache, ViewModels};
use crate::util::hex_color;
use crate::settings::SettingsManager;
use crate::settings::schema::{
    MULTIVIEW_OPEN_POLL_MS, MULTIVIEW_OPEN_XID_ATTEMPTS, MULTIVIEW_SELECTION_DEBOUNCE_MS,
    MULTIVIEW_TICK_MS,
};
use crate::system::{MonitorInfo, SystemDisplayService};
use crate::ui::bridge_state::UiState;
use crate::ui::MainWindow;
use crate::ui::MultiviewFullscreenWindow;
use crate::viewer::{TabId, ViewerTabManager};
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance};
use crate::viewer::multiview::route::UiAction;
use crate::viewer::{PointerProbe, force_x11_fullscreen_on_monitor, get_raw_window_xid};
use parking_lot::RwLock;
use slint::{
    CloseRequestResponse, Color, ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, Timer,
    TimerMode,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

thread_local! {
    static WINDOW: RefCell<Option<MultiviewFullscreenWindow>> = const { RefCell::new(None) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// Where a Multiview that was opened from a tab returns to when its fullscreen closes.
#[derive(Debug, Clone, Copy)]
pub(super) struct TabReturn {
    pub(super) tab_id: TabId,
    /// View mode of the main window at the time (1 = tab view, 2 = tab grid).
    pub(super) origin_mode: i32,
}

/// What a fullscreen session shows: a Multiview opened from the Browse selection, or the one a tab owns.
enum Source {
    Plan {
        plan: OpenPlan,
        state: Arc<RwLock<UiState>>,
    },
    Tab {
        instance: SharedInstance,
        tab: TabReturn,
    },
}

pub(super) struct Session {
    /// Id of the instance; timers that outlive a session compare it to notice that the session was replaced.
    pub(super) generation: u64,
    pub(super) handle: MultiviewHandle,
    pub(super) window: slint::Weak<MultiviewFullscreenWindow>,
    /// Source of the live selection; only a Multiview opened from the Browse view follows it.
    pub(super) state: Option<Arc<RwLock<UiState>>>,
    pub(super) monitor: MonitorInfo,
    pub(super) fs_xid: u64,
    pub(super) instance: SharedInstance,
    /// The tiles of the instance still have to be started (a Multiview that comes from a tab is already running).
    fresh: bool,
    pub(super) tab_return: Option<TabReturn>,
    pub(super) probe: Option<PointerProbe>,
    pub(super) models: ViewModels,
    /// What a click on a button of the action bar window does (the window is shared with the viewer tab host).
    pub(super) bar_click: Rc<dyn Fn(i32, i32)>,
    /// What a click on a button of the primary bar window does.
    pub(super) primary_click: Rc<dyn Fn(i32, i32)>,
    /// The mouse cursor is hidden over the window right now.
    pub(super) cursor_hidden: bool,
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
        execute(action, &handle, None);
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

/// A user action changed the Browse selection: the Multiview follows it, at once or once the selection is quiet.
pub(super) fn schedule_follow(pace: Pace) {
    with_session(|s| {
        if !s.populated {
            return;
        }
        match pace {
            Pace::Immediate => s.follow_now(),
            Pace::Settled => s.debounce.start(
                TimerMode::SingleShot,
                Duration::from_millis(MULTIVIEW_SELECTION_DEBOUNCE_MS),
                || with_session(Session::follow_now),
            ),
            Pace::After(delay) => s.debounce.start(TimerMode::SingleShot, delay, || with_session(Session::follow_now)),
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

/// Tells the window where its background comes from (theme, the viewer setting, or the custom color); the window resolves
/// the final color.
fn apply_background(win: &MultiviewFullscreenWindow, settings: &SettingsManager) {
    let current = settings.get();
    let background = &current.fullscreen.background;
    win.set_background_mode(background_mode_index(&current.viewer.background_mode));
    win.set_fill_source(match background.source.as_str() {
        "viewer" => 1,
        "custom" => 2,
        _ => 0,
    });
    let [r, g, b] = hex_color::parse_rgb(&background.color).unwrap_or([0x11, 0x11, 0x1b]);
    win.set_custom_fill(Color::from_rgb_u8(r, g, b));
    tracing::debug!(source = %background.source, "[Multiview: Background] Fullscreen background applied");
}

/// The color behind the media as mpv needs it: a single video covers the whole display, so mpv paints the bars around it.
/// `None` while the background is drawn by the viewer setting (Slint only), mpv keeps its black then.
fn video_background(win: &MultiviewFullscreenWindow) -> Option<[u8; 3]> {
    let fill = win.get_resolved_fill();
    (fill.alpha() > 0).then(|| [fill.red(), fill.green(), fill.blue()])
}

/// The settings changed: a fullscreen that is open follows the look it describes. The behavior settings (action bar,
/// sound) are read when they are needed, see `viewer::multiview::prefs`. UI thread only.
pub(super) fn refresh_settings(settings: &SettingsManager) {
    with_session(|s| {
        let Some(win) = s.window.upgrade() else {
            return;
        };
        apply_background(&win, settings);
        s.instance.lock().set_video_background(video_background(&win));
    });
}

fn create_window() -> Result<MultiviewFullscreenWindow, slint::PlatformError> {
    let win = MultiviewFullscreenWindow::new()?;
    win.on_request_close(|| {
        close_by_user();
    });
    win.on_request_rename(|| run(Session::rename_hovered));
    win.on_request_delete(|| run(Session::delete_single));
    win.on_tile_clicked(|_, is_ctrl| run(|s| s.on_click(is_ctrl)));
    win.on_background_clicked(|is_ctrl| run(|s| s.on_click(is_ctrl)));
    win.on_middle_clicked(|| {
        close_by_user();
    });
    win.on_navigate(|forward| run(|_| UiAction::Navigate { forward }));
    win.on_wheel(|delta| {
        run(|_| match delta {
            d if d < 0.0 => UiAction::Navigate { forward: true },
            d if d > 0.0 => UiAction::Navigate { forward: false },
            _ => UiAction::None,
        })
    });
    win.on_gutter_drag_started(|index| with_session(|s| s.drag_started(index.max(0) as usize)));
    win.on_gutter_dragged(|_, distance| with_session(|s| s.dragged(distance)));
    win.on_gutter_drag_finished(|_| with_session(Session::drag_finished));
    win.on_gutter_reset(|| with_session(Session::reset_split));
    win.on_gutter_swap(|index| with_session(|s| s.swap(index.max(0) as usize)));
    win.on_size_changed(|| with_session(Session::on_size_changed));
    win.window().on_close_requested(|| {
        if close_by_user() {
            CloseRequestResponse::HideWindow
        } else {
            CloseRequestResponse::KeepWindowShown
        }
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
    models: &ViewModels,
) {
    win.set_is_ready(false);
    apply_background(win, settings);
    win.set_tiles(ModelRc::from(models.tiles.clone()));
    win.set_gutters(ModelRc::from(models.gutters.clone()));
    win.set_navigation(0);
    let appearance = &settings.get().appearance;
    bar_overlay::set_tooltip_settings(appearance.enable_tooltips, appearance.tooltip_delay_ms as i32);
    bar_overlay::prepare();
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
    launch(handle, Source::Plan { plan, state }, &settings, main_window)
}

/// Shows the Multiview that a tab owns in the fullscreen window. The running players move over, nothing restarts; closing
/// the fullscreen hands the windows back to the tab. UI thread only.
pub(super) fn open_tab(
    handle: MultiviewHandle,
    instance: SharedInstance,
    tab: TabReturn,
    settings: Arc<SettingsManager>,
    main_window: slint::Weak<MainWindow>,
) -> bool {
    tab_host::suspend();
    super::tab_plain::suspend();
    instance.lock().deactivate();
    launch(
        handle,
        Source::Tab { instance, tab },
        &settings,
        main_window,
    )
}

fn launch(
    handle: MultiviewHandle,
    source: Source,
    settings: &SettingsManager,
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

    let monitor = resolve_monitor(settings, &main_window);
    let isolated = matches!(source, Source::Tab { .. });
    let session = Session::new(handle, window, monitor, source);
    let generation = session.generation;
    let tile_count = session.instance.lock().tile_count();
    prepare_window(
        &win,
        settings,
        &session.monitor,
        &session.models,
    );
    session.instance.lock().set_video_background(video_background(&win));
    if SESSION.with(|cell| {
        cell.try_borrow_mut()
            .map(|mut g| *g = Some(session))
            .is_err()
    }) {
        tracing::error!("[Multiview: Open] The session slot is busy; not opening");
        return false;
    }
    handle::set_open(true);

    // Only a Multiview that belongs to a tab is isolated and locks the Browse view; one that follows the selection is a
    // second window next to it.
    if isolated && let Some(mw) = main_window.upgrade() {
        handle::set_isolated(true);
        mw.set_is_fullscreen_locked(true);
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

/// Whether the fullscreen refuses to close because it is pinned. A refusal makes the primary bar show itself and its pin
/// button blink (the instance does that); the next tick draws it.
pub(super) fn close_refused() -> bool {
    SESSION.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|slot| slot.as_ref().map(|s| s.instance.lock().refuse_close()))
            .unwrap_or(false)
    })
}

/// A close the user asked for (Esc, double or middle click, the window's close button, the close button of the bar). A
/// pinned fullscreen stays; returns whether it closed.
pub(super) fn close_by_user() -> bool {
    if close_refused() {
        return false;
    }
    close(true);
    true
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

/// The application exits: the fullscreen closes whatever the user pinned. The close handler does not run (the main window is
/// closing, so there is no view left to re-sync). The window is only hidden, never dropped here: the video windows of the tiles
/// are its children, and destroying it while a teardown thread still shuts their players down would end the process (libmpv's
/// Xlib error handler). Slint destroys hidden windows when the event loop ends, after `ViewerTabManager::shutdown` has waited
/// for the teardown threads. UI thread only.
pub(super) fn shutdown() {
    match take_session() {
        Ok(Some(session)) => finish_close(session, false),
        Ok(None) => {}
        Err(()) => tracing::warn!("[Shutdown: Windows] The fullscreen is busy with an event; it cannot be closed in order"),
    }
}

/// Closes the fullscreen if it shows a tab that no longer exists.
pub(super) fn close_if_tab_gone(tab_manager: &ViewerTabManager) {
    let gone = SESSION.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|slot| slot.as_ref().and_then(|s| s.tab_return))
            .is_some_and(|tab| tab_manager.get_tab(tab.tab_id).is_none())
    });
    if gone {
        tracing::info!("[Multiview: Close] The tab of the fullscreen is gone");
        close(true);
    }
}

/// Takes the session out of its slot. `Err` means the slot is borrowed right now (an event handler is running).
pub(super) fn take_session() -> Result<Option<Session>, ()> {
    SESSION.with(|cell| {
        cell.try_borrow_mut()
            .map(|mut slot| slot.take())
            .map_err(|_| ())
    })
}

/// Shuts a session that was taken out of its slot down. A Multiview that comes from a tab hands its windows back to the tab
/// first (while the fullscreen window still exists); any other instance is destroyed with the session unless a tab holds it.
pub(super) fn finish_close(session: Session, notify: bool) {
    handle::set_open(false);
    let handle = session.handle.clone();
    let tab_return = session.tab_return;
    if tab_return.is_some()
        && let Err(reason) = park_below_main_window(&session)
    {
        tracing::warn!(
            reason,
            "[Multiview: Close] The windows could not return to the tab"
        );
    }
    if let Some(tab) = tab_return {
        let alive = handle
            .tab_manager()
            .is_some_and(|tab_manager| tab_manager.get_tab(tab.tab_id).is_some());
        session.instance.lock().return_loans(alive);
    }
    session.shutdown();
    tracing::info!(notify, "[Multiview: Close] Multiview closed");
    if notify {
        handle.notify_closed(tab_return);
    }
}

impl Session {
    fn new(
        handle: MultiviewHandle,
        window: slint::Weak<MultiviewFullscreenWindow>,
        monitor: MonitorInfo,
        source: Source,
    ) -> Self {
        let (instance, state, tab_return) = match source {
            Source::Plan { plan, state } => {
                let sink = Arc::new(UiEventSink::new(handle.clone()));
                (
                    MultiviewInstance::create(sink, plan.tiles),
                    Some(state),
                    None,
                )
            }
            Source::Tab { instance, tab } => (instance, None, Some(tab)),
        };
        handle.bind_instance(&instance, tab_return.is_none());
        let generation = instance.lock().id();
        Self {
            generation,
            handle,
            window,
            state,
            monitor,
            fs_xid: 0,
            instance,
            fresh: tab_return.is_none(),
            tab_return,
            probe: None,
            models: ViewModels::default(),
            bar_click: Rc::new(|tile, action| run(|s| s.on_action(tile, action))),
            primary_click: Rc::new(|_, action| run(|s| s.on_primary_action(action))),
            cursor_hidden: false,
            images: ImageCache::default(),
            shown_revision: None,
            debounce: Timer::default(),
            tick: Timer::default(),
            populated: false,
        }
    }

    /// Builds the tiles once the native window exists: positions it on the target monitor, lays the tiles out, starts
    /// every video and decodes the images in the background.
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
            if self.fresh {
                instance.start_all();
            } else {
                instance.start_pending_images();
            }
        }

        self.probe = match PointerProbe::new() {
            Ok(probe) => Some(probe),
            Err(e) => {
                tracing::warn!(error = %e, "[Multiview: Hover] No pointer probe; the hover bar is disabled");
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
        self.models.sync(&view, &mut self.images);
        if let Some(win) = self.window.upgrade() {
            win.set_navigation(views::navigation_code(view.navigation));
        }
        self.update_bar_overlay();
    }

    /// Hides the window and empties its models (the images would otherwise stay in memory until the next open); dropping
    /// the session then releases the instance, whose last reference shuts the players down before their windows are
    /// destroyed.
    pub(super) fn shutdown(mut self) {
        self.debounce.stop();
        self.tick.stop();
        bar_overlay::hide(BarOwner::Fullscreen);
        if let Some(win) = self.window.upgrade() {
            cursor::release(win.window(), Some(&self.instance), &mut self.cursor_hidden);
            win.set_is_ready(false);
            let _ = win.hide();
        }
        self.models.clear();
    }
}
