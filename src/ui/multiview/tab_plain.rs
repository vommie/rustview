//! The host of the two action bars for a normal image or video tab (the one a double click in the grid opens).
//!
//! Such a tab is not a Multiview, so `tab_host` has no instance to ask. This presenter polls the pointer while the tab is
//! shown (mpv swallows the pointer events over a video), builds the bars from `viewer::multiview::plain_tab` (the same
//! provider lists and preferences as every other host) and carries out what a click means. Like `tab_host` it shows the bars
//! only while the tab is the shown content, and hides the idle mouse cursor when the user wants that.

use super::bar_hosting::{self, BarHost, BarTargets};
use super::bar_overlay::{self, BarOwner};
use super::cursor;
use super::handle::MultiviewHandle;
use super::router;
use super::views;
use crate::library::models::MediaId;
use crate::settings::schema::MULTIVIEW_TICK_MS;
use crate::ui::MainWindow;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::media_actions::BarClick;
use crate::viewer::multiview::model::TileKind;
use crate::viewer::multiview::plain_tab::{PlainOutcome, PlainTabState};
use crate::viewer::multiview::primary_actions::PrimaryActionId;
use crate::viewer::x11_embed::X11VideoSubwindow;
use crate::viewer::{MediaTabSession, MpvPlayer, PointerProbe, TabId, TabPayload, ViewerTabManager, get_slint_window_xid};
use parking_lot::Mutex;
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

type SharedSubwindow = Arc<Mutex<Option<X11VideoSubwindow>>>;

/// The tab the bars act on, as far as they need it. Kept here so a tick does not clone the tab session.
struct ShownTab {
    tab_id: TabId,
    media_id: MediaId,
    kind: TileKind,
    player: Option<Arc<MpvPlayer>>,
    subwindow: Option<SharedSubwindow>,
}

impl ShownTab {
    /// `None` for a tab that cannot be acted on: a Multiview (it has its own host) or a medium that failed to load.
    fn of(tab: &MediaTabSession) -> Option<Self> {
        let (kind, player, subwindow) = match &tab.payload {
            TabPayload::Image(image) if image.error_message.is_empty() => (TileKind::Image, None, None),
            TabPayload::Video(video) if video.error_message.is_empty() => {
                (TileKind::Video, Some(video.player.clone()), Some(video.subwindow.clone()))
            }
            _ => return None,
        };
        Some(Self {
            tab_id: tab.id,
            media_id: tab.media_id,
            kind,
            player,
            subwindow,
        })
    }
}

#[derive(Default)]
struct PlainHost {
    window: Option<slint::Weak<MainWindow>>,
    handle: Option<MultiviewHandle>,
    main_xid: u64,
    area: Option<Rect>,
    probe: Option<PointerProbe>,
    shown: Option<ShownTab>,
    tick: Timer,
    pointer: Option<(i32, i32)>,
    last_move: Option<Instant>,
    /// The mouse cursor is hidden over the main window (and the video window of `shown`) right now.
    cursor_hidden: bool,
}

thread_local! {
    static HOST: RefCell<PlainHost> = RefCell::new(PlainHost::default());
}

/// Runs `f` on the host state. Re-entrant calls are skipped instead of panicking.
fn with_host<R>(f: impl FnOnce(&mut PlainHost) -> R) -> Option<R> {
    HOST.with(|cell| match cell.try_borrow_mut() {
        Ok(mut host) => Some(f(&mut host)),
        Err(_) => {
            tracing::debug!("[Multiview: PlainTab] Skipped a re-entrant event");
            None
        }
    })
}

/// The state the bars are built from right now; `None` while no tab is shown or the pointer cannot be asked.
fn current_state() -> Option<(PlainTabState, Option<Arc<MpvPlayer>>)> {
    let window = with_host(|h| h.window.clone()).flatten()?.upgrade()?;
    with_host(|h| {
        let shown = h.shown.as_ref()?;
        let player = shown.player.clone();
        let state = PlainTabState {
            media_id: shown.media_id,
            kind: shown.kind,
            muted: player.as_ref().map(|p| p.get_mute()),
            playing: player.as_ref().is_some_and(|p| !p.is_user_paused()),
            area: h.area?,
            pointer: h.pointer,
            dpi: window.window().scale_factor(),
        };
        Some((state, player))
    })
    .flatten()
}

/// Gives the host the handle through which bar clicks reach the application. Called once at startup.
pub fn bind(handle: MultiviewHandle) {
    with_host(|h| h.handle = Some(handle));
}

/// Brings the bars in line with what the main window shows: they belong to the active tab while it is a normal image or
/// video tab shown in the viewer (not behind a fullscreen of its own). Called whenever the view, the tabs or the viewer area
/// may have changed; cheap when nothing did.
pub fn refresh(window: &MainWindow, tab_manager: &ViewerTabManager) {
    let Some(handle) = with_host(|h| h.handle.clone()).flatten() else {
        return;
    };
    let shown = window.get_active_view_mode() == 1 && !handle.is_isolated_open();
    match shown.then(|| tab_manager.get_active_tab()).flatten() {
        Some(tab) => sync(window, &handle, &tab),
        None => suspend(),
    }
}

/// Shows the bars for `tab`, the active tab of the viewer, and starts polling the pointer. The host keeps what it already
/// knows.
fn sync(window: &MainWindow, handle: &MultiviewHandle, tab: &MediaTabSession) {
    let Some(shown) = ShownTab::of(tab) else {
        suspend();
        return;
    };
    tracing::trace!(tab_id = shown.tab_id, "[Multiview: PlainTab] Sync");
    let Some(main_xid) = get_slint_window_xid(window.window()) else {
        return;
    };
    let area = viewer_area(window);
    let replaced = with_host(|h| {
        h.shown
            .as_ref()
            .is_some_and(|s| (s.tab_id, s.media_id) != (shown.tab_id, shown.media_id))
    })
    .unwrap_or(false);
    if replaced {
        // The previous medium's video window may still carry the hidden cursor.
        bar_overlay::hide(BarOwner::TabView);
        release_cursor(true);
    }
    let started = with_host(|h| {
        h.window = Some(window.as_weak());
        h.handle = Some(handle.clone());
        h.main_xid = main_xid;
        h.area = Some(area);
        if h.probe.is_none() {
            h.probe = PointerProbe::new()
                .map_err(|e| tracing::warn!(error = %e, "[Multiview: PlainTab] No pointer probe; the action bars are disabled"))
                .ok();
        }
        h.shown = Some(shown);
        if h.probe.is_none() {
            h.tick.stop();
            return false;
        }
        let start = !h.tick.running();
        if start {
            h.tick.start(TimerMode::Repeated, Duration::from_millis(MULTIVIEW_TICK_MS), on_tick);
            tracing::debug!("[Multiview: PlainTab] Bars shown for the active tab");
        }
        start
    })
    .unwrap_or(false);
    if started {
        // A bar window that is created and shown in the same moment stays black until something redraws it.
        bar_overlay::prepare();
    }
}

/// The tab is no longer the shown content (another tab, the Browse view, the grid or a fullscreen took over).
pub fn suspend() {
    let was_shown = with_host(|h| {
        h.tick.stop();
        h.pointer = None;
        h.last_move = None;
        h.shown.is_some()
    })
    .unwrap_or(false);
    if !was_shown {
        return;
    }
    bar_overlay::hide(BarOwner::TabView);
    release_cursor(true);
    tracing::debug!("[Multiview: PlainTab] Suspended");
}

/// The application exits: the host lets go of its timer and its connection to the X server.
pub fn shutdown() {
    suspend();
    with_host(|h| {
        h.probe = None;
        h.window = None;
        h.handle = None;
    });
}

/// Shows the cursor again over the main window and the video window the host hid it on. With `forget` the shown tab is
/// dropped too (also when the cursor was not hidden).
fn release_cursor(forget: bool) {
    let Some((window, subwindow, hidden)) = with_host(|h| {
        let hidden = std::mem::replace(&mut h.cursor_hidden, false);
        let subwindow = h.shown.as_ref().and_then(|s| s.subwindow.clone());
        if forget {
            h.shown = None;
        }
        (h.window.clone().and_then(|w| w.upgrade()), subwindow, hidden)
    }) else {
        return;
    };
    if !hidden {
        return;
    }
    if let Some(window) = window {
        cursor::set_hidden(window.window(), false);
    }
    set_video_cursor_hidden(subwindow.as_ref(), false);
}

fn set_video_cursor_hidden(subwindow: Option<&SharedSubwindow>, hidden: bool) {
    if let Some(subwindow) = subwindow
        && let Some(mut guard) = subwindow.try_lock()
        && let Some(window) = guard.as_mut()
    {
        window.set_cursor_hidden(hidden);
    }
}

/// The viewer area of the main window in physical pixels, read from Slint like the Multiview tab does.
fn viewer_area(window: &MainWindow) -> Rect {
    let view = window.invoke_multiview_viewport();
    views::physical_rect(view.x, view.y, view.width, view.height, window.window().scale_factor())
}

fn on_tick() {
    let moved = with_host(|h| {
        if let Some(window) = h.window.as_ref().and_then(|w| w.upgrade()) {
            h.area = Some(viewer_area(&window));
        }
        let pointer = h.probe.as_ref().and_then(|p| p.position_in(h.main_xid));
        if pointer != h.pointer || h.last_move.is_none() {
            h.last_move = Some(Instant::now());
        }
        h.pointer = pointer;
    });
    if moved.is_none() {
        return;
    }
    update_bar_overlay();
}

/// Shows, moves or hides the two bar windows while the pointer is close to their place, and hides the mouse cursor while it
/// rests and no bar is in use.
fn update_bar_overlay() {
    let Some((state, _)) = current_state() else {
        bar_overlay::hide(BarOwner::TabView);
        return;
    };
    let Some((window, host_xid, last_move)) = with_host(|h| {
        Some((h.window.clone()?.upgrade()?, h.main_xid, h.last_move))
    })
    .flatten() else {
        return;
    };
    // While a dialog (or the settings window) locks the main window, no bar is shown, so none can be used.
    if window.get_is_modal_locked() {
        bar_overlay::hide(BarOwner::TabView);
        sync_cursor(&window, false);
        return;
    }
    let host = BarHost {
        owner: BarOwner::TabView,
        host_xid,
        scale: state.dpi,
        media_click: Rc::new(move |_, action| on_media_click(action)),
        primary_click: Rc::new(|_, action| on_primary_click(action)),
    };
    let targets = BarTargets {
        media: state.media_bar_target(),
        primary: state.primary_bar_target(),
        zoom: None,
    };
    let bars_visible = bar_hosting::update(&host, targets, true);
    sync_cursor(&window, state.cursor_should_hide(last_move) && !bars_visible);
}

fn sync_cursor(window: &MainWindow, hide: bool) {
    let Some((changed, subwindow)) = with_host(|h| {
        let changed = h.cursor_hidden != hide;
        h.cursor_hidden = hide;
        (changed, h.shown.as_ref().and_then(|s| s.subwindow.clone()))
    }) else {
        return;
    };
    if changed {
        cursor::set_hidden(window.window(), hide);
        set_video_cursor_hidden(subwindow.as_ref(), hide);
    }
}

/// A button of the media bar (or an item of its drop-down) was clicked.
fn on_media_click(code: i32) {
    let Some(click) = BarClick::from_code(code) else {
        return;
    };
    let Some((state, player)) = current_state() else {
        return;
    };
    finish(state.media_click(click), player);
}

/// A button of the primary bar was clicked.
fn on_primary_click(code: i32) {
    let Some(id) = PrimaryActionId::from_code(code) else {
        return;
    };
    let Some((state, player)) = current_state() else {
        return;
    };
    finish(state.primary_click(id), player);
}

/// Carries out what a click asked for, then brings the bars in line with the new state.
fn finish(outcome: PlainOutcome, player: Option<Arc<MpvPlayer>>) {
    tracing::info!(?outcome, "[Multiview: PlainTab] Bar button clicked");
    match outcome {
        PlainOutcome::None => return,
        PlainOutcome::SetMuted(muted) => {
            if let Some(Err(e)) = player.map(|p| p.set_mute(muted)) {
                tracing::warn!(error = %e, "[Multiview: PlainTab] Mute failed");
            }
        }
        PlainOutcome::SetPaused(paused) => {
            let result = player.map(|p| if paused { p.pause() } else { p.play() });
            if let Some(Err(e)) = result {
                tracing::warn!(error = %e, "[Multiview: PlainTab] Pause or play failed");
            }
        }
        PlainOutcome::Ui(action) => {
            if let Some(handle) = with_host(|h| h.handle.clone()).flatten() {
                router::execute(action, &handle, None);
            }
            return;
        }
    }
    update_bar_overlay();
}
