//! The viewer-tab host of a Multiview: shows the instance of the active tab in the main window.
//!
//! The canvas `multiview-viewer` in `main.slint` draws the scene from the `MultiviewTabBus` global. This module gives the
//! instance its rectangle (read from Slint, never from hard-wired chrome sizes), fills the bus from the instance, runs the
//! tick for the hover bar while the tab is shown and turns the events of the canvas into instance calls and UI actions.
//! All state lives on the UI thread; the instance stays the only copy of the Multiview itself.

use super::bar_hosting::{self, BarHost, BarTargets};
use super::bar_overlay::{self, BarOwner};
use super::cursor;
use super::handle::MultiviewHandle;
use super::router;
use super::video_bar::{self, VideoBarHost};
use super::views::{self, ImageCache, ViewModels};
use crate::ui::{MainWindow, MultiviewTabBus};
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::click::ClickKind;
use crate::viewer::multiview::command::{MultiviewCommand, PlayerCommand, ZoomKey};
use crate::viewer::multiview::hint::HintKind;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
use crate::viewer::multiview::media_actions::{BarClick, MediaActionId};
use crate::viewer::multiview::primary_actions::PrimaryActionId;
use crate::viewer::multiview::route::{HostMode, OriginKind, RoutedEvent, UiAction, route};
use crate::viewer::{PointerProbe, ViewerTabManager, get_slint_window_xid};
use rustview_core::library::models::MediaId;
use rustview_core::settings::schema::MULTIVIEW_TICK_MS;
use rustview_core::util::cast::SaturatingAs;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct TabHost {
    window: Option<slint::Weak<MainWindow>>,
    handle: Option<MultiviewHandle>,
    instance: WeakInstance,
    main_xid: u64,
    probe: Option<PointerProbe>,
    models: ViewModels,
    images: ImageCache,
    /// Instance id and revision that the bus shows right now.
    shown: Option<(u64, u64)>,
    tick: Timer,
    /// Redraws every frame while a zoom glides to its new value (the tick is too slow for that).
    glide: Timer,
    /// Wakes the host for the next change of a feedback icon (`media_feedback`); armed only while one is shown.
    feedback: Timer,
    /// The mouse cursor is hidden over the main window right now.
    cursor_hidden: bool,
    /// The instance and its selected medium the panels of the main window were last brought in line with.
    panel_focus: Option<(u64, Option<MediaId>)>,
}

thread_local! {
    static HOST: RefCell<TabHost> = RefCell::new(TabHost::default());
}

/// Runs `f` on the host state. Re-entrant calls are skipped instead of panicking.
fn with_host<R>(f: impl FnOnce(&mut TabHost) -> R) -> Option<R> {
    HOST.with(|cell| {
        if let Ok(mut host) = cell.try_borrow_mut() {
            Some(f(&mut host))
        } else {
            tracing::debug!("[Multiview: TabView] Skipped a re-entrant event");
            None
        }
    })
}

pub(super) fn current_instance() -> Option<SharedInstance> {
    with_host(|h| h.instance.upgrade()).flatten()
}

/// The Multiview the viewer tab shows right now.
pub fn shown_instance() -> Option<SharedInstance> {
    current_instance()
}

/// The feedback pulse changed (started, restarted): draw it and arm the timer. UI thread only.
pub fn feedback_changed() {
    refresh();
    arm_feedback_timer();
}

/// A player key or the wheel over a video may have started a pulse or a volume bar inside the instance: arm the timer for it.
fn arm_requested_feedback(instance: &SharedInstance) {
    if instance.lock().take_feedback_request() {
        arm_feedback_timer();
    }
}

fn arm_feedback_timer() {
    let Some(instance) = current_instance() else {
        return;
    };
    with_host(|h| super::feedback::arm(&h.feedback, &instance, on_feedback_frame));
}

fn on_feedback_frame() {
    if let Some(instance) = current_instance() {
        instance.lock().poll_feedback(std::time::Instant::now());
    }
    feedback_changed();
}

/// Wires the canvas events once at startup.
pub fn bind(window: &MainWindow, handle: MultiviewHandle) {
    let bus = window.global::<MultiviewTabBus>();
    with_host(|h| {
        h.window = Some(window.as_weak());
        h.handle = Some(handle);
        bus.set_tiles(ModelRc::from(h.models.tiles.clone()));
        bus.set_gutters(ModelRc::from(h.models.gutters.clone()));
    });
    bus.on_gutter_drag_started(|index| act(|mv| mv.drag_started(index.max(0).saturating_as::<usize>())));
    bus.on_gutter_dragged(|_, distance| act(|mv| mv.dragged(distance)));
    bus.on_gutter_drag_finished(|_| act(MultiviewInstance::drag_finished));
    bus.on_gutter_reset(|| act(MultiviewInstance::reset_split));
    bus.on_gutter_swap(|index| act(|mv| mv.swap(index.max(0).saturating_as::<usize>())));
    bus.on_tile_clicked(|index, is_ctrl| {
        // A plain click on a medium selects its tile.
        if !is_ctrl && let Ok(index) = usize::try_from(index) {
            act(|mv| {
                mv.select_tile_at(index);
            });
        }
        on_click(is_ctrl);
    });
    bus.on_tile_zoom_wheel(|index, delta, x, y| {
        act(|mv| mv.zoom_wheel(index.max(0).saturating_as::<usize>(), delta, x, y));
    });
    bus.on_tile_video_wheel(|index, delta, ctrl| {
        video_bar_after(|mv| mv.video_wheel(index.max(0).saturating_as::<usize>(), delta, ctrl));
    });
    bus.on_tile_right_clicked(|index| {
        video_bar_after(|mv| mv.video_right_click(index.max(0).saturating_as::<usize>()));
    });
    bus.on_tile_pan(|index, dx, dy| act(|mv| mv.zoom_pan(index.max(0).saturating_as::<usize>(), dx, dy)));
    bus.on_minimap_pointer(|index, x, y, phase| {
        let xid = with_host(|h| h.main_xid).unwrap_or(0);
        act(|mv| {
            super::pointer_lock::drag_phase(xid, phase, &mut |locked_move| match locked_move {
                Some((dx, dy)) => mv.zoom_minimap_delta(index.max(0).saturating_as::<usize>(), dx, dy),
                None => mv.zoom_minimap(index.max(0).saturating_as::<usize>(), x, y, phase),
            });
        });
    });
    bus.on_minimap_slider(|index, code| act(|mv| mv.zoom_minimap_slider(index.max(0).saturating_as::<usize>(), code)));
    bus.on_crop_pointer(|index, phase, x, y, shift| {
        if let Some(instance) = current_instance()
            && instance
                .lock()
                .crop_pointer_logical(index.max(0).saturating_as::<usize>(), phase, x, y, shift)
        {
            refresh();
        }
    });
    bus.on_crop_ratio(|index, landscape| act(|mv| mv.crop_ratio(index.max(0).saturating_as::<usize>(), landscape)));
    window.on_edit_key(edit_key);
    window.on_viewer_delete_key(|| command_key(MultiviewCommand::Delete));
    window.on_viewer_zoom_key(|code| {
        if let Some(zoom) = ZoomKey::from_code(code) {
            command_key(MultiviewCommand::Zoom(zoom));
        }
    });
    bus.on_background_clicked(on_click);
    bus.on_middle_clicked(|| run(route_for(RoutedEvent::ToggleFullscreen, ClickKind::None)));
}

/// A key of the image editor in the tab view (0 = Esc, see `MultiviewCommand::from_edit_code` for the others): the instance of
/// the shown tab decides; returns whether it used the key (Esc only when it cancelled a crop tool, every other one always).
fn edit_key(code: i32) -> bool {
    let Some(instance) = current_instance() else {
        return false;
    };
    let Some(command) = MultiviewCommand::from_edit_code(code) else {
        let used = code == 0 && instance.lock().escape_crop();
        drop(instance);
        refresh();
        return used;
    };
    let action = super::commands::run(&instance, command);
    refresh();
    arm_requested_feedback(&instance);
    drop(instance);
    run(action);
    true
}

/// A key of the tab view that acts on the selected medium of the shown tab (DEL, the zoom keys).
fn command_key(command: MultiviewCommand) {
    let Some(instance) = current_instance() else {
        return;
    };
    let action = super::commands::run(&instance, command);
    drop(instance);
    refresh();
    run(action);
}

/// The key of a media bar button (Settings > Hotkeys): the button of the selected medium, like a click on it.
pub fn bar_key(action: MediaActionId) {
    command_key(MultiviewCommand::Bar(action));
}

/// A video player key (Settings > Hotkeys > Video Player) on the selected video of the shown tab.
pub fn player_key(command: PlayerCommand) {
    command_key(MultiviewCommand::Player(command));
}

/// The shown tab holds a video, so the video player keys come first.
pub fn shown_has_video() -> bool {
    current_instance().is_some_and(|instance| instance.lock().has_video())
}

/// The key of a primary bar button (Settings > Hotkeys), like a click on it.
pub fn primary_key(action: PrimaryActionId) {
    on_primary_action(action.code());
}

/// A panel of the main window (Tag & Rate, tag history) was used while the shown tab holds several media and none is selected:
/// the hint asks for a selection, over the tab view.
pub fn show_selection_hint() {
    if let Some(instance) = current_instance() {
        let anchor = instance.lock().hint_anchor();
        super::commands::refused(anchor, HintKind::NoSelection);
    }
}

/// The selected medium of the shown tab for a key the application carries out itself (F5), or `None` after the hint.
pub fn key_target_media() -> Option<MediaId> {
    super::commands::target_media(&current_instance()?)
}

/// Runs `f` on the instance of the shown tab and syncs the result.
fn act(f: impl FnOnce(&mut MultiviewInstance)) {
    if let Some(instance) = current_instance() {
        f(&mut instance.lock());
    }
    refresh();
}

/// A button of the hover bar was clicked; what the application has to do about it runs after the instance lock is released.
fn on_action(tile: i32, action: i32) {
    let (Some(instance), Some(click)) = (current_instance(), BarClick::from_code(action)) else {
        return;
    };
    let outcome = {
        let mut guard = instance.lock();
        let Some(key) = usize::try_from(tile).ok().and_then(|index| guard.tile_key_at(index)) else {
            return;
        };
        guard.perform_click(key, click)
    };
    drop(instance);
    refresh();
    run(outcome);
}

/// The user deleted media through the Multiview of the shown tab: their tiles leave the view.
pub fn drop_deleted(ids: &[MediaId]) {
    let Some(instance) = current_instance() else {
        return;
    };
    let outcome = instance.lock().drop_deleted(ids);
    drop(instance);
    refresh();
    run(outcome);
}

/// The application finished what a button of the hover bar asked for: the button flashes green, or red if it failed. Only
/// a Multiview that is shown in the viewer tab is addressed (the fullscreen session reports to its own instance).
pub fn action_finished(media_id: MediaId, action: MediaActionId, ok: bool) {
    let Some(instance) = current_instance() else {
        return;
    };
    {
        let mut guard = instance.lock();
        if guard.host_mode() != HostMode::TabView {
            return;
        }
        guard.action_finished(media_id, action, ok);
    }
    drop(instance);
    refresh();
}

/// Shows, moves or hides the two bar windows while the pointer is close to their place (only while the Multiview is shown
/// in the viewer tab: a fullscreen over it has its own bars), and hides the mouse cursor while it rests and no bar is in use.
fn update_bar_overlay() {
    super::pointer_lock::service();
    let Some((window, instance, host_xid)) =
        with_host(|h| Some((h.window.clone()?.upgrade()?, h.instance.upgrade()?, h.main_xid))).flatten()
    else {
        bar_overlay::hide(BarOwner::TabView);
        video_bar::hide(BarOwner::TabView);
        return;
    };
    // While a dialog (or the settings window) locks the main window, no bar is shown, so none can be used, and the video
    // windows let the pointer through to the lock of the main window.
    let locked = window.get_is_modal_locked();
    let in_tab_view = {
        let mut guard = instance.lock();
        let shown_here = guard.host_mode() == HostMode::TabView;
        if shown_here {
            guard.set_modal_locked(locked);
        }
        shown_here && !locked
    };
    let host = BarHost {
        owner: BarOwner::TabView,
        host_xid,
        scale: window.window().scale_factor(),
        media_click: Rc::new(on_action),
        primary_click: Rc::new(|_, action| on_primary_action(action)),
    };
    let bars_visible = bar_hosting::update(
        &host,
        BarTargets::of_instance(BarOwner::TabView, &instance, in_tab_view),
        in_tab_view,
    );
    let video_bars_visible = update_video_bars(&instance, host_xid, in_tab_view);
    let mut window_hidden = with_host(|h| h.cursor_hidden).unwrap_or(false);
    if in_tab_view {
        cursor::sync(
            window.window(),
            &instance,
            bars_visible || video_bars_visible,
            &mut window_hidden,
        );
    } else {
        cursor::release(window.window(), Some(&instance), &mut window_hidden);
    }
    with_host(|h| h.cursor_hidden = window_hidden);
}

/// Shows, updates or hides the control bars of the videos of the tab view (none while `active` is false). Returns whether one
/// is visible.
fn update_video_bars(instance: &SharedInstance, host_xid: u64, active: bool) -> bool {
    if !active {
        instance.lock().release_video_bars();
        video_bar::hide(BarOwner::TabView);
        return false;
    }
    let prefs = video_bar::effective_prefs();
    let targets = instance.lock().video_bar_targets(&prefs, Instant::now());
    let host = VideoBarHost {
        owner: BarOwner::TabView,
        host_xid,
        on_input: Rc::new(|key, input| {
            video_bar_after(|mv| mv.video_bar_input(key, input, &video_bar::effective_prefs()));
        }),
    };
    video_bar::update(&host, &targets, &prefs)
}

/// Runs `f` on the shown instance; when it changed something, the view and the control bars follow at once.
fn video_bar_after(f: impl FnOnce(&mut MultiviewInstance) -> bool) {
    let Some(instance) = current_instance() else {
        return;
    };
    let changed = f(&mut instance.lock());
    if changed {
        refresh();
        let host_xid = with_host(|h| h.main_xid).unwrap_or(0);
        let shown_here = instance.lock().host_mode() == HostMode::TabView;
        update_video_bars(&instance, host_xid, shown_here);
    }
    arm_requested_feedback(&instance);
}

/// A button of the primary bar was clicked.
fn on_primary_action(action: i32) {
    let (Some(instance), Some(action)) = (current_instance(), PrimaryActionId::from_code(action)) else {
        return;
    };
    let outcome = instance.lock().perform_primary_action(action);
    drop(instance);
    refresh();
    run(outcome);
}

fn route_for(event: RoutedEvent, click: ClickKind) -> UiAction {
    let origin = current_instance().map_or(OriginKind::Tab, |i| i.lock().origin());
    route(HostMode::TabView, origin, event, click)
}

/// Two quick clicks with the same modifier state are a double click; the routing table decides what that means here.
fn on_click(is_ctrl: bool) {
    bar_overlay::close_menu(bar_overlay::BarKind::Media);
    let Some(instance) = current_instance() else {
        return;
    };
    let click = instance.lock().register_click(is_ctrl);
    drop(instance);
    if click == ClickKind::None {
        return;
    }
    let event = if is_ctrl {
        RoutedEvent::CtrlClick
    } else {
        RoutedEvent::ToggleFullscreen
    };
    run(route_for(event, click));
}

fn run(action: UiAction) {
    let instance_id = current_instance().map(|i| i.lock().id());
    if let Some(handle) = with_host(|h| h.handle.clone()).flatten() {
        router::execute(action, &handle, instance_id);
    }
}

/// Places the instance of the active tab in the viewer area and shows it. Called whenever the area, the tab or the
/// cutouts of menus and tooltips may have changed; the instance ignores a surface it already has.
pub fn sync(window: &MainWindow, tab_manager: &ViewerTabManager, instance: &SharedInstance) {
    let Some(main_xid) = get_slint_window_xid(window.window()) else {
        return;
    };
    let scale = window.window().scale_factor();
    let area = window.invoke_multiview_viewport();
    let rect = views::physical_rect(area.x, area.y, area.width, area.height, scale);
    let cutouts = tab_manager.get_combined_cutouts();
    let surface = HostSurface::tab_view(main_xid, rect, scale, cutouts);
    instance.lock().set_host(&surface);
    with_host(|h| {
        h.window = Some(window.as_weak());
        h.instance = Arc::downgrade(instance);
        h.main_xid = main_xid;
        if h.probe.is_none() {
            h.probe = PointerProbe::new()
                .map_err(
                    |e| tracing::warn!(error = %e, "[Multiview: TabView] No pointer probe; the hover bar is disabled"),
                )
                .ok();
        }
        if h.probe.is_some() {
            if !h.tick.running() {
                h.tick
                    .start(TimerMode::Repeated, Duration::from_millis(MULTIVIEW_TICK_MS), on_tick);
            }
        } else {
            h.tick.stop();
        }
    });
    refresh();
}

/// The Multiview is no longer the shown content (another tab, the Browse view, the grid or a fullscreen took over).
pub fn suspend() {
    // Called on every embedding sync, also while another host owns the bars: only a host that was active lets go of them.
    let active = with_host(|h| h.tick.running() || h.shown.is_some() || h.instance.upgrade().is_some()).unwrap_or(true);
    if !active {
        return;
    }
    bar_overlay::hide(BarOwner::TabView);
    video_bar::hide(BarOwner::TabView);
    if let Some(instance) = with_host(|h| h.instance.upgrade()).flatten() {
        instance.lock().release_video_bars();
    }
    if let Some(window) = with_host(|h| h.window.clone()).flatten().and_then(|w| w.upgrade()) {
        let instance = with_host(|h| h.instance.upgrade()).flatten();
        let mut window_hidden = with_host(|h| h.cursor_hidden).unwrap_or(false);
        cursor::release(window.window(), instance.as_ref(), &mut window_hidden);
        with_host(|h| h.cursor_hidden = window_hidden);
    }
    let released = with_host(|h| {
        h.tick.stop();
        h.glide.stop();
        h.feedback.stop();
        h.instance = WeakInstance::new();
        h.shown.take().is_some()
    })
    .unwrap_or(false);
    if released {
        with_host(|h| {
            h.models.clear();
            h.images = ImageCache::default();
        });
        tracing::debug!("[Multiview: TabView] Suspended");
    }
}

/// The application exits: the host lets go of the instance, its timer and its connection to the X server.
pub fn shutdown() {
    suspend();
    with_host(|h| {
        h.probe = None;
        h.window = None;
        h.handle = None;
    });
    tracing::debug!("[Shutdown: Windows] Tab view host released");
}

/// Brings the bus in line with the instance when it changed since the last sync (no X11 work).
pub fn refresh() {
    let Some((window, instance)) = with_host(|h| Some((h.window.clone()?.upgrade()?, h.instance.upgrade()?))).flatten()
    else {
        return;
    };
    if let Some(app) = with_host(|h| h.handle.clone())
        .flatten()
        .and_then(|handle| handle.app())
    {
        crate::ui::tabs::unsaved_marks::sync_if_changed(&window, &app.tab_manager);
    }
    let (id, view, focus) = {
        let guard = instance.lock();
        let key = (guard.id(), guard.revision());
        if with_host(|h| h.shown == Some(key)).unwrap_or(true) {
            return;
        }
        (key.0, guard.view(), guard.focus_target_media())
    };
    follow_panels(id, focus);
    with_host(|h| {
        let TabHost { models, images, .. } = h;
        models.sync(&view, images);
        h.shown = Some((id, view.revision));
    });
    let bus = window.global::<MultiviewTabBus>();
    bus.set_screen(views::screen_view(&view));
    update_bar_overlay();
    manage_glide(instance.lock().zoom_animating());
}

/// Another medium of the shown tab was selected (or the selection went): the panels of the main window follow it. They are
/// synced right after this event, when no lock of the caller is held any more.
fn follow_panels(instance_id: u64, focus: Option<MediaId>) {
    let changed = with_host(|h| {
        let now = Some((instance_id, focus));
        (h.panel_focus != now).then(|| {
            h.panel_focus = now;
            h.handle.clone()
        })
    })
    .flatten()
    .flatten();
    if let Some(handle) = changed {
        slint::Timer::single_shot(Duration::ZERO, move || handle.sync_tab_panels());
    }
}

/// Starts the frame timer while a zoom glides and stops it when it is done.
fn manage_glide(animating: bool) {
    with_host(|h| {
        if animating && !h.glide.running() {
            h.glide.start(TimerMode::Repeated, Duration::from_millis(16), || {
                if let Some(instance) = current_instance() {
                    instance.lock().poll_zoom();
                }
                refresh();
            });
        } else if !animating && h.glide.running() {
            h.glide.stop();
        }
    });
}

fn on_tick() {
    let Some((instance, pointer)) = with_host(|h| {
        let instance = h.instance.upgrade()?;
        let pointer = h.probe.as_ref().and_then(|p| p.position_in(h.main_xid));
        Some((instance, pointer))
    })
    .flatten() else {
        return;
    };
    instance.lock().tick(|| pointer);
    drop(instance);
    refresh();
    update_bar_overlay();
}

/// The medium an F2 rename acts on in the active tab: its selected medium (the hint shows when several are shown and none is).
pub fn rename_target(tab_manager: &ViewerTabManager) -> Option<MediaId> {
    let tab = tab_manager.get_active_tab()?;
    super::commands::target_media(&tab.multiview.instance)
}
