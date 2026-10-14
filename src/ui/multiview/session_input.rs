//! Session behavior around input: window geometry, the tick, clicks, gutter drag, swap, rename and the live selection.
//! Everything here translates a UI event into a call on the instance and syncs the result into the window.

use super::eligibility;
use super::session::Session;
use super::views;
use crate::library::models::MediaId;
use crate::system::MonitorInfo;
use crate::viewer::PointerProbe;
use crate::ui::bridge_ctx_menu::make_rounded_cutout_rectangles;
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::actions::ActionId;
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
        let mut surface = HostSurface::fullscreen(
            self.fs_xid,
            size.width.max(1) as i32,
            size.height.max(1) as i32,
            scale,
        );
        surface.overlay_cutouts = self.tooltip_cutouts.clone();
        Some(surface)
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
        let Some(action) = ActionId::from_code(action) else {
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
            instance.perform_action(media_id, action)
        };
        self.refresh();
        outcome
    }

    /// The hover bar was drawn, moved or hidden: the video window below it is cut open (or closed again).
    pub(super) fn on_bar_overlay(&mut self, visible: bool, x: f32, y: f32, w: f32, h: f32) {
        let dpi = self.window.upgrade().map_or(1.0, |win| win.window().scale_factor());
        let rect = views::bar_overlay(visible, x, y, w, h, dpi);
        self.instance.lock().set_bar_overlay(rect);
    }

    /// The tooltip bubble appeared, moved or went away: video windows leave its area free.
    pub(super) fn on_tooltip(&mut self, visible: bool, x: f32, y: f32, w: f32, h: f32) {
        let scale = self.window.upgrade().map_or(1.0, |win| win.window().scale_factor());
        self.tooltip_cutouts = if visible && w > 2.0 && h > 2.0 {
            make_rounded_cutout_rectangles(x, y, w, h, scale)
        } else {
            Vec::new()
        };
        if let Some(surface) = self.host_surface() {
            self.instance.lock().set_overlay_cutouts(&surface.overlay_cutouts);
        }
    }

    /// A click on a Slint part of the Multiview. Two quick clicks with the same modifier state are a double click, and what
    /// that means is decided by the routing table.
    pub(super) fn on_click(&mut self, is_ctrl: bool) -> UiAction {
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

    /// Brings the tiles in line with the current selection: tiles that are not sticky and no longer selected leave, new
    /// items take the freed slots, the remaining tiles keep running. An empty selection changes nothing.
    pub(super) fn follow_now(&mut self) {
        let Some(state) = self.state.clone() else {
            return;
        };
        if !self.populated || self.instance.lock().origin() != OriginKind::Browse {
            return;
        }
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
        self.instance.lock().apply_follow(&plan, seeds);
        self.refresh();
    }
}
