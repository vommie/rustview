//! The windows of the two action bars (UI thread only): the media action bar (top-right corner of the hovered medium) and
//! the primary action bar (top-left corner of the whole Multiview).
//!
//! Each bar is one frameless transparent window that floats above whichever Multiview shows it (the fullscreen window or the
//! viewer tab).
//! It is an override-redirect window of its own, like the tooltips and popup menus of other toolkits: the window manager
//! leaves it alone (no focus is taken, nothing is stacked below the fullscreen window), and the compositing manager blends
//! it with everything below it, including the native mpv windows that nothing inside the Multiview window could blend
//! with. Where no compositing manager runs, the window cannot be blended: it is solid then and shaped like the bar, so it
//! does not show black corners. Placement and shapes are computed by the pure `bar_placement`; this module only turns them
//! into window calls.

use super::views::sync_model;
use crate::ctx_menu::{CtxMenuDispatcher, CtxMenuItem};
use crate::ui::{ContextMenuItemModel, MultiviewBarButtonView, MultiviewBarWindow};
use crate::viewer::multiview::bar_menu::OutsidePress;
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::view::ZoomBubbleView;
use crate::viewer::multiview::bar_placement::{BarCorner, BarPlacement, in_reveal_zone, place_bar_in, rounded_region};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::{OverlayServer, XRectangle, get_raw_window_xid};
use slint::winit_030::winit::platform::x11::WindowAttributesExtX11;
use slint::winit_030::winit::window::WindowAttributes;
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// Which host shows the bar. The window is shared, so a host may only hide a bar it shows itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarOwner {
    Fullscreen,
    TabView,
}

/// The two bars. Each has a window of its own, so both can be visible at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// The buttons of the hovered medium, in the top-right corner of its tile.
    Media,
    /// The buttons of the whole view, in the top-left corner of the Multiview.
    Primary,
}

impl BarKind {
    const fn index(self) -> usize {
        match self {
            Self::Media => 0,
            Self::Primary => 1,
        }
    }

    const fn corner(self) -> BarCorner {
        match self {
            Self::Media => BarCorner::TopRight,
            Self::Primary => BarCorner::TopLeft,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Media => "media action bar",
            Self::Primary => "primary action bar",
        }
    }
}

/// Called with the tile and the action id of a clicked bar button (the tile is -1 for the primary bar).
type ActionHandler = Rc<dyn Fn(i32, i32)>;

/// What a host wants a bar to show.
pub struct BarRequest {
    pub kind: BarKind,
    pub owner: BarOwner,
    /// The window the Multiview is drawn in; the bar is only shown while that window is the one in front at the pointer
    /// (a window of another application on top of it must not get a bar floating above it).
    pub host_xid: u64,
    /// The hovered tile (handed back with a click; -1 for the primary bar) and the rectangle the bar sits in the corner of
    /// (the tile, or the whole Multiview), in the coordinates of the host window.
    pub tile: i32,
    pub area: Rect,
    /// Pointer position in the coordinates of the host window. The bar is revealed while the pointer is within
    /// `reveal_distance_px` (logical pixels) of the place of the bar, and hidden otherwise.
    pub pointer: Option<(i32, i32)>,
    pub reveal_distance_px: u32,
    /// Show the bar whatever the pointer does (it shows itself to give feedback).
    pub forced: bool,
    /// Physical pixels per logical pixel of the host.
    pub scale: f32,
    pub actions: Vec<MultiviewBarButtonView>,
    pub on_action: ActionHandler,
}

/// While a menu is open the pointer is polled this often: a click is shorter than a tick of the host, and a press that
/// started and ended between two polls would be missed.
const MENU_POLL_MS: u64 = 16;

thread_local! {
    /// Polls the pointer while the menu of the media bar is open.
    static MENU_TIMER: Timer = Timer::default();
    static OVERLAYS: [RefCell<Option<Overlay>>; 2] = const { [RefCell::new(None), RefCell::new(None)] };
    /// The native windows of the bars: a pointer on any of them still counts as being on the Multiview.
    static OVERLAY_XIDS: Cell<[Option<u64>; 2]> = const { Cell::new([None; 2]) };
    /// Set right before the overlay window is created, so that `window_attributes_hook` knows which window it is.
    static NEXT_IS_OVERLAY: Cell<bool> = const { Cell::new(false) };
    static TOOLTIPS: Cell<(bool, i32)> = const { Cell::new((true, 750)) };
}

/// Makes the overlay window an override-redirect window. Installed once at startup through the backend selector; Slint
/// calls it for every window it creates and cannot say which one, so the creator announces the overlay window.
pub fn window_attributes_hook(attributes: WindowAttributes) -> WindowAttributes {
    if NEXT_IS_OVERLAY.with(Cell::take) {
        attributes.with_override_redirect(true)
    } else {
        attributes
    }
}

/// The tooltip settings of the application (they apply to the overlay window too).
pub fn set_tooltip_settings(enabled: bool, delay_ms: i32) {
    TOOLTIPS.with(|cell| cell.set((enabled, delay_ms)));
    // Only bars that exist are updated. A bar window costs an X window and a GL context, so it is not created at startup; a new
    // one reads these settings when it is created.
    OVERLAYS.with(|cells| {
        for cell in cells {
            if let Ok(slot) = cell.try_borrow()
                && let Some(overlay) = slot.as_ref()
            {
                overlay.apply_tooltip_settings();
            }
        }
    });
}

/// Creates both bar windows ahead of their first reveal (a Multiview opens), so that creating them does not delay the first
/// appearance of a bar.
pub fn prepare() {
    for kind in [BarKind::Media, BarKind::Primary] {
        with_overlay(kind, |_| ());
    }
}

/// Shows the bar for the tile of `request` while the pointer is close enough to it, or hides it if the pointer is not, the
/// tile cannot carry a bar or the host is not the window in front. Returns whether the bar is visible.
pub fn show(request: &BarRequest) -> bool {
    with_overlay(request.kind, |overlay| overlay.show(request)).unwrap_or(false)
}

/// Opens the drop-down menu below the bar `kind` (the items are shown with the rows of a context menu); the next click on an
/// item calls the bar's action handler with the id of the item. A bar that is not shown ignores this.
pub fn open_menu(kind: BarKind, opener_action: i32, items: &[CtxMenuItem]) {
    with_overlay(kind, |overlay| {
        if overlay.shown {
            let opener = overlay.last_actions.iter().position(|a| a.id == opener_action);
            overlay.window.set_menu_owner_index(opener.map_or(-1, |i| i as i32));
            overlay.window.set_menu_items(CtxMenuDispatcher::items_to_slint_model(items, 0));
            overlay.window.set_menu_open(true);
            let down = overlay.pointer_state().is_some_and(|(_, down)| down);
            overlay.outside.arm(down);
        }
    });
    MENU_TIMER.with(|timer| {
        timer.start(TimerMode::Repeated, Duration::from_millis(MENU_POLL_MS), move || {
            if !poll_menu(kind) {
                MENU_TIMER.with(Timer::stop);
            }
        });
    });
}

/// One poll of the open menu of `kind`, whoever shows the bar. Returns whether the menu is still open.
fn poll_menu(kind: BarKind) -> bool {
    OVERLAYS.with(|cells| {
        let Ok(mut slot) = cells[kind.index()].try_borrow_mut() else {
            return true;
        };
        slot.as_mut().is_some_and(Overlay::service_menu)
    })
}

/// While the menu of the bar `kind`, shown by `owner`, is open: the bar stays where it is whatever the pointer does, and a click
/// outside the bar and the menu closes the menu. Called on every tick of the host. Returns whether the menu is open.
pub fn service_menu(kind: BarKind, owner: BarOwner) -> bool {
    OVERLAYS.with(|cells| {
        let Ok(mut slot) = cells[kind.index()].try_borrow_mut() else {
            return false;
        };
        slot.as_mut().filter(|overlay| overlay.owner == Some(owner)).is_some_and(Overlay::service_menu)
    })
}

/// Shows, updates or closes the zoom slider that hangs below the bar `kind`, as the Multiview that shows the bar says (`None` =
/// closed). While it is open it keeps its bar where it is (`service_menu`), until the Multiview closes it (the button toggles it).
pub fn sync_zoom(kind: BarKind, owner: BarOwner, zoom: Option<&ZoomBubbleView>) {
    OVERLAYS.with(|cells| {
        let Ok(mut slot) = cells[kind.index()].try_borrow_mut() else {
            return;
        };
        if let Some(overlay) = slot.as_mut().filter(|overlay| overlay.owner == Some(owner) || zoom.is_none()) {
            overlay.sync_zoom(zoom);
        }
    });
}

/// Closes the drop-down menu of the bar `kind`. Returns whether one was open.
pub fn close_menu(kind: BarKind) -> bool {
    OVERLAYS.with(|cells| {
        let Ok(slot) = cells[kind.index()].try_borrow() else {
            return false;
        };
        slot.as_ref().is_some_and(|overlay| overlay.close_menu())
    })
}

/// Hides the bars `owner` shows (a host may only hide a bar it shows itself).
pub fn hide(owner: BarOwner) {
    for kind in [BarKind::Media, BarKind::Primary] {
        hide_kind(kind, owner);
    }
}

/// Hides one bar if `owner` is the host that shows it.
pub fn hide_kind(kind: BarKind, owner: BarOwner) {
    OVERLAYS.with(|cells| {
        let Ok(mut slot) = cells[kind.index()].try_borrow_mut() else {
            tracing::debug!("[Multiview: Overlay] Skipped a re-entrant call");
            return;
        };
        // A bar that was never shown has no window; hiding it must not create one.
        if let Some(overlay) = slot.as_mut().filter(|overlay| overlay.owner == Some(owner)) {
            overlay.hide();
        }
    });
}

/// The application exits: both bar windows are hidden and destroyed (with their X connections). A bar that was never shown has
/// no window and is not created for this.
pub fn shutdown() {
    OVERLAYS.with(|cells| {
        for kind in [BarKind::Media, BarKind::Primary] {
            let taken = cells[kind.index()].try_borrow_mut().ok().and_then(|mut slot| slot.take());
            if let Some(mut overlay) = taken {
                overlay.hide();
                tracing::debug!(bar = kind.label(), "[Shutdown: Windows] Bar window destroyed");
            }
        }
    });
    OVERLAY_XIDS.with(|cell| cell.set([None; 2]));
}

/// Runs `f` on the overlay, creating it on first use. Re-entrant calls (a click handler that hides the bar) are skipped.
fn with_overlay<R>(kind: BarKind, f: impl FnOnce(&mut Overlay) -> R) -> Option<R> {
    OVERLAYS.with(|cells| {
        let Ok(mut slot) = cells[kind.index()].try_borrow_mut() else {
            tracing::debug!("[Multiview: Overlay] Skipped a re-entrant call");
            return None;
        };
        if slot.is_none() {
            *slot = Overlay::new(kind);
        }
        slot.as_mut().map(f)
    })
}

/// What the callbacks of the window need as well.
#[derive(Default)]
struct Shared {
    on_action: RefCell<Option<ActionHandler>>,
    x: RefCell<XState>,
}

/// The X side of the window: the private connection and what shapes the window currently has to have.
#[derive(Default)]
struct XState {
    server: Option<OverlayServer>,
    xid: Option<u64>,
    /// The window is blended by a compositing manager (glass); otherwise it is solid and shaped.
    translucent: bool,
    placed: Option<BarPlacement>,
    /// The tooltip bubble inside the window, physical pixels relative to the window.
    tooltip: Option<Rect>,
    /// The drop-down menu inside the window, physical pixels relative to the window (the caret zone on top included).
    menu: Option<Rect>,
    /// The caret of the menu: its center x (relative to the window) and its height, physical pixels.
    menu_caret: (i32, i32),
    bar_radius: i32,
    tooltip_radius: i32,
    menu_radius: i32,
    scale: f32,
}

impl XState {
    /// The part of the window that takes the pointer is the bar; in the solid case the visible part is the bar and the
    /// tooltip bubble, with rounded corners.
    fn apply_shape(&self) {
        let (Some(server), Some(xid), Some(placed)) = (self.server.as_ref(), self.xid, self.placed) else {
            return;
        };
        let bar = Rect::new(
            placed.bar.x - placed.window.x,
            placed.bar.y - placed.window.y,
            placed.bar.w,
            placed.bar.h,
        );
        let mut input = vec![xrectangle(bar)];
        input.extend(self.menu.map(xrectangle));
        server.set_input_region(xid, &input);
        if self.translucent {
            server.set_bounding_region(xid, None);
            return;
        }
        let mut region = rounded_region(bar, self.bar_radius);
        if let Some(tooltip) = self.tooltip {
            region.extend(rounded_region(tooltip, self.tooltip_radius));
        }
        if let Some(menu) = self.menu {
            let (caret_x, caret_h) = self.menu_caret;
            let body = Rect::new(menu.x, menu.y + caret_h, menu.w, (menu.h - caret_h).max(1));
            region.extend(rounded_region(body, self.menu_radius));
            // The caret is a 45 degree triangle: row i from its tip is 2 * (i + 1) pixels wide.
            for i in 0..caret_h.max(0) {
                region.push(xrectangle(Rect::new(caret_x - (i + 1), menu.y + i, 2 * (i + 1), 1)));
            }
        }
        server.set_bounding_region(xid, Some(&region));
    }
}

/// Closes the drop-down of `window` and forgets the area it took.
fn clear_menu(window: &MultiviewBarWindow, shared: &Shared) {
    window.set_menu_open(false);
    window.set_menu_owner_index(-1);
    window.set_menu_items(ModelRc::<ContextMenuItemModel>::default());
    if let Ok(mut state) = shared.x.try_borrow_mut() {
        state.menu = None;
        state.apply_shape();
    }
}

fn xrectangle(rect: Rect) -> XRectangle {
    XRectangle {
        x: rect.x.clamp(0, i16::MAX as i32) as i16,
        y: rect.y.clamp(0, i16::MAX as i32) as i16,
        width: rect.w.clamp(1, u16::MAX as i32) as u16,
        height: rect.h.clamp(1, u16::MAX as i32) as u16,
    }
}

struct Overlay {
    kind: BarKind,
    window: MultiviewBarWindow,
    model: Rc<VecModel<MultiviewBarButtonView>>,
    shared: Rc<Shared>,
    owner: Option<BarOwner>,
    shown: bool,
    last_actions: Vec<MultiviewBarButtonView>,
    last_zoom_snaps: Vec<i32>,
    outside: OutsidePress,
}

impl Overlay {
    fn new(kind: BarKind) -> Option<Self> {
        NEXT_IS_OVERLAY.with(|flag| flag.set(true));
        let created = MultiviewBarWindow::new();
        NEXT_IS_OVERLAY.with(|flag| flag.set(false));
        let window = match created {
            Ok(window) => window,
            Err(e) => {
                tracing::error!(error = %e, "[Multiview: Overlay] Failed to create a bar window");
                return None;
            }
        };
        let server = match OverlayServer::new() {
            Ok(server) => Some(server),
            Err(e) => {
                tracing::warn!(error = %e, "[Multiview: Overlay] No X connection; the bar cannot be placed");
                None
            }
        };
        let shared = Rc::new(Shared::default());
        shared.x.borrow_mut().server = server;
        let model = Rc::new(VecModel::default());
        window.set_actions(ModelRc::from(model.clone()));

        let for_click = shared.clone();
        window.on_action_clicked(move |tile, action| {
            let handler = for_click.on_action.borrow().clone();
            if let Some(handler) = handler {
                handler(tile, action);
            }
        });
        let for_tooltip = shared.clone();
        window.on_tooltip_changed(move |visible, x, y, w, h| {
            let Ok(mut state) = for_tooltip.x.try_borrow_mut() else {
                return;
            };
            let scale = state.scale.max(0.5);
            let px = |logical: f32| (logical * scale).round() as i32;
            state.tooltip = (visible && w > 2.0 && h > 2.0)
                .then(|| Rect::new(px(x), px(y), px(w).max(1), px(h).max(1)));
            state.apply_shape();
        });
        let for_menu = shared.clone();
        window.on_menu_changed(move |visible, x, y, w, h, caret_x, caret_h| {
            let Ok(mut state) = for_menu.x.try_borrow_mut() else {
                return;
            };
            let scale = state.scale.max(0.5);
            let px = |logical: f32| (logical * scale).round() as i32;
            state.menu = (visible && w > 2.0 && h > 2.0).then(|| Rect::new(px(x), px(y), px(w).max(1), px(h).max(1)));
            state.menu_caret = (px(caret_x), px(caret_h));
            state.apply_shape();
        });
        let for_menu_click = shared.clone();
        let weak = window.as_weak();
        window.on_menu_item_clicked(move |tile, id| {
            if let Some(window) = weak.upgrade() {
                clear_menu(&window, &for_menu_click);
            }
            let handler = for_menu_click.on_action.borrow().clone();
            if let Some(handler) = handler {
                handler(tile, id);
            }
        });
        window.set_align_left(kind == BarKind::Primary);
        let overlay = Self {
            kind,
            window,
            model,
            shared,
            owner: None,
            shown: false,
            last_actions: Vec::new(),
            last_zoom_snaps: Vec::new(),
            outside: OutsidePress::default(),
        };
        overlay.apply_tooltip_settings();
        tracing::info!(bar = kind.label(), "[Multiview: Overlay] Bar window created");
        Some(overlay)
    }

    fn apply_tooltip_settings(&self) {
        let (enabled, delay_ms) = TOOLTIPS.with(Cell::get);
        self.window.set_enable_tooltips(enabled);
        self.window.set_tooltip_delay_ms(delay_ms);
    }

    fn show(&mut self, request: &BarRequest) -> bool {
        self.sync_content(request);
        let bar_logical = (self.window.get_bar_width(), self.window.get_bar_height());
        // The pointer decision comes first and needs no X round trip: both rectangles are in host window coordinates.
        let Some(in_host) = place_bar_in(self.kind.corner(), request.area, bar_logical, request.scale) else {
            self.hide();
            return false;
        };
        let menu_zone = self.menu_zone_in_host(in_host);
        let revealed = request.forced
            || request.pointer.is_some_and(|p| {
                in_reveal_zone(in_host.bar, p, request.scale, request.reveal_distance_px)
                    || menu_zone.is_some_and(|zone| zone.contains(p.0, p.1))
            });
        if !revealed {
            self.hide();
            return false;
        }
        // A bar that shows itself to give feedback does not wait for the pointer to be over the Multiview.
        let Some((origin, in_front)) = self.host_state(request.host_xid) else {
            self.hide();
            return false;
        };
        if !in_front && !request.forced {
            self.hide();
            return false;
        }
        self.owner = Some(request.owner);
        self.place(in_host.translated(origin.0, origin.1), request.scale);
        true
    }

    /// While the drop-down is open: the area it covers in host window coordinates, including the gap up to the bar, so the
    /// bar stays while the pointer travels from the bar to the menu.
    fn menu_zone_in_host(&self, placement: BarPlacement) -> Option<Rect> {
        let menu = self.shared.x.borrow().menu?;
        let bar_bottom = placement.bar.y + placement.bar.h;
        let top = (placement.window.y + menu.y).min(bar_bottom);
        Some(Rect::new(
            placement.window.x + menu.x,
            top,
            menu.w,
            placement.window.y + menu.y + menu.h - top,
        ))
    }

    fn pointer_state(&self) -> Option<((i32, i32), bool)> {
        self.shared.x.borrow().server.as_ref()?.pointer_and_buttons()
    }

    /// See `sync_zoom`.
    fn sync_zoom(&mut self, zoom: Option<&ZoomBubbleView>) {
        let Some(zoom) = zoom else {
            if self.window.get_zoom_open() {
                self.window.set_zoom_open(false);
                if !self.window.get_menu_open() {
                    self.window.set_menu_owner_index(-1);
                }
            }
            return;
        };
        if self.window.get_menu_open() {
            // One bubble at a time.
            clear_menu(&self.window, &self.shared);
        }
        let opener = self.last_actions.iter().position(|a| a.id == MediaActionId::Zoom.code());
        self.window.set_menu_owner_index(opener.map_or(-1, |i| i as i32));
        self.window.set_tile(zoom.tile as i32);
        self.window.set_zoom_permille(zoom.permille);
        self.window.set_zoom_min_permille(zoom.min_permille);
        self.window.set_zoom_max_permille(zoom.max_permille);
        if self.last_zoom_snaps != zoom.snaps {
            self.window.set_zoom_snaps(ModelRc::new(VecModel::from(zoom.snaps.clone())));
            self.last_zoom_snaps.clone_from(&zoom.snaps);
        }
        if !self.window.get_zoom_open() {
            self.window.set_zoom_open(true);
            tracing::debug!("[Multiview: Overlay] Zoom slider opened");
        }
    }

    /// See `service_menu`.
    fn service_menu(&mut self) -> bool {
        if self.window.get_zoom_open() {
            // The slider stays as long as the Multiview says so, wherever the pointer is.
            return true;
        }
        if !self.window.get_menu_open() {
            return false;
        }
        let Some((pointer, down)) = self.pointer_state() else {
            return true;
        };
        let inside: Vec<Rect> = {
            let state = self.shared.x.borrow();
            match state.placed {
                Some(placed) => std::iter::once(placed.bar)
                    .chain(state.menu.map(|m| Rect::new(placed.window.x + m.x, placed.window.y + m.y, m.w, m.h)))
                    .collect(),
                None => Vec::new(),
            }
        };
        if self.outside.poll(pointer, down, &inside) {
            tracing::debug!("[Multiview: Overlay] Click outside the bar menu; closing it");
            self.close_menu();
            return false;
        }
        true
    }

    /// Closes the drop-down. Returns whether one was open.
    fn close_menu(&self) -> bool {
        let was_open = self.window.get_menu_open();
        clear_menu(&self.window, &self.shared);
        was_open
    }

    /// Where the host window is on the screen, and whether it is in front at the pointer. The overlay window itself counts
    /// as the host: the pointer is on the bar then.
    fn host_state(&self, host_xid: u64) -> Option<((i32, i32), bool)> {
        let x = self.shared.x.borrow();
        let server = x.server.as_ref()?;
        let origin = server.origin_in_root(host_xid)?;
        let host_top = server.top_level_of(host_xid)?;
        let bars = OVERLAY_XIDS.with(Cell::get);
        let in_front = server
            .top_level_at_pointer()
            .is_some_and(|front| front == host_top || bars.contains(&Some(front)));
        Some((origin, in_front))
    }

    fn sync_content(&mut self, request: &BarRequest) {
        *self.shared.on_action.borrow_mut() = Some(request.on_action.clone());
        if self.last_actions != request.actions {
            sync_model(&self.model, request.actions.clone());
            self.last_actions.clone_from(&request.actions);
        }
        if self.window.get_tile() != request.tile {
            self.close_menu();
        }
        self.window.set_tile(request.tile);
    }

    fn place(&mut self, placement: BarPlacement, scale: f32) {
        let (moved, appeared) = {
            let state = self.shared.x.borrow();
            (state.placed != Some(placement), !self.shown)
        };
        if appeared {
            self.refresh_glass();
        }
        if moved || appeared {
            let window = self.window.window();
            window.set_size(PhysicalSize::new(
                placement.window.w.max(1) as u32,
                placement.window.h.max(1) as u32,
            ));
            window.set_position(PhysicalPosition::new(placement.window.x, placement.window.y));
        }
        if appeared {
            if let Err(e) = self.window.show() {
                tracing::warn!(error = %e, "[Multiview: Overlay] A bar window could not be shown");
                return;
            }
            self.shown = true;
            tracing::debug!(bar = self.kind.label(), "[Multiview: Overlay] Bar shown");
        }
        let mut state = self.shared.x.borrow_mut();
        state.scale = scale;
        state.bar_radius = (self.window.get_bar_radius() * scale).round() as i32;
        state.tooltip_radius = (self.window.get_tooltip_radius() * scale).round() as i32;
        state.menu_radius = (self.window.get_menu_radius() * scale).round() as i32;
        state.placed = Some(placement);
        let first_xid = state.xid.is_none();
        if first_xid {
            state.xid = get_raw_window_xid(self.window.window());
            let kind = self.kind;
            OVERLAY_XIDS.with(|ids| {
                let mut all = ids.get();
                all[kind.index()] = state.xid;
                ids.set(all);
            });
        }
        if let (Some(server), Some(xid)) = (state.server.as_ref(), state.xid)
            && (moved || appeared || first_xid)
        {
            server.raise(xid);
        }
        if moved || appeared || first_xid {
            state.apply_shape();
        }
    }

    /// Glass if a compositing manager blends the window and the window has an alpha channel, solid otherwise.
    fn refresh_glass(&mut self) {
        let translucent = {
            let state = self.shared.x.borrow();
            state.server.as_ref().is_some_and(|server| {
                server.compositor_running()
                    && state.xid.is_none_or(|xid| server.window_depth(xid) == Some(32))
            })
        };
        let changed = {
            let mut state = self.shared.x.borrow_mut();
            std::mem::replace(&mut state.translucent, translucent) != translucent
        };
        if changed {
            tracing::info!(translucent, "[Multiview: Overlay] Bar style");
        }
        self.window.set_translucent(translucent);
    }

    fn hide(&mut self) {
        if !self.shown {
            return;
        }
        self.window.invoke_reset_tooltip();
        self.close_menu();
        self.window.set_zoom_open(false);
        if let Err(e) = self.window.hide() {
            tracing::warn!(error = %e, "[Multiview: Overlay] A bar window could not be hidden");
        }
        self.shown = false;
        self.owner = None;
        let mut state = self.shared.x.borrow_mut();
        state.placed = None;
        state.tooltip = None;
        tracing::debug!("[Multiview: Overlay] Bar hidden");
    }
}
