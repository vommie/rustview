//! The viewer-tab host of a Multiview: shows the instance of the active tab in the main window.
//!
//! The canvas `multiview-viewer` in `main.slint` draws the scene from the `MultiviewTabBus` global. This module gives the
//! instance its rectangle (read from Slint, never from hard-wired chrome sizes), fills the bus from the instance, runs the
//! tick for the mute badge while a video is shown and turns the events of the canvas into instance calls and UI actions.
//! All state lives on the UI thread; the instance stays the only copy of the Multiview itself.

use super::handle::MultiviewHandle;
use super::router;
use super::views::{self, ImageCache};
use crate::library::models::MediaId;
use crate::settings::schema::MULTIVIEW_TICK_MS;
use crate::ui::{
    MainWindow, MultiviewGutterView, MultiviewScreenView, MultiviewTabBus, MultiviewTileView,
};
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::click::ClickKind;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::route::{HostMode, OriginKind, RoutedEvent, UiAction, route};
use crate::viewer::{PointerProbe, ViewerTabManager, get_slint_window_xid};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
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
    tile_model: Rc<VecModel<MultiviewTileView>>,
    gutter_model: Rc<VecModel<MultiviewGutterView>>,
    images: ImageCache,
    /// Instance id and revision that the bus shows right now.
    shown: Option<(u64, u64)>,
    tick: Timer,
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

/// Logical area of the canvas in the window -> physical window pixels. Edges are rounded, not sizes, so neighbors stay
/// seamless.
fn physical_rect(area: &MultiviewScreenView, scale: f32) -> Rect {
    let edge = |logical: f32| (logical * scale).round() as i32;
    let (x0, y0) = (edge(area.x), edge(area.y));
    let (x1, y1) = (edge(area.x + area.width), edge(area.y + area.height));
    Rect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
}

/// Wires the canvas events once at startup.
pub fn bind(window: &MainWindow, handle: MultiviewHandle) {
    let bus = window.global::<MultiviewTabBus>();
    with_host(|h| {
        h.window = Some(window.as_weak());
        h.handle = Some(handle);
        bus.set_tiles(ModelRc::from(h.tile_model.clone()));
        bus.set_gutters(ModelRc::from(h.gutter_model.clone()));
    });
    bus.on_gutter_drag_started(|index| act(|mv| mv.drag_started(index.max(0) as usize)));
    bus.on_gutter_dragged(|_, distance| act(|mv| mv.dragged(distance)));
    bus.on_gutter_drag_finished(|_| act(MultiviewInstance::drag_finished));
    bus.on_gutter_reset(|| act(MultiviewInstance::reset_split));
    bus.on_gutter_swap(|index| act(|mv| mv.swap(index.max(0) as usize)));
    bus.on_badge_clicked(|| act(MultiviewInstance::toggle_badge_mute));
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
    let rect = physical_rect(&window.invoke_multiview_viewport(), scale);
    let cutouts = tab_manager.get_combined_cutouts();
    let surface = HostSurface::tab_view(main_xid, rect, scale, cutouts);
    let has_video = {
        let mut guard = instance.lock();
        guard.set_host(&surface);
        guard.has_video()
    };
    with_host(|h| {
        h.window = Some(window.as_weak());
        h.instance = Arc::downgrade(instance);
        h.main_xid = main_xid;
        if h.probe.is_none() {
            h.probe = PointerProbe::new()
                .map_err(|e| tracing::warn!(error = %e, "[Multiview: TabView] No pointer probe; the mute badge is disabled"))
                .ok();
        }
        if has_video && h.probe.is_some() {
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
    let released = with_host(|h| {
        h.tick.stop();
        h.instance = WeakInstance::new();
        h.shown.take().is_some()
    })
    .unwrap_or(false);
    if released {
        with_host(|h| {
            h.tile_model.set_vec(Vec::new());
            h.gutter_model.set_vec(Vec::new());
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
        views::sync_models(&h.tile_model, &h.gutter_model, &view, &mut h.images);
        h.shown = Some((id, view.revision));
    });
    let bus = window.global::<MultiviewTabBus>();
    bus.set_screen(views::screen_view(&view));
    bus.set_badge(views::badge_view(view.badge.as_ref(), view.dpi));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: f32, y: f32, width: f32, height: f32) -> MultiviewScreenView {
        MultiviewScreenView {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_viewer_area_converts_to_physical_pixels_by_rounding_edges() {
        let rect = physical_rect(&area(0.0, 64.0, 960.0, 540.0), 1.0);
        assert_eq!(rect, Rect::new(0, 64, 960, 540));

        let rect = physical_rect(&area(0.0, 64.0, 801.5, 539.5), 1.25);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 80);
        assert_eq!(rect.x + rect.w, (801.5f32 * 1.25).round() as i32);
        assert_eq!(rect.y + rect.h, ((64.0f32 + 539.5) * 1.25).round() as i32);
    }

    #[test]
    fn a_collapsed_area_never_yields_an_empty_rectangle() {
        let rect = physical_rect(&area(10.0, 10.0, 0.0, 0.0), 2.0);
        assert!(rect.w >= 1 && rect.h >= 1);
    }
}
