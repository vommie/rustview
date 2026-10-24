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

use crate::ctx_menu::{CtxMenuDispatcher, CtxMenuItem};
use crate::ui::MultiviewBarButtonView;
use crate::viewer::multiview::bar_placement::BarCorner;
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::view::ZoomBubbleView;
use slint::winit_030::winit::platform::x11::WindowAttributesExtX11;
use slint::winit_030::winit::window::WindowAttributes;
use slint::{Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

mod overlay;
use overlay::Overlay;

/// Which host shows the bar. The window is shared, so a host may only hide a bar it shows itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarOwner {
    Fullscreen,
    TabView,
    /// The tab grid (only the small video control bars).
    TabGrid,
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

/// Creates a window that is override-redirect like the bar windows (the hint window of the Multiview is one too).
pub(super) fn create_override_redirect<T>(
    create: impl FnOnce() -> Result<T, slint::PlatformError>,
) -> Result<T, slint::PlatformError> {
    NEXT_IS_OVERLAY.with(|flag| flag.set(true));
    let created = create();
    NEXT_IS_OVERLAY.with(|flag| flag.set(false));
    created
}

/// Another window that floats over the Multiview like the bars (a video control bar): a pointer on it still counts as being on
/// the Multiview.
pub(super) fn register_overlay_xid(xid: u64) {
    OVERLAY_XIDS.with(|ids| {
        let mut ids = ids.borrow_mut();
        if !ids.contains(&xid) {
            ids.push(xid);
        }
    });
}

/// Whether `xid` is one of the windows that float over the Multiview (the bars).
pub(super) fn is_overlay_xid(xid: u64) -> bool {
    OVERLAY_XIDS.with(|ids| ids.borrow().contains(&xid))
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
