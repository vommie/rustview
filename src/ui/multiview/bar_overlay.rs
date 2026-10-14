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
use crate::ui::{MultiviewBarButtonView, MultiviewBarWindow};
use crate::viewer::multiview::bar_placement::{BarCorner, BarPlacement, in_reveal_zone, place_bar_in, rounded_region};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::{OverlayServer, XRectangle, get_raw_window_xid};
use slint::winit_030::winit::platform::x11::WindowAttributesExtX11;
use slint::winit_030::winit::window::WindowAttributes;
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

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

thread_local! {
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
    for kind in [BarKind::Media, BarKind::Primary] {
        with_overlay(kind, |overlay| overlay.apply_tooltip_settings());
    }
}

/// Shows the bar for the tile of `request` while the pointer is close enough to it, or hides it if the pointer is not, the
/// tile cannot carry a bar or the host is not the window in front. Returns whether the bar is visible.
pub fn show(request: &BarRequest) -> bool {
    with_overlay(request.kind, |overlay| overlay.show(request)).unwrap_or(false)
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
    bar_radius: i32,
    tooltip_radius: i32,
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
        server.set_input_region(xid, &[xrectangle(bar)]);
        if self.translucent {
            server.set_bounding_region(xid, None);
            return;
        }
        let mut region = rounded_region(bar, self.bar_radius);
        if let Some(tooltip) = self.tooltip {
            region.extend(rounded_region(tooltip, self.tooltip_radius));
        }
        server.set_bounding_region(xid, Some(&region));
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
        window.set_align_left(kind == BarKind::Primary);
        let overlay = Self {
            kind,
            window,
            model,
            shared,
            owner: None,
            shown: false,
            last_actions: Vec::new(),
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
        let revealed = request.forced
            || request
                .pointer
                .is_some_and(|p| in_reveal_zone(in_host.bar, p, request.scale, request.reveal_distance_px));
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
