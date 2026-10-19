//! Session behavior around input: window geometry, the tick, clicks, gutter drag, swap, rename and the live selection.
//! Everything here translates a UI event into a call on the instance and syncs the result into the window.

use super::bar_hosting::{self, BarHost, BarTargets};
use super::bar_overlay::{self, BarKind, BarOwner};
use super::cursor;
use super::eligibility;
use super::session::Session;
use crate::library::models::MediaId;
use crate::system::MonitorInfo;
use crate::viewer::PointerProbe;
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::media_actions::{BarClick, MediaActionId};
use crate::viewer::multiview::primary_actions::PrimaryActionId;
use crate::viewer::multiview::click::ClickKind;
use crate::viewer::multiview::route::{HostMode, OriginKind, RoutedEvent, UiAction, route};
use crate::viewer::multiview::tile::TileSeed;
use slint::ComponentHandle;
use std::collections::HashMap;

/// The pointer in the coordinates of the fullscreen window, which covers its monitor.
fn pointer_in_window(probe: &Option<PointerProbe>, monitor: &MonitorInfo) -> Option<(i32, i32)> {
    let (root_x, root_y) = probe.as_ref()?.position()?;
    Some((root_x - monitor.x, root_y - monitor.y))
}

impl Session {
    /// The fullscreen window as host rectangle, or `None` when the window is gone.
    pub(super) fn host_surface(&self) -> Option<HostSurface> {
        let win = self.window.upgrade()?;
        let size = win.window().size();
        let scale = win.window().scale_factor().max(0.5);
        Some(HostSurface::fullscreen(
            self.fs_xid,
            size.width.max(1) as i32,
            size.height.max(1) as i32,
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
        let (probe, monitor) = (&self.probe, &self.monitor);
        self.instance
            .lock()
            .tick(|| pointer_in_window(probe, monitor));
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
            let Some(media_id) = usize::try_from(tile)
                .ok()
                .and_then(|index| instance.member_ids().get(index).copied())
            else {
                return UiAction::None;
            };
            instance.perform_click(media_id, click)
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
        let bars_visible = bar_hosting::update(&host, BarTargets::of_instance(&self.instance, !dialog_open), !dialog_open);
        if dialog_open {
            cursor::release(win.window(), Some(&self.instance), &mut self.cursor_hidden);
        } else {
            cursor::sync(win.window(), &self.instance, bars_visible, &mut self.cursor_hidden);
        }
    }

    /// A click on a Slint part of the Multiview. Two quick clicks with the same modifier state are a double click, and what
    /// that means is decided by the routing table.
    pub(super) fn on_click(&mut self, is_ctrl: bool) -> UiAction {
        // A click on the Multiview itself dismisses an open bar menu (the pointer poll of the bar can miss a very short click).
        bar_overlay::close_menu(BarKind::Media);
        let (click, origin) = {
            let mut instance = self.instance.lock();
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

    /// F2 acts on the tile under the pointer, else the last hovered one, else the first.
    pub(super) fn rename_hovered(&mut self) -> UiAction {
        let pointer = pointer_in_window(&self.probe, &self.monitor);
        match self.instance.lock().rename_hovered(pointer) {
            Some(_) => UiAction::RequestRename,
            None => UiAction::None,
        }
    }

    /// DEL in the fullscreen window.
    pub(super) fn delete_single(&mut self) -> UiAction {
        self.instance.lock().delete_single()
    }

    /// The user deleted media through the Multiview: their tiles leave the view.
    pub(super) fn drop_deleted(&mut self, ids: &[MediaId]) -> UiAction {
        let outcome = self.instance.lock().drop_deleted(ids);
        self.refresh();
        outcome
    }

    /// Brings the tiles in line with the current selection: tiles that are not sticky and no longer selected leave, new
    /// items take the freed slots, the remaining tiles keep running. An empty selection changes nothing.
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
        if plan.is_noop() {
            if plan.overflow > 0 {
                tracing::info!(
                    overflow = plan.overflow,
                    "[Multiview: Follow] Selected media found no free slot"
                );
            }
            return;
        }
        let seeds: HashMap<MediaId, TileSeed> = {
            let st = state.read();
            plan.add
                .iter()
                .filter_map(|&id| eligibility::plan_tile_for(&st, id))
                .map(|seed| (seed.media_id, seed))
                .collect()
        };
        let mut instance = self.instance.lock();
        instance.apply_follow(&plan, seeds);
        {
            let st = state.read();
            for &id in &plan.add {
                if let Some(thumbnail) = st.thumbs.get(id) {
                    instance.set_placeholder(id, thumbnail.clone());
                }
            }
        }
        drop(instance);
        self.refresh();
    }
}
