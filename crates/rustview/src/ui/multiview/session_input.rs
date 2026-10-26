//! Session behavior around input: window geometry, the tick, clicks, gutter drag, swap, rename and the live selection.
//! Everything here translates a UI event into a call on the instance and syncs the result into the window.

use super::bar_hosting::{self, BarHost, BarTargets};
use super::bar_overlay::{self, BarKind, BarOwner};
use super::cursor;
use super::eligibility;
use super::session::Session;
use crate::system::MonitorInfo;
use crate::ui::MultiviewFullscreenWindow;
use crate::viewer::PointerProbe;
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::click::ClickKind;
use crate::viewer::multiview::command::MultiviewCommand;
use crate::viewer::multiview::media_actions::{BarClick, MediaActionId};
use crate::viewer::multiview::primary_actions::PrimaryActionId;
use crate::viewer::multiview::route::{HostMode, OriginKind, RoutedEvent, UiAction, route};
use crate::viewer::multiview::tile::TileSeed;
use crate::viewer::video_surface::FULLSCREEN_WINDOW;
use rustview_core::library::models::MediaId;
use rustview_core::util::cast::SaturatingAs;
use slint::ComponentHandle;

/// The pointer in the coordinates of the fullscreen window, which covers its monitor.
fn pointer_in_window(probe: Option<&PointerProbe>, monitor: &MonitorInfo) -> Option<(i32, i32)> {
    let (root_x, root_y) = probe?.position()?;
    Some((root_x - monitor.x, root_y - monitor.y))
}

impl Session {
    /// The fullscreen window as host rectangle, or `None` when the window is gone.
    pub(super) fn host_surface(&self) -> Option<HostSurface> {
        let win = self.window.upgrade()?;
        let size = win.window().size();
        let scale = win.window().scale_factor().max(0.5);
        Some(HostSurface::fullscreen(
            FULLSCREEN_WINDOW,
            self.fs_xid,
            size.width.max(1).saturating_as::<i32>(),
            size.height.max(1).saturating_as::<i32>(),
            scale,
        ))
    }

    pub(super) fn on_size_changed(&mut self) {
        if !self.populated {
            return;
        }
        let Some(surface) = self.host_surface() else {
            return;
        };
        self.instance.lock().set_host(&surface);
        self.refresh();
    }

    pub(super) fn on_tick(&mut self) {
        let (probe, monitor) = (self.probe.as_ref(), &self.monitor);
        self.instance.lock().tick(|| pointer_in_window(probe, monitor));
        self.refresh();
        self.update_bar_overlay();
    }

    pub(super) fn drag_started(&mut self, index: usize) {
        self.instance.lock().drag_started(index);
    }

    /// `distance` is the pointer travel since the drag started, in logical pixels.
    pub(super) fn dragged(&mut self, distance: f32) {
        self.instance.lock().dragged(distance);
        self.refresh();
    }

    pub(super) fn drag_finished(&mut self) {
        self.instance.lock().drag_finished();
    }

    /// Ctrl + mouse wheel over an image of the Multiview: zoom around the pointer.
    pub(super) fn zoom_wheel(&mut self, index: usize, delta: f32, x: f32, y: f32) {
        self.instance.lock().zoom_wheel(index, delta, x, y);
        self.refresh();
    }

    /// The picture of a zoomed image is dragged by `dx`, `dy` logical pixels.
    pub(super) fn zoom_pan(&mut self, index: usize, dx: f32, dy: f32) {
        self.instance.lock().zoom_pan(index, dx, dy);
        self.refresh();
    }

    /// The minimap was pressed (0), dragged (1) or released (2), middle- (3) or right-clicked (4). During a drag the pointer is
    /// locked where it can be (see `pointer_lock`) and only how far it moves counts; else the rectangle follows the pointer.
    pub(super) fn zoom_minimap(&mut self, index: usize, x: f32, y: f32, phase: i32) {
        super::pointer_lock::drag_phase(self.fs_xid, phase, &mut |locked_move| {
            let mut instance = self.instance.lock();
            match locked_move {
                Some((dx, dy)) => instance.zoom_minimap_delta(index, dx, dy),
                None => instance.zoom_minimap(index, x, y, phase),
            }
        });
        self.refresh();
    }

    /// The pointer on the crop overlay of tile `index` (logical pixels relative to the tile).
    pub(super) fn crop_pointer(&mut self, index: usize, phase: i32, x: f32, y: f32, shift: bool) {
        if self.instance.lock().crop_pointer_logical(index, phase, x, y, shift) {
            self.refresh();
        }
    }

    /// The ratio button of the crop overlay of tile `index`.
    pub(super) fn crop_ratio(&mut self, index: usize, landscape: bool) {
        self.instance.lock().crop_ratio(index, landscape);
        self.refresh();
    }

    /// A key of the image editor (0 = Esc, see `MultiviewCommand::from_edit_code` for the others); returns whether it was used
    /// and what the application has to do about it. Esc is used only when it cancelled a crop tool; every other editor key is
    /// used (it acts on the selected picture, or the hint says why not).
    pub(super) fn edit_key(&mut self, code: i32) -> (bool, UiAction) {
        let outcome = match MultiviewCommand::from_edit_code(code) {
            Some(command) => (true, super::commands::run(&self.instance, command)),
            None => (code == 0 && self.instance.lock().escape_crop(), UiAction::None),
        };
        self.refresh();
        outcome
    }

    /// A key that acts on the selected medium (F2, DEL, the zoom keys).
    pub(super) fn command(&mut self, command: MultiviewCommand) -> UiAction {
        let action = super::commands::run(&self.instance, command);
        self.refresh();
        self.arm_requested_feedback();
        action
    }

    pub(super) fn zoom_minimap_slider(&mut self, index: usize, code: i32) {
        self.instance.lock().zoom_minimap_slider(index, code);
        self.refresh();
    }

    pub(super) fn reset_split(&mut self) {
        self.instance.lock().reset_split();
        self.refresh();
    }

    pub(super) fn swap(&mut self, index: usize) {
        self.instance.lock().swap(index);
        self.refresh();
    }

    /// A button of the hover bar was clicked. What the application has to do about it runs after the session borrow is
    /// released.
    pub(super) fn on_action(&mut self, tile: i32, action: i32) -> UiAction {
        let Some(click) = BarClick::from_code(action) else {
            return UiAction::None;
        };
        let outcome = {
            let mut instance = self.instance.lock();
            let Some(key) = usize::try_from(tile).ok().and_then(|index| instance.tile_key_at(index)) else {
                return UiAction::None;
            };
            instance.perform_click(key, click)
        };
        self.refresh();
        outcome
    }

    /// The application finished what a button asked for: the button flashes green, or red if it failed.
    pub(super) fn action_finished(&mut self, media_id: MediaId, action: MediaActionId, ok: bool) {
        self.instance.lock().action_finished(media_id, action, ok);
        self.refresh();
    }

    /// A button of the primary bar was clicked.
    pub(super) fn on_primary_action(&mut self, action: i32) -> UiAction {
        let Some(action) = PrimaryActionId::from_code(action) else {
            return UiAction::None;
        };
        let outcome = self.instance.lock().perform_primary_action(action);
        self.refresh();
        outcome
    }

    /// Shows, moves or hides the two bar windows (each appears while the pointer is close to the place of its bar) and
    /// hides the mouse cursor while it rests and no bar is in use.
    pub(super) fn update_bar_overlay(&mut self) {
        super::pointer_lock::service();
        if !self.populated {
            return;
        }
        let Some(win) = self.window.upgrade() else {
            return;
        };
        let host = BarHost {
            owner: BarOwner::Fullscreen,
            host_xid: self.fs_xid,
            scale: win.window().scale_factor(),
            media_click: self.bar_click.clone(),
            primary_click: self.primary_click.clone(),
        };
        // While a dialog (or the settings window) locks the main window, no bar is shown, so none can be used.
        let dialog_open = self
            .handle
            .main_window()
            .and_then(|main| main.upgrade())
            .is_some_and(|main| main.get_is_modal_locked());
        self.sync_modal_lock(&win, dialog_open && crate::ui::dialog_parent::modal_dialog_open());
        let bars_visible = bar_hosting::update(
            &host,
            BarTargets::of_instance(BarOwner::Fullscreen, &self.instance, !dialog_open),
            !dialog_open,
        );
        let video_bars_visible = self.update_video_bars(!dialog_open);
        if dialog_open {
            cursor::release(win.window(), Some(&self.instance), &mut self.cursor_hidden);
        } else {
            cursor::sync(
                win.window(),
                &self.instance,
                bars_visible || video_bars_visible,
                &mut self.cursor_hidden,
            );
        }
    }

    /// Locks the fullscreen while a dialog is open (its own or one of the main window): the overlay of the window takes every
    /// click and key.
    fn sync_modal_lock(&self, win: &MultiviewFullscreenWindow, locked: bool) {
        if win.get_is_modal_locked() == locked {
            return;
        }
        win.set_is_modal_locked(locked);
        self.instance.lock().set_modal_locked(locked);
        tracing::info!(locked, "[Multiview: ModalLock] The fullscreen follows the open dialog");
    }

    /// A click on the medium of tile `index`: a plain click selects the tile, then it counts for a double click like any other.
    pub(super) fn on_tile_click(&mut self, index: i32, is_ctrl: bool) -> UiAction {
        if !is_ctrl
            && let Ok(index) = usize::try_from(index)
            && self.instance.lock().select_tile_at(index)
        {
            self.refresh();
        }
        self.on_click(is_ctrl)
    }

    /// A click on a Slint part of the Multiview. Two quick clicks with the same modifier state are a double click, and what
    /// that means is decided by the routing table.
    pub(super) fn on_click(&mut self, is_ctrl: bool) -> UiAction {
        // A click on the Multiview itself dismisses an open bar menu (the pointer poll of the bar can miss a very short click).
        bar_overlay::close_menu(BarKind::Media);
        let (click, origin) = {
            let mut instance = self.instance.lock();
            instance.close_ab_popups();
            (instance.register_click(is_ctrl), instance.origin())
        };
        if click == ClickKind::None {
            return UiAction::None;
        }
        let event = if is_ctrl {
            RoutedEvent::CtrlClick
        } else {
            RoutedEvent::ToggleFullscreen
        };
        let action = route(HostMode::Fullscreen, origin, event, click);
        if action != UiAction::None {
            tracing::info!(is_ctrl, ?action, "[Multiview: Click] Double click");
        }
        action
    }

    /// The user deleted media through the Multiview: their tiles leave the view.
    pub(super) fn drop_deleted(&mut self, ids: &[MediaId]) -> UiAction {
        let outcome = self.instance.lock().drop_deleted(ids);
        self.refresh();
        outcome
    }

    /// Brings the tiles in line with the current selection: tiles that are not sticky and no longer selected leave, new
    /// items take the freed slots, the remaining tiles keep running. An empty selection changes nothing.
    /// Pictures with unsaved edits would leave the view because the selection changed: the user decides first. When the
    /// question takes over (`true`), following waits; it runs again once the edits are saved or discarded, and when the user
    /// cancels, the edited tiles become sticky (they stay, whatever the selection) so nothing is lost.
    fn ask_before_following(&self, leaving: &[crate::viewer::multiview::follow::TileKey]) -> bool {
        if crate::ui::image_edit::close_guard::is_asking() {
            return true;
        }
        let dirty: Vec<_> = {
            let instance = self.instance.lock();
            instance
                .unsaved_entries_of(leaving)
                .into_iter()
                .map(|e| e.tile)
                .collect()
        };
        if dirty.is_empty() {
            return false;
        }
        let Some((app, window)) = self.handle.app_and_window_for_edit() else {
            return false;
        };
        let context = self.handle.edit_context(app, &window);
        let weak = std::sync::Arc::downgrade(&self.instance);
        let keep = dirty.clone();
        // A cancelled question keeps the tiles: they become sticky right away and turn back to following once saved.
        for key in &keep {
            self.instance.lock().set_sticky(*key, true);
        }
        let proceeded = crate::ui::image_edit::close_guard::confirm(
            &context,
            vec![(self.instance.clone(), Some(dirty))],
            crate::viewer::edit::unsaved::CloseReason::SelectionChange,
            move || {
                if let Some(instance) = weak.upgrade() {
                    for key in &keep {
                        instance.lock().set_sticky(*key, false);
                    }
                }
                super::session::schedule_follow(crate::ui::multiview::handle::Pace::Immediate);
            },
        );
        !proceeded
    }

    pub(super) fn follow_now(&mut self) {
        let Some(state) = self.state.clone() else {
            return;
        };
        if !self.populated || self.instance.lock().origin() != OriginKind::Browse {
            return;
        }
        self.debounce.stop();
        let wanted = eligibility::wanted(&state.read());
        let plan = self.instance.lock().plan_follow(&wanted);
        if self.ask_before_following(&plan.remove) {
            return;
        }
        if plan.is_noop() {
            if plan.overflow > 0 {
                tracing::info!(
                    overflow = plan.overflow,
                    "[Multiview: Follow] Selected media found no free slot"
                );
            }
            return;
        }
        let seeds: Vec<Option<TileSeed>> = {
            let st = state.read();
            plan.add.iter().map(|&id| eligibility::plan_tile_for(&st, id)).collect()
        };
        tracing::debug!(
            added = plan.add.len(),
            "[Multiview: Follow] Following the Browse selection"
        );
        let mut instance = self.instance.lock();
        instance.apply_follow(&plan, seeds);
        {
            let st = state.read();
            for &id in &plan.add {
                if let Some(thumbnail) = st.thumbs.get(id) {
                    instance.set_placeholder(id, &thumbnail.clone());
                }
            }
        }
        drop(instance);
        self.refresh();
    }
}
