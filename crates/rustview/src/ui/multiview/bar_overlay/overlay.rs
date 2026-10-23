//! One bar window: its X11 state, shape, placement and menu servicing.

use super::{ActionHandler, BarKind, BarOwner, BarRequest, LAST_CLICKED, NEXT_IS_OVERLAY, OVERLAY_XIDS, TOOLTIPS};
use crate::ui::multiview::views::sync_model;
use crate::ui::{ContextMenuItemModel, MultiviewBarButtonView, MultiviewBarWindow};
use crate::viewer::multiview::bar_menu::OutsidePress;
use crate::viewer::multiview::bar_placement::{BarPlacement, in_reveal_zone, place_bar_in, rounded_region};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::view::ZoomBubbleView;
use crate::viewer::{OverlayServer, XRectangle, get_raw_window_xid};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What the callbacks of the window need as well.
#[derive(Default)]
pub(super) struct Shared {
    /// The tile this window's bar belongs to now (for `LAST_CLICKED`).
    pub(super) tile_key: Cell<TileKey>,
    pub(super) on_action: RefCell<Option<ActionHandler>>,
    pub(super) x: RefCell<XState>,
}

/// The X side of the window: the private connection and what shapes the window currently has to have.
#[derive(Default)]
pub(super) struct XState {
    pub(super) server: Option<OverlayServer>,
    pub(super) xid: Option<u64>,
    /// The window is blended by a compositing manager (glass); otherwise it is solid and shaped.
    pub(super) translucent: bool,
    pub(super) placed: Option<BarPlacement>,
    /// The tooltip bubble inside the window, physical pixels relative to the window.
    pub(super) tooltip: Option<Rect>,
    /// The floating percentage of the zoom slider, physical pixels relative to the window.
    pub(super) pill: Option<Rect>,
    /// The drop-down menu inside the window, physical pixels relative to the window (the caret zone on top included).
    pub(super) menu: Option<Rect>,
    /// The caret of the menu: its center x (relative to the window) and its height, physical pixels.
    pub(super) menu_caret: (i32, i32),
    pub(super) bar_radius: i32,
    pub(super) tooltip_radius: i32,
    pub(super) menu_radius: i32,
    pub(super) scale: f32,
}

impl XState {
    /// The part of the window that takes the pointer is the bar; in the solid case the visible part is the bar and the
    /// tooltip bubble, with rounded corners.
    pub(super) fn apply_shape(&self) {
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
        if let Some(pill) = self.pill {
            region.extend(rounded_region(pill, pill.h / 2));
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
pub(super) fn clear_menu(window: &MultiviewBarWindow, shared: &Shared) {
    window.set_menu_open(false);
    window.set_menu_items(ModelRc::<ContextMenuItemModel>::default());
    // An open zoom slider hangs in the same bubble below the same opener, and its place keeps being the area of the bubble: letting
    // go of the opener or the area would make it jump to the left edge for a moment, or cut it out of the window's shape.
    if window.get_zoom_open() {
        return;
    }
    window.set_menu_owner_index(-1);
    if let Ok(mut state) = shared.x.try_borrow_mut() {
        state.menu = None;
        state.apply_shape();
    }
}

pub(super) fn xrectangle(rect: Rect) -> XRectangle {
    XRectangle {
        x: rect.x.clamp(0, i32::from(i16::MAX)).saturating_as::<i16>(),
        y: rect.y.clamp(0, i32::from(i16::MAX)).saturating_as::<i16>(),
        width: rect.w.clamp(1, i32::from(u16::MAX)).saturating_as::<u16>(),
        height: rect.h.clamp(1, i32::from(u16::MAX)).saturating_as::<u16>(),
    }
}

pub(super) struct Overlay {
    pub(super) kind: BarKind,
    /// The host and the tile this media bar window has been given to (`None` for the primary bar and for a window nobody uses).
    pub(super) assigned: Option<(BarOwner, TileKey)>,
    pub(super) window: MultiviewBarWindow,
    pub(super) model: Rc<VecModel<MultiviewBarButtonView>>,
    pub(super) shared: Rc<Shared>,
    pub(super) owner: Option<BarOwner>,
    /// The window of the host the bar was last shown for (0 = none yet).
    pub(super) host_xid: u64,
    pub(super) shown: bool,
    pub(super) last_actions: Vec<MultiviewBarButtonView>,
    pub(super) last_zoom_snaps: Vec<i32>,
    pub(super) outside: OutsidePress,
}

impl Overlay {
    pub(super) fn new(kind: BarKind) -> Option<Self> {
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
            LAST_CLICKED.with(|cell| cell.set(Some(for_click.tile_key.get())));
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
            let px = |logical: f32| (logical * scale).round().to_int::<i32>();
            state.tooltip =
                (visible && w > 2.0 && h > 2.0).then(|| Rect::new(px(x), px(y), px(w).max(1), px(h).max(1)));
            state.apply_shape();
        });
        let for_pill = shared.clone();
        window.on_pill_changed(move |visible, x, y, w, h| {
            let Ok(mut state) = for_pill.x.try_borrow_mut() else {
                return;
            };
            let scale = state.scale.max(0.5);
            let px = |logical: f32| (logical * scale).round().to_int::<i32>();
            state.pill = (visible && w > 2.0 && h > 2.0).then(|| Rect::new(px(x), px(y), px(w).max(1), px(h).max(1)));
            state.apply_shape();
        });
        let for_menu = shared.clone();
        window.on_menu_changed(move |visible, x, y, w, h, caret_x, caret_h| {
            let Ok(mut state) = for_menu.x.try_borrow_mut() else {
                return;
            };
            let scale = state.scale.max(0.5);
            let px = |logical: f32| (logical * scale).round().to_int::<i32>();
            state.menu = (visible && w > 2.0 && h > 2.0).then(|| Rect::new(px(x), px(y), px(w).max(1), px(h).max(1)));
            state.menu_caret = (px(caret_x), px(caret_h));
            state.apply_shape();
        });
        let for_menu_click = shared.clone();
        let weak = window.as_weak();
        window.on_menu_item_clicked(move |tile, id| {
            LAST_CLICKED.with(|cell| cell.set(Some(for_menu_click.tile_key.get())));
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
            assigned: None,
            window,
            model,
            shared,
            owner: None,
            host_xid: 0,
            shown: false,
            last_actions: Vec::new(),
            last_zoom_snaps: Vec::new(),
            outside: OutsidePress::default(),
        };
        overlay.apply_tooltip_settings();
        tracing::info!(bar = kind.label(), "[Multiview: Overlay] Bar window created");
        Some(overlay)
    }

    pub(super) fn apply_tooltip_settings(&self) {
        let (enabled, delay_ms) = TOOLTIPS.with(Cell::get);
        self.window.set_enable_tooltips(enabled);
        self.window.set_tooltip_delay_ms(delay_ms);
    }

    /// Nobody needs this window: it is hidden and has no menu open (it may be given to another tile).
    pub(super) fn is_free(&self) -> bool {
        !self.shown && !self.window.get_menu_open()
    }

    /// Gives the window to the media bar of tile `key` of `owner`, with nothing left over from its last use.
    pub(super) fn assign(&mut self, owner: BarOwner, key: TileKey) {
        self.close_menu();
        self.window.set_zoom_open(false);
        self.window.set_menu_owner_index(-1);
        self.assigned = Some((owner, key));
        self.shared.tile_key.set(key);
    }

    pub(super) fn show(&mut self, request: &BarRequest) -> bool {
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
        // A bar that gives feedback shows without the pointer, but never while the user works in another window.
        if !(in_front || request.forced && self.host_active(request.host_xid)) {
            self.hide();
            return false;
        }
        self.host_xid = request.host_xid;
        self.owner = Some(request.owner);
        self.place(in_host.translated(origin.0, origin.1), request.scale);
        true
    }

    /// While the drop-down is open: the area it covers in host window coordinates, including the gap up to the bar, so the
    /// bar stays while the pointer travels from the bar to the menu.
    pub(super) fn menu_zone_in_host(&self, placement: BarPlacement) -> Option<Rect> {
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

    pub(super) fn pointer_state(&self) -> Option<((i32, i32), bool)> {
        self.shared.x.borrow().server.as_ref()?.pointer_and_buttons()
    }

    /// See `sync_zoom`.
    pub(super) fn sync_zoom(&mut self, zoom: Option<&ZoomBubbleView>) {
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
        let opener = self
            .last_actions
            .iter()
            .position(|a| a.id == MediaActionId::Zoom.code());
        self.window
            .set_menu_owner_index(opener.map_or(-1, rustview_core::util::cast::SaturatingAs::saturating_as::<i32>));
        self.window.set_tile(zoom.tile.saturating_as::<i32>());
        self.window.set_zoom_permille(zoom.permille);
        self.window.set_zoom_min_permille(zoom.min_permille);
        self.window.set_zoom_max_permille(zoom.max_permille);
        if self.last_zoom_snaps != zoom.snaps {
            self.window
                .set_zoom_snaps(ModelRc::new(VecModel::from(zoom.snaps.clone())));
            self.last_zoom_snaps.clone_from(&zoom.snaps);
        }
        if !self.window.get_zoom_open() {
            self.window.set_zoom_open(true);
            tracing::debug!("[Multiview: Overlay] Zoom slider opened");
        }
    }

    /// Whether the window manager has the host window (or the window that holds it) as the active window.
    pub(super) fn host_active(&self, host_xid: u64) -> bool {
        let x = self.shared.x.borrow();
        let Some(server) = x.server.as_ref() else {
            return false;
        };
        let Some(active) = server.active_window() else {
            // A window manager that does not tell cannot be asked; the pointer decides alone then.
            return false;
        };
        active == host_xid
            || server
                .top_level_of(active)
                .is_some_and(|top| Some(top) == server.top_level_of(host_xid))
    }

    /// Whether the user is looking at the host: the pointer is over it (or over the bar) or it is the active window. A bar that
    /// stays by itself (open menu or slider) goes away when this is false, so it never floats over another application.
    pub(super) fn host_in_use(&self) -> bool {
        self.host_xid != 0
            && (self.host_state(self.host_xid).is_some_and(|(_, in_front)| in_front) || self.host_active(self.host_xid))
    }

    /// While a drop-down menu is open: a click outside the bar and the menu closes it, and so does the user leaving the application
    /// (nothing of a bar may float over another window). Returns whether the menu is still open. A zoom slider is not looked after
    /// here: its tile keeps asking for its bar every tick.
    pub(super) fn service_menu(&mut self) -> bool {
        if !self.shown || !self.window.get_menu_open() {
            return false;
        }
        if !self.host_in_use() {
            self.close_menu();
            return false;
        }
        let Some((pointer, down)) = self.pointer_state() else {
            return true;
        };
        let inside: Vec<Rect> = {
            let state = self.shared.x.borrow();
            match state.placed {
                Some(placed) => std::iter::once(placed.bar)
                    .chain(
                        state
                            .menu
                            .map(|m| Rect::new(placed.window.x + m.x, placed.window.y + m.y, m.w, m.h)),
                    )
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
    pub(super) fn close_menu(&self) -> bool {
        let was_open = self.window.get_menu_open();
        if was_open {
            clear_menu(&self.window, &self.shared);
        }
        was_open
    }

    /// Where the host window is on the screen, and whether it is in front at the pointer. The overlay window itself counts
    /// as the host: the pointer is on the bar then.
    pub(super) fn host_state(&self, host_xid: u64) -> Option<((i32, i32), bool)> {
        let x = self.shared.x.borrow();
        let server = x.server.as_ref()?;
        let origin = server.origin_in_root(host_xid)?;
        let host_top = server.top_level_of(host_xid)?;
        let in_front = server
            .top_level_at_pointer()
            .is_some_and(|front| front == host_top || OVERLAY_XIDS.with(|ids| ids.borrow().contains(&front)));
        Some((origin, in_front))
    }

    pub(super) fn sync_content(&mut self, request: &BarRequest) {
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

    pub(super) fn place(&mut self, placement: BarPlacement, scale: f32) {
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
                placement.window.w.max(1).saturating_as::<u32>(),
                placement.window.h.max(1).saturating_as::<u32>(),
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
        state.bar_radius = (self.window.get_bar_radius() * scale).round().to_int::<i32>();
        state.tooltip_radius = (self.window.get_tooltip_radius() * scale).round().to_int::<i32>();
        state.menu_radius = (self.window.get_menu_radius() * scale).round().to_int::<i32>();
        state.placed = Some(placement);
        let first_xid = state.xid.is_none();
        if first_xid {
            state.xid = get_raw_window_xid(self.window.window());
            if let Some(xid) = state.xid {
                OVERLAY_XIDS.with(|ids| ids.borrow_mut().push(xid));
            }
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
    pub(super) fn refresh_glass(&mut self) {
        let translucent = {
            let state = self.shared.x.borrow();
            state.server.as_ref().is_some_and(|server| {
                server.compositor_running() && state.xid.is_none_or(|xid| server.window_depth(xid) == Some(32))
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

    pub(super) fn hide(&mut self) {
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
