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
use crate::viewer::multiview::bar_placement::{BarCorner, BarPlacement, in_reveal_zone, place_bar_in, rounded_region};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::view::ZoomBubbleView;
use crate::viewer::{OverlayServer, XRectangle, get_raw_window_xid};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
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
    /// The tile's stable key (the bar window belongs to the tile, whatever its place in the row; 0 for the primary bar).
    pub key: TileKey,
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
    /// Polls the pointer while the menu of a media bar is open.
    static MENU_TIMER: Timer = Timer::default();
    /// The bar windows that exist. A window is created when it is first needed and lives until the application ends. The primary
    /// bar has one; media bars are a pool: every tile that has a bar takes a window for as long as it has the bar, and hands it back
    /// afterwards, so the windows follow their tiles (not their places in the row) however media come and go.
    static OVERLAYS: RefCell<Vec<Rc<RefCell<Overlay>>>> = const { RefCell::new(Vec::new()) };
    /// The native windows of the bars: a pointer on any of them still counts as being on the Multiview.
    static OVERLAY_XIDS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// The tile whose bar button was clicked last (a menu that button opens belongs to that bar).
    static LAST_CLICKED: Cell<Option<TileKey>> = const { Cell::new(None) };
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
    for cell in all_overlays() {
        if let Ok(overlay) = cell.try_borrow() {
            overlay.apply_tooltip_settings();
        }
    }
}

/// Creates the primary bar window and the first media bar window ahead of their first reveal (a Multiview opens), so that creating
/// them does not delay the first appearance of a bar.
pub fn prepare() {
    prepare_media_bars(1);
    if !all_overlays()
        .iter()
        .any(|cell| cell.try_borrow().is_ok_and(|o| o.kind == BarKind::Primary))
    {
        add_overlay(BarKind::Primary);
    }
}

/// Makes sure there are windows for the media bars of `count` tiles (not shown yet): a window created at the moment its bar is first
/// revealed would show black for a moment before its first frame is drawn.
pub fn prepare_media_bars(count: usize) {
    let have = overlays_of(BarKind::Media).len();
    for _ in have..count.min(MAX_MEDIA_BARS) {
        add_overlay(BarKind::Media);
    }
}

/// More tiles than this never exist at once (see `MULTIVIEW_MAX_TILES`); the pool never grows beyond it.
const MAX_MEDIA_BARS: usize = 8;

fn add_overlay(kind: BarKind) -> Option<Rc<RefCell<Overlay>>> {
    let overlay = Rc::new(RefCell::new(Overlay::new(kind)?));
    OVERLAYS.with(|all| all.borrow_mut().push(overlay.clone()));
    Some(overlay)
}

/// The window of the bar of `request`: the primary bar's, or the media bar window `request.owner` has for the tile (a free one of
/// the pool, if the tile has none yet; one is created when all are in use).
fn overlay_for(request: &BarRequest) -> Option<Rc<RefCell<Overlay>>> {
    if request.kind == BarKind::Primary {
        return overlays_of(BarKind::Primary)
            .into_iter()
            .next()
            .or_else(|| add_overlay(BarKind::Primary));
    }
    let media = overlays_of(BarKind::Media);
    let assigned = |cell: &Rc<RefCell<Overlay>>| cell.try_borrow().ok().and_then(|o| o.assigned);
    if let Some(cell) = media
        .iter()
        .find(|cell| assigned(cell) == Some((request.owner, request.key)))
    {
        return Some(cell.clone());
    }
    let free = media.iter().find(|cell| cell.try_borrow().is_ok_and(|o| o.is_free()));
    let cell = match free {
        Some(cell) => cell.clone(),
        None => add_overlay(BarKind::Media)?,
    };
    cell.try_borrow_mut().ok()?.assign(request.owner, request.key);
    Some(cell)
}

/// Shows the bar for the tile of `request` while the pointer is close enough to it (or the bar is `forced`), or hides it if the
/// pointer is not, the tile cannot carry a bar or the host is not the window in front. Returns whether the bar is visible.
pub fn show(request: &BarRequest) -> bool {
    let Some(cell) = overlay_for(request) else { return false };
    let Ok(mut overlay) = cell.try_borrow_mut() else {
        tracing::debug!("[Multiview: Overlay] Skipped a re-entrant call");
        return false;
    };
    overlay.show(request)
}

/// Opens the drop-down menu below the bar whose button was clicked last (the items are shown with the rows of a context menu); the
/// next click on an item calls the bar's action handler with the id of the item. A bar that is not shown ignores this.
pub fn open_menu(kind: BarKind, opener_action: i32, items: &[CtxMenuItem]) {
    let clicked = LAST_CLICKED.with(Cell::get);
    let cell = overlays_of(kind).into_iter().find(|cell| {
        cell.try_borrow()
            .is_ok_and(|o| kind == BarKind::Primary || (o.shown && o.assigned.map(|(_, key)| key) == clicked))
    });
    if let Some(cell) = cell
        && let Ok(mut overlay) = cell.try_borrow_mut()
        && overlay.shown
    {
        let opener = overlay.last_actions.iter().position(|a| a.id == opener_action);
        overlay
            .window
            .set_menu_owner_index(opener.map_or(-1, rustview_core::util::cast::SaturatingAs::saturating_as::<i32>));
        overlay
            .window
            .set_menu_items(CtxMenuDispatcher::items_to_slint_model(items, 0));
        overlay.window.set_menu_open(true);
        let down = overlay.pointer_state().is_some_and(|(_, down)| down);
        overlay.outside.arm(down);
    }
    MENU_TIMER.with(|timer| {
        timer.start(TimerMode::Repeated, Duration::from_millis(MENU_POLL_MS), move || {
            if !poll_menu(kind) {
                MENU_TIMER.with(Timer::stop);
            }
        });
    });
}

/// One poll of the open menus of `kind`, whoever shows the bars. Returns whether a menu is still open.
fn poll_menu(kind: BarKind) -> bool {
    let mut open = false;
    for cell in overlays_of(kind) {
        match cell.try_borrow_mut() {
            Ok(mut overlay) => open |= overlay.service_menu(),
            Err(_) => open = true,
        }
    }
    open
}

/// Called on every tick of a host: the open menus of the media bars `owner` shows close on a click outside bar and menu (or when
/// the user leaves the application).
pub fn service_menus(owner: BarOwner) {
    for cell in overlays_of(BarKind::Media) {
        if let Ok(mut overlay) = cell.try_borrow_mut()
            && overlay.assigned.is_some_and(|(o, _)| o == owner)
        {
            overlay.service_menu();
        }
    }
}

/// The tiles `owner` shows whose media bar has a menu open: their bars stay (and follow their tiles) until it closes.
pub fn open_menu_keys(owner: BarOwner) -> Vec<TileKey> {
    overlays_of(BarKind::Media)
        .into_iter()
        .filter_map(|cell| {
            let overlay = cell.try_borrow().ok()?;
            let (assigned_to, key) = overlay.assigned?;
            (assigned_to == owner && overlay.shown && overlay.window.get_menu_open()).then_some(key)
        })
        .collect()
}

/// Shows, updates or closes the zoom sliders that hang below the media bars, as the Multiview that shows the bars says: each tile
/// that has a slider open is in `zooms`, every other bar of `owner` closes its slider.
pub fn sync_zooms(owner: BarOwner, zooms: &[ZoomBubbleView]) {
    for cell in overlays_of(BarKind::Media) {
        let Ok(mut overlay) = cell.try_borrow_mut() else {
            continue;
        };
        let Some((assigned_to, key)) = overlay.assigned else {
            continue;
        };
        if assigned_to == owner {
            overlay.sync_zoom(zooms.iter().find(|z| z.key == key));
        }
    }
}

/// Closes the drop-down menus of the bars `kind`. Returns whether one was open.
pub fn close_menu(kind: BarKind) -> bool {
    let mut closed = false;
    for cell in overlays_of(kind) {
        if let Ok(overlay) = cell.try_borrow() {
            closed |= overlay.close_menu();
        }
    }
    closed
}

/// Closes the drop-down menu of the bar whose button was clicked last, if it is open (the button of a menu toggles it; the menus of
/// other bars are not its business). Returns whether one was open.
pub fn close_clicked_menu(kind: BarKind) -> bool {
    let clicked = LAST_CLICKED.with(Cell::get);
    overlays_of(kind)
        .into_iter()
        .find(|cell| {
            cell.try_borrow()
                .is_ok_and(|o| kind == BarKind::Primary || o.assigned.map(|(_, key)| key) == clicked)
        })
        .is_some_and(|cell| cell.try_borrow().is_ok_and(|o| o.close_menu()))
}

/// Hides the bars `owner` shows (a host may only hide a bar it shows itself).
pub fn hide(owner: BarOwner) {
    for kind in [BarKind::Media, BarKind::Primary] {
        hide_kind(kind, owner);
    }
}

/// Hides every bar of `kind` that `owner` shows.
pub fn hide_kind(kind: BarKind, owner: BarOwner) {
    hide_kind_except(kind, owner, &[]);
}

/// Hides the media bars (or the primary bar) `owner` shows, except those of the tiles in `keep`.
pub fn hide_kind_except(kind: BarKind, owner: BarOwner, keep: &[TileKey]) {
    for cell in overlays_of(kind) {
        let Ok(mut overlay) = cell.try_borrow_mut() else {
            tracing::debug!("[Multiview: Overlay] Skipped a re-entrant call");
            continue;
        };
        let kept = overlay
            .assigned
            .is_some_and(|(o, key)| o == owner && keep.contains(&key));
        if overlay.owner == Some(owner) && !kept {
            overlay.hide();
        }
    }
}

/// The application exits: all bar windows are hidden and destroyed (with their X connections). A bar that was never shown has no
/// window and is not created for this.
pub fn shutdown() {
    let taken: Vec<_> = OVERLAYS.with(|all| std::mem::take(&mut *all.borrow_mut()));
    for cell in taken {
        if let Ok(mut overlay) = cell.try_borrow_mut() {
            overlay.hide();
            tracing::debug!(bar = overlay.kind.label(), "[Shutdown: Windows] Bar window destroyed");
        }
    }
    OVERLAY_XIDS.with(|ids| ids.borrow_mut().clear());
}

fn all_overlays() -> Vec<Rc<RefCell<Overlay>>> {
    OVERLAYS.with(|all| all.borrow().clone())
}

fn overlays_of(kind: BarKind) -> Vec<Rc<RefCell<Overlay>>> {
    all_overlays()
        .into_iter()
        .filter(|cell| cell.try_borrow().is_ok_and(|o| o.kind == kind))
        .collect()
}

/// What the callbacks of the window need as well.
#[derive(Default)]
struct Shared {
    /// The tile this window's bar belongs to now (for `LAST_CLICKED`).
    tile_key: Cell<TileKey>,
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
    /// The floating percentage of the zoom slider, physical pixels relative to the window.
    pill: Option<Rect>,
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
fn clear_menu(window: &MultiviewBarWindow, shared: &Shared) {
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

fn xrectangle(rect: Rect) -> XRectangle {
    XRectangle {
        x: rect.x.clamp(0, i32::from(i16::MAX)).saturating_as::<i16>(),
        y: rect.y.clamp(0, i32::from(i16::MAX)).saturating_as::<i16>(),
        width: rect.w.clamp(1, i32::from(u16::MAX)).saturating_as::<u16>(),
        height: rect.h.clamp(1, i32::from(u16::MAX)).saturating_as::<u16>(),
    }
}

struct Overlay {
    kind: BarKind,
    /// The host and the tile this media bar window has been given to (`None` for the primary bar and for a window nobody uses).
    assigned: Option<(BarOwner, TileKey)>,
    window: MultiviewBarWindow,
    model: Rc<VecModel<MultiviewBarButtonView>>,
    shared: Rc<Shared>,
    owner: Option<BarOwner>,
    /// The window of the host the bar was last shown for (0 = none yet).
    host_xid: u64,
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

    fn apply_tooltip_settings(&self) {
        let (enabled, delay_ms) = TOOLTIPS.with(Cell::get);
        self.window.set_enable_tooltips(enabled);
        self.window.set_tooltip_delay_ms(delay_ms);
    }

    /// Nobody needs this window: it is hidden and has no menu open (it may be given to another tile).
    fn is_free(&self) -> bool {
        !self.shown && !self.window.get_menu_open()
    }

    /// Gives the window to the media bar of tile `key` of `owner`, with nothing left over from its last use.
    fn assign(&mut self, owner: BarOwner, key: TileKey) {
        self.close_menu();
        self.window.set_zoom_open(false);
        self.window.set_menu_owner_index(-1);
        self.assigned = Some((owner, key));
        self.shared.tile_key.set(key);
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
    fn host_active(&self, host_xid: u64) -> bool {
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
    fn host_in_use(&self) -> bool {
        self.host_xid != 0
            && (self.host_state(self.host_xid).is_some_and(|(_, in_front)| in_front) || self.host_active(self.host_xid))
    }

    /// While a drop-down menu is open: a click outside the bar and the menu closes it, and so does the user leaving the application
    /// (nothing of a bar may float over another window). Returns whether the menu is still open. A zoom slider is not looked after
    /// here: its tile keeps asking for its bar every tick.
    fn service_menu(&mut self) -> bool {
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
    fn close_menu(&self) -> bool {
        let was_open = self.window.get_menu_open();
        if was_open {
            clear_menu(&self.window, &self.shared);
        }
        was_open
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
            .is_some_and(|front| front == host_top || OVERLAY_XIDS.with(|ids| ids.borrow().contains(&front)));
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
    fn refresh_glass(&mut self) {
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
