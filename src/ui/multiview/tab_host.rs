//! The viewer-tab host of a Multiview: shows the instance of the active tab in the main window.
//!
//! The canvas `multiview-viewer` in `main.slint` draws the scene from the `MultiviewTabBus` global. This module gives the
//! instance its rectangle (read from Slint, never from hard-wired chrome sizes), fills the bus from the instance, runs the
//! tick for the hover bar while the tab is shown and turns the events of the canvas into instance calls and UI actions.
//! All state lives on the UI thread; the instance stays the only copy of the Multiview itself.

use super::bar_hosting::{self, BarHost};
use super::bar_overlay::{self, BarOwner};
use super::cursor;
use super::handle::MultiviewHandle;
use super::router;
use super::views::{self, ImageCache, ViewModels};
use crate::library::models::MediaId;
use crate::settings::schema::MULTIVIEW_TICK_MS;
use crate::ui::{MainWindow, MultiviewTabBus};
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::primary_actions::PrimaryActionId;
use crate::viewer::multiview::click::ClickKind;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
use crate::viewer::multiview::route::{HostMode, OriginKind, RoutedEvent, UiAction, route};
use crate::viewer::{PointerProbe, ViewerTabManager, get_slint_window_xid};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

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
    /// The mouse cursor is hidden over the main window right now.
    cursor_hidden: bool,
}

thread_local! {
    static HOST: RefCell<TabHost> = RefCell::new(TabHost::default());
}

/// Runs `f` on the host state. Re-entrant calls are skipped instead of panicking.
fn with_host<R>(f: impl FnOnce(&mut TabHost) -> R) -> Option<R> {
    HOST.with(|cell| match cell.try_borrow_mut() {
        Ok(mut host) => Some(f(&mut host)),
        Err(_) => {
            tracing::debug!("[Multiview: TabView] Skipped a re-entrant event");
            None
        }
    })
}

fn current_instance() -> Option<SharedInstance> {
    with_host(|h| h.instance.upgrade()).flatten()
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
    bus.on_gutter_drag_started(|index| act(|mv| mv.drag_started(index.max(0) as usize)));
    bus.on_gutter_dragged(|_, distance| act(|mv| mv.dragged(distance)));
    bus.on_gutter_drag_finished(|_| act(MultiviewInstance::drag_finished));
    bus.on_gutter_reset(|| act(MultiviewInstance::reset_split));
    bus.on_gutter_swap(|index| act(|mv| mv.swap(index.max(0) as usize)));
    bus.on_tile_clicked(|_, is_ctrl| on_click(is_ctrl));
    bus.on_background_clicked(on_click);
    bus.on_middle_clicked(|| run(route_for(RoutedEvent::ToggleFullscreen, ClickKind::None)));
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
    let (Some(instance), Some(action)) = (current_instance(), MediaActionId::from_code(action)) else {
        return;
    };
    let outcome = {
        let mut guard = instance.lock();
        let Some(media_id) = usize::try_from(tile)
            .ok()
            .and_then(|index| guard.member_ids().get(index).copied())
        else {
            return;
        };
        guard.perform_action(media_id, action)
    };
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
    let Some((window, instance, host_xid)) = with_host(|h| {
        Some((h.window.clone()?.upgrade()?, h.instance.upgrade()?, h.main_xid))
    })
    .flatten() else {
        bar_overlay::hide(BarOwner::TabView);
        return;
    };
    let in_tab_view = instance.lock().host_mode() == HostMode::TabView;
    let host = BarHost {
        owner: BarOwner::TabView,
        host_xid,
        scale: window.window().scale_factor(),
        media_click: Rc::new(on_action),
        primary_click: Rc::new(|_, action| on_primary_action(action)),
    };
    let bars_visible = bar_hosting::update(&host, &instance, in_tab_view);
    let mut window_hidden = with_host(|h| h.cursor_hidden).unwrap_or(false);
    if in_tab_view {
        cursor::sync(window.window(), &instance, bars_visible, &mut window_hidden);
    } else {
        cursor::release(window.window(), Some(&instance), &mut window_hidden);
    }
    with_host(|h| h.cursor_hidden = window_hidden);
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
                .map_err(|e| tracing::warn!(error = %e, "[Multiview: TabView] No pointer probe; the hover bar is disabled"))
                .ok();
        }
        if h.probe.is_some() {
            if !h.tick.running() {
                h.tick.start(
                    TimerMode::Repeated,
                    Duration::from_millis(MULTIVIEW_TICK_MS),
                    on_tick,
                );
            }
        } else {
            h.tick.stop();
        }
    });
    refresh();
}

/// The Multiview is no longer the shown content (another tab, the Browse view, the grid or a fullscreen took over).
pub fn suspend() {
    bar_overlay::hide(BarOwner::TabView);
    if let Some(window) = with_host(|h| h.window.clone()).flatten().and_then(|w| w.upgrade()) {
        let instance = with_host(|h| h.instance.upgrade()).flatten();
        let mut window_hidden = with_host(|h| h.cursor_hidden).unwrap_or(false);
        cursor::release(window.window(), instance.as_ref(), &mut window_hidden);
        with_host(|h| h.cursor_hidden = window_hidden);
    }
    let released = with_host(|h| {
        h.tick.stop();
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

/// Brings the bus in line with the instance when it changed since the last sync (no X11 work).
pub fn refresh() {
    let Some((window, instance)) =
        with_host(|h| Some((h.window.clone()?.upgrade()?, h.instance.upgrade()?))).flatten()
    else {
        return;
    };
    let (id, view) = {
        let guard = instance.lock();
        let key = (guard.id(), guard.revision());
        if with_host(|h| h.shown == Some(key)).unwrap_or(true) {
            return;
        }
        (key.0, guard.view())
    };
    with_host(|h| {
        let TabHost { models, images, .. } = h;
        models.sync(&view, images);
        h.shown = Some((id, view.revision));
    });
    let bus = window.global::<MultiviewTabBus>();
    bus.set_screen(views::screen_view(&view));
    update_bar_overlay();
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

/// The medium an F2 rename acts on in the active tab: its only medium, or for a Multiview the tile a player named, else
/// the tile under the pointer.
pub fn rename_target(tab_manager: &ViewerTabManager) -> Option<MediaId> {
    let tab = tab_manager.get_active_tab()?;
    let Some(multiview) = tab.multiview() else {
        return Some(tab.media_id);
    };
    let pointer = with_host(|h| h.probe.as_ref().and_then(|p| p.position_in(h.main_xid))).flatten();
    multiview.instance.lock().resolve_rename_target(pointer)
}
