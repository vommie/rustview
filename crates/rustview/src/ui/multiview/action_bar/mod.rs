//! The action bars of a Multiview as rows of a Slint model (UI thread only): the fullscreen window and the tab view each have one
//! `ActionBarModel`, and their canvas draws a `MultiviewActionBar` per row: the media bar of every tile that shows one and the
//! primary bar. Which bars show and where is decided by the instance (`media_bar_targets`, `primary_bar_target`); this module turns
//! the targets into rows and keeps the one drop-down a host can have open ("Open With", the Save menu).
//!
//! A row belongs to its bar (the tile's `TileKey`, or the primary bar) for as long as the bar shows: rows are updated in place,
//! never rebuilt, so tooltips and the zoom slider survive every tick. A bar with an open bubble (drop-down or zoom slider) is the
//! last row, so no other bar is drawn over it. The events of every bar of a window arrive at its `ActionBarBus` with the key.

mod convert;

use crate::ctx_menu::{CtxMenuDispatcher, CtxMenuItem};
use crate::ui::{
    ActionBarBus, ContextMenuItemModel, MultiviewActionBarView, MultiviewBarButtonView, MultiviewBarMetrics,
};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::route::HostMode;
use crate::viewer::multiview::view::{MediaBarTarget, PrimaryBarTarget, ZoomBubbleView};
use rustview_core::settings::schema::{
    MULTIVIEW_BAR_ARROW_LOGICAL_PX, MULTIVIEW_BAR_BUTTON_LOGICAL_PX, MULTIVIEW_BAR_INSET_LOGICAL_PX,
    MULTIVIEW_BAR_SPACING_LOGICAL_PX,
};
use rustview_core::util::cast::WrappingAs;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Which bar a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarId {
    Primary,
    Media(TileKey),
}

/// What a row keeps between ticks: the lists inside it and what they were made from.
struct Slot {
    id: BarId,
    actions: Rc<VecModel<MultiviewBarButtonView>>,
    last_actions: Vec<MultiviewBarButtonView>,
    snaps: Rc<VecModel<i32>>,
    last_snaps: Vec<i32>,
}

impl Slot {
    fn new(id: BarId) -> Self {
        Self {
            id,
            actions: Rc::default(),
            last_actions: Vec::new(),
            snaps: Rc::default(),
            last_snaps: Vec::new(),
        }
    }
}

/// The drop-down open below a media bar.
struct OpenMenu {
    key: TileKey,
    /// The action code of the button it belongs to (its bubble points at it).
    opener: i32,
    items: ModelRc<ContextMenuItemModel>,
}

/// What the instance offers the bars of a host on this tick.
#[derive(Default)]
pub struct BarTargets {
    pub media: Vec<MediaBarTarget>,
    pub primary: Option<PrimaryBarTarget>,
    /// The zoom sliders the user has open, one per tile.
    pub zoom: Vec<ZoomBubbleView>,
}

/// The bars one canvas draws.
#[derive(Default)]
pub struct ActionBarModel {
    rows: Rc<VecModel<MultiviewActionBarView>>,
    /// One per row, in the same order.
    slots: Vec<Slot>,
    menu: Option<OpenMenu>,
}

impl ActionBarModel {
    /// The model to hand to the canvas (once; it stays the same).
    fn model(&self) -> ModelRc<MultiviewActionBarView> {
        ModelRc::from(self.rows.clone())
    }

    /// Shows the bars of `targets` (and only those), with `scale` physical pixels per logical pixel. Returns whether a bar shows.
    fn sync(&mut self, targets: BarTargets, scale: f32) -> bool {
        let BarTargets { media, primary, zoom } = targets;
        if let Some(menu) = &self.menu {
            // A drop-down goes with its bar, and gives way to a zoom slider of its tile (one bubble at a time).
            let bar_shown = media.iter().any(|t| t.key == menu.key);
            if !bar_shown || zoom.iter().any(|z| z.key == menu.key) {
                tracing::debug!(tile = menu.key, bar_shown, "[Multiview: ActionBar] Drop-down closed");
                self.menu = None;
            }
        }
        let open_bubble =
            |key: TileKey| zoom.iter().any(|z| z.key == key) || self.menu.as_ref().is_some_and(|m| m.key == key);
        let mut wanted: Vec<(BarId, Option<&MediaBarTarget>)> = Vec::with_capacity(media.len() + 1);
        if primary.is_some() {
            wanted.push((BarId::Primary, None));
        }
        let (bubbles, plain): (Vec<&MediaBarTarget>, Vec<&MediaBarTarget>) =
            media.iter().partition(|t| open_bubble(t.key));
        wanted.extend(plain.into_iter().chain(bubbles).map(|t| (BarId::Media(t.key), Some(t))));
        for (index, (id, target)) in wanted.iter().enumerate() {
            match self.slots.iter().position(|slot| slot.id == *id) {
                Some(found) if found == index => {}
                Some(found) => {
                    let slot = self.remove(found);
                    self.insert(index, slot);
                }
                None => {
                    self.insert(index, Slot::new(*id));
                    tracing::debug!(bar = ?id, "[Multiview: ActionBar] Bar shown");
                }
            }
            let row = match (id, target, primary.as_ref()) {
                (BarId::Primary, _, Some(primary)) => convert::primary_row(&mut self.slots[index], primary, scale),
                (BarId::Media(key), Some(target), _) => {
                    let bubble = zoom.iter().find(|z| z.key == *key);
                    let menu = self.menu.as_ref().filter(|m| m.key == *key);
                    convert::media_row(&mut self.slots[index], target, menu, bubble, scale)
                }
                _ => continue,
            };
            if self.rows.row_data(index).as_ref() != Some(&row) {
                self.rows.set_row_data(index, row);
            }
        }
        while self.slots.len() > wanted.len() {
            let slot = self.remove(self.slots.len() - 1);
            tracing::debug!(bar = ?slot.id, "[Multiview: ActionBar] Bar hidden");
        }
        !wanted.is_empty()
    }

    /// No bar shows any more (the host closes, hides or is locked).
    fn clear(&mut self) {
        if !self.slots.is_empty() {
            tracing::debug!(bars = self.slots.len(), "[Multiview: ActionBar] Bars hidden");
        }
        self.menu = None;
        self.slots.clear();
        self.rows.set_vec(Vec::new());
    }

    /// The tile whose bar reported `key` (the 32-bit key of the canvas), if it is one of these bars.
    fn tile(&self, key: i32) -> Option<TileKey> {
        self.slots.iter().find_map(|slot| match slot.id {
            BarId::Media(tile) if tile.wrapping_as::<i32>() == key => Some(tile),
            _ => None,
        })
    }

    /// Opens the drop-down `items` below the bar of `key`, pointing at the button `opener`. A bar that is not shown ignores it.
    fn open_menu(&mut self, key: TileKey, opener: i32, items: &[CtxMenuItem]) {
        let Some(index) = self.slots.iter().position(|slot| slot.id == BarId::Media(key)) else {
            return;
        };
        let menu = OpenMenu {
            key,
            opener,
            items: CtxMenuDispatcher::items_to_slint_model(items, 0),
        };
        if let Some(mut row) = self.rows.row_data(index) {
            convert::show_menu(&mut row, &self.slots[index], Some(&menu));
            self.rows.set_row_data(index, row);
        }
        self.menu = Some(menu);
        tracing::debug!(
            tile = key,
            opener,
            items = items.len(),
            "[Multiview: ActionBar] Drop-down opened"
        );
    }

    /// Closes the drop-down (of the bar of `key` only, when given). Returns whether one was open.
    fn close_menu(&mut self, key: Option<TileKey>) -> bool {
        let Some(menu) = self.menu.take_if(|m| key.is_none_or(|k| k == m.key)) else {
            return false;
        };
        if let Some(index) = self.slots.iter().position(|slot| slot.id == BarId::Media(menu.key))
            && let Some(mut row) = self.rows.row_data(index)
        {
            convert::show_menu(&mut row, &self.slots[index], None);
            self.rows.set_row_data(index, row);
        }
        tracing::debug!(tile = menu.key, "[Multiview: ActionBar] Drop-down closed");
        true
    }

    fn insert(&mut self, index: usize, slot: Slot) {
        // The row is filled right after; until then it is an empty bar at the same place.
        self.rows.insert(index, MultiviewActionBarView::default());
        self.slots.insert(index, slot);
    }

    fn remove(&mut self, index: usize) -> Slot {
        self.rows.remove(index);
        self.slots.remove(index)
    }
}

thread_local! {
    /// The bars of the fullscreen window and of the tab view.
    static FULLSCREEN: RefCell<ActionBarModel> = RefCell::new(ActionBarModel::default());
    static TAB_VIEW: RefCell<ActionBarModel> = RefCell::new(ActionBarModel::default());
    /// The host and the tile whose media bar button was clicked last (a drop-down that button opens belongs to that bar).
    static LAST_CLICKED: Cell<Option<(HostMode, TileKey)>> = const { Cell::new(None) };
}

/// Runs `f` on the bars of `host` (nothing for a host without action bars, or on a re-entrant call).
fn with_model<R>(host: HostMode, f: impl FnOnce(&mut ActionBarModel) -> R) -> Option<R> {
    let cell = match host {
        HostMode::Fullscreen => &FULLSCREEN,
        HostMode::TabView => &TAB_VIEW,
        HostMode::TabGrid | HostMode::Hidden => return None,
    };
    cell.with(|model| {
        let Ok(mut model) = model.try_borrow_mut() else {
            tracing::debug!("[Multiview: ActionBar] Skipped a re-entrant call");
            return None;
        };
        Some(f(&mut model))
    })
}

/// What a bar of a window reported, for its host to carry out.
#[derive(Debug, Clone, Copy)]
pub enum BarEvent {
    /// A button of the media bar of a tile (or its zoom slider), or an item of its drop-down (the drop-down is closed already).
    Media(TileKey, i32),
    /// A button of the primary bar.
    Primary(i32),
}

/// Hands the bar model of `host` to its canvas (`set_model`) and the bar events of `window` to `on_event`. Once per window, when it
/// is made. The sizes of the bars are given to the window too (`MultiviewBarMetrics`).
pub fn bind<W: ComponentHandle>(
    host: HostMode,
    window: &W,
    set_model: impl FnOnce(ModelRc<MultiviewActionBarView>),
    on_event: impl Fn(BarEvent) + 'static,
) where
    for<'a> ActionBarBus<'a>: slint::Global<'a, W>,
    for<'a> MultiviewBarMetrics<'a>: slint::Global<'a, W>,
{
    if let Some(model) = with_model(host, |bars| bars.model()) {
        set_model(model);
    }
    let metrics = window.global::<MultiviewBarMetrics>();
    metrics.set_button_size(MULTIVIEW_BAR_BUTTON_LOGICAL_PX);
    metrics.set_spacing(MULTIVIEW_BAR_SPACING_LOGICAL_PX);
    metrics.set_inset(MULTIVIEW_BAR_INSET_LOGICAL_PX);
    metrics.set_arrow_width(MULTIVIEW_BAR_ARROW_LOGICAL_PX);
    window.global::<ActionBarBus>().on_event(move |kind, key, code| {
        let tile = with_model(host, |bars| bars.tile(key)).flatten();
        match (kind, tile) {
            (0, Some(tile)) => {
                LAST_CLICKED.with(|cell| cell.set(Some((host, tile))));
                on_event(BarEvent::Media(tile, code));
            }
            (1, Some(tile)) => {
                LAST_CLICKED.with(|cell| cell.set(Some((host, tile))));
                with_model(host, |bars| bars.close_menu(Some(tile)));
                on_event(BarEvent::Media(tile, code));
            }
            (2, _) => on_event(BarEvent::Primary(code)),
            (3, _) => {
                tracing::debug!("[Multiview: ActionBar] Press outside the drop-down");
                with_model(host, |bars| bars.close_menu(None));
            }
            _ => tracing::debug!(kind, key, "[Multiview: ActionBar] Event of a bar that is gone"),
        }
    });
}

/// What `instance` offers the bars of `host` now (nothing while `active` is false: a dialog locks the view, or the host does not
/// show it). The instance lock is released before the caller syncs the bars.
pub fn targets(host: HostMode, instance: &SharedInstance, active: bool) -> BarTargets {
    if !active {
        return BarTargets::default();
    }
    let open_menus = open_menu_keys(host);
    let guard = instance.lock();
    BarTargets {
        media: guard.media_bar_targets(&open_menus),
        primary: guard.primary_bar_target(),
        zoom: guard.zoom_bubbles(),
    }
}

/// Shows, updates or hides the bars of `host`. Returns whether a bar shows.
pub fn sync(host: HostMode, targets: BarTargets, scale: f32) -> bool {
    with_model(host, |bars| bars.sync(targets, scale)).unwrap_or(false)
}

/// No bar of `host` shows any more.
pub fn clear(host: HostMode) {
    with_model(host, ActionBarModel::clear);
}

/// The tiles of `host` whose media bar has a drop-down open: their bars stay (and follow their tiles) until it closes.
pub fn open_menu_keys(host: HostMode) -> Vec<TileKey> {
    with_model(host, |bars| bars.menu.as_ref().map(|m| m.key))
        .flatten()
        .into_iter()
        .collect()
}

/// Opens the drop-down `items` below the media bar whose button was clicked last, pointing at its button `opener`; a click on an
/// item reports the item's id like a button.
pub fn open_menu(opener: i32, items: &[CtxMenuItem]) {
    if let Some((host, key)) = LAST_CLICKED.with(Cell::get) {
        with_model(host, |bars| bars.open_menu(key, opener, items));
    }
}

/// Closes the drop-down of the media bar whose button was clicked last, if it is open (the button of a drop-down toggles it).
/// Returns whether one was open.
pub fn close_clicked_menu() -> bool {
    LAST_CLICKED
        .with(Cell::get)
        .and_then(|(host, key)| with_model(host, |bars| bars.close_menu(Some(key))))
        .unwrap_or(false)
}
