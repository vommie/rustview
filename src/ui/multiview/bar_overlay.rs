//! The window of the action bar (UI thread only).
//!
//! One frameless transparent window floats above whichever Multiview shows a bar (the fullscreen window or the viewer tab).
//! It is an override-redirect window of its own, like the tooltips and popup menus of other toolkits: the window manager
//! leaves it alone (no focus is taken, nothing is stacked below the fullscreen window), and the compositing manager blends
//! it with everything below it, including the native mpv windows that nothing inside the Multiview window could blend
//! with. Where no compositing manager runs, the window cannot be blended: it is solid then and shaped like the bar, so it
//! does not show black corners. Placement and shapes are computed by the pure `bar_placement`; this module only turns them
//! into window calls.

use super::views::sync_model;
use crate::ui::{MultiviewActionView, MultiviewOverlayWindow};
use crate::viewer::multiview::bar_placement::{BarPlacement, place_bar, rounded_region};
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

/// Called with the tile and the action id of a clicked bar button.
type ActionHandler = Rc<dyn Fn(i32, i32)>;

/// What a host wants the bar to show.
pub struct BarRequest {
    pub owner: BarOwner,
    /// The window the Multiview is drawn in; the bar is only shown while that window is the one in front at the pointer
    /// (a window of another application on top of it must not get a bar floating above it).
    pub host_xid: u64,
    /// The hovered tile (handed back with a click) and its rectangle in the coordinates of the host window.
    pub tile: i32,
    pub tile_rect: Rect,
    /// Physical pixels per logical pixel of the host.
    pub scale: f32,
    pub actions: Vec<MultiviewActionView>,
    pub on_action: ActionHandler,
}

thread_local! {
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
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
    with_overlay(|overlay| overlay.apply_tooltip_settings());
}

/// Shows the bar for the tile of `request`, or hides it if the tile cannot carry a bar or the host is not the active
/// window. Returns the place of the bar in the coordinates of the host window (the pointer rests on it, see the hover
/// logic of the instance), `None` while there is no bar.
pub fn show(request: &BarRequest) -> Option<Rect> {
    with_overlay(|overlay| overlay.show(request)).flatten()
}

/// Hides the bar if `owner` is the host that shows it.
pub fn hide(owner: BarOwner) {
    with_overlay(|overlay| {
        if overlay.owner == Some(owner) {
            overlay.hide();
        }
    });
}

/// Runs `f` on the overlay, creating it on first use. Re-entrant calls (a click handler that hides the bar) are skipped.
fn with_overlay<R>(f: impl FnOnce(&mut Overlay) -> R) -> Option<R> {
    OVERLAY.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            tracing::debug!("[Multiview: Overlay] Skipped a re-entrant call");
            return None;
        };
        if slot.is_none() {
            *slot = Overlay::new();
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
    window: MultiviewOverlayWindow,
    model: Rc<VecModel<MultiviewActionView>>,
    shared: Rc<Shared>,
    owner: Option<BarOwner>,
    shown: bool,
    last_actions: Vec<MultiviewActionView>,
}

impl Overlay {
    fn new() -> Option<Self> {
        NEXT_IS_OVERLAY.with(|flag| flag.set(true));
        let created = MultiviewOverlayWindow::new();
        NEXT_IS_OVERLAY.with(|flag| flag.set(false));
        let window = match created {
            Ok(window) => window,
            Err(e) => {
                tracing::error!(error = %e, "[Multiview: Overlay] Failed to create the action bar window");
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
        let overlay = Self {
            window,
            model,
            shared,
            owner: None,
            shown: false,
            last_actions: Vec::new(),
        };
        overlay.apply_tooltip_settings();
        tracing::info!("[Multiview: Overlay] Action bar window created");
        Some(overlay)
    }

    fn apply_tooltip_settings(&self) {
        let (enabled, delay_ms) = TOOLTIPS.with(Cell::get);
        self.window.set_enable_tooltips(enabled);
        self.window.set_tooltip_delay_ms(delay_ms);
    }

    fn show(&mut self, request: &BarRequest) -> Option<Rect> {
        let Some((origin, in_front)) = self.host_state(request.host_xid) else {
            self.hide();
            return None;
        };
        if !in_front {
            self.hide();
            return None;
        }
        self.sync_content(request);
        let bar_logical = (self.window.get_bar_width(), self.window.get_bar_height());
        let tile = Rect::new(
            request.tile_rect.x + origin.0,
            request.tile_rect.y + origin.1,
            request.tile_rect.w,
            request.tile_rect.h,
        );
        let Some(placement) = place_bar(tile, bar_logical, request.scale) else {
            self.hide();
            return None;
        };
        self.owner = Some(request.owner);
        self.place(placement, request.scale);
        Some(Rect::new(
            placement.bar.x - origin.0,
            placement.bar.y - origin.1,
            placement.bar.w,
            placement.bar.h,
        ))
    }

    /// Where the host window is on the screen, and whether it is in front at the pointer. The overlay window itself counts
    /// as the host: the pointer is on the bar then.
    fn host_state(&self, host_xid: u64) -> Option<((i32, i32), bool)> {
        let x = self.shared.x.borrow();
        let server = x.server.as_ref()?;
        let origin = server.origin_in_root(host_xid)?;
        let host_top = server.top_level_of(host_xid)?;
        let in_front = server
            .top_level_at_pointer()
            .is_some_and(|front| front == host_top || Some(front) == x.xid);
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
                tracing::warn!(error = %e, "[Multiview: Overlay] The action bar window could not be shown");
                return;
            }
            self.shown = true;
            tracing::debug!("[Multiview: Overlay] Action bar shown");
        }
        let mut state = self.shared.x.borrow_mut();
        state.scale = scale;
        state.bar_radius = (self.window.get_bar_radius() * scale).round() as i32;
        state.tooltip_radius = (self.window.get_tooltip_radius() * scale).round() as i32;
        state.placed = Some(placement);
        let first_xid = state.xid.is_none();
        if first_xid {
            state.xid = get_raw_window_xid(self.window.window());
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
            tracing::info!(translucent, "[Multiview: Overlay] Action bar style");
        }
        self.window.set_translucent(translucent);
    }

    fn hide(&mut self) {
        if !self.shown {
            return;
        }
        self.window.invoke_reset_tooltip();
        if let Err(e) = self.window.hide() {
            tracing::warn!(error = %e, "[Multiview: Overlay] The action bar window could not be hidden");
        }
        self.shown = false;
        self.owner = None;
        let mut state = self.shared.x.borrow_mut();
        state.placed = None;
        state.tooltip = None;
        tracing::debug!("[Multiview: Overlay] Action bar hidden");
    }
}
