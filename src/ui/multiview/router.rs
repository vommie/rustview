//! Turns player events into UI actions.
//!
//! mpv observer threads and decode workers post `PlayerEvent`s together with a weak reference to their instance. The sink
//! hops to the UI thread, and `route` (a pure table) decides from the current host of the instance what the event means.
//! A dead weak reference means the Multiview is gone, so the event is dropped.

use super::handle::MultiviewHandle;
use super::session::{self, Session};
use super::session_convert;
use super::tab_host;
use crate::viewer::multiview::click::ClickKind;
use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
use crate::viewer::multiview::instance::WeakInstance;
use crate::viewer::multiview::route::{HostMode, RoutedEvent, UiAction, route};

pub(super) struct UiEventSink {
    handle: MultiviewHandle,
}

impl UiEventSink {
    pub(super) fn new(handle: MultiviewHandle) -> Self {
        Self { handle }
    }
}

impl MultiviewEventSink for UiEventSink {
    fn post(&self, instance: WeakInstance, event: PlayerEvent) {
        let handle = self.handle.clone();
        let _ = slint::invoke_from_event_loop(move || route_event(&handle, instance, event));
    }
}

fn route_event(handle: &MultiviewHandle, weak: WeakInstance, event: PlayerEvent) {
    let Some(instance) = weak.upgrade() else {
        return;
    };
    let (routed, click) = match event {
        PlayerEvent::ImageDecoded { media_id, result } => {
            let mode = {
                let mut guard = instance.lock();
                guard.on_image_loaded(media_id, result);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(mode);
            return;
        }
        PlayerEvent::ToggleFullscreen => (RoutedEvent::ToggleFullscreen, ClickKind::None),
        PlayerEvent::CloseTab => (RoutedEvent::CloseTab, ClickKind::None),
        PlayerEvent::RenameRequested(media_id) => {
            instance.lock().note_rename_target(media_id);
            (RoutedEvent::Rename, ClickKind::None)
        }
        PlayerEvent::CtrlClick => {
            let click = instance.lock().register_click(true);
            (RoutedEvent::CtrlClick, click)
        }
        PlayerEvent::GridLeftClick => (RoutedEvent::GridLeft, ClickKind::None),
        PlayerEvent::GridMiddleClick => (RoutedEvent::GridMiddle, ClickKind::None),
        PlayerEvent::GridWheel { up } => (RoutedEvent::GridWheel { up }, ClickKind::None),
    };
    let (mode, origin, instance_id) = {
        let guard = instance.lock();
        (guard.host_mode(), guard.origin(), guard.id())
    };
    drop(instance);
    let action = route(mode, origin, routed, click);
    if click == ClickKind::CtrlDouble && action != UiAction::None {
        tracing::info!(?action, "[Multiview: Click] Ctrl+double click on a video");
    }
    execute(action, handle, Some(instance_id));
}

/// Shows the changed state of an instance in the host that presents it.
fn refresh_host(mode: HostMode) {
    match mode {
        HostMode::Fullscreen => session::with_session(Session::refresh),
        HostMode::TabView => tab_host::refresh(),
        HostMode::Hidden | HostMode::TabGrid => {}
    }
}

/// Carries out an action. Called without any session borrow, so actions may close the session. `instance_id` names the
/// Multiview the action comes from (needed for the actions that address its tab).
pub(super) fn execute(action: UiAction, handle: &MultiviewHandle, instance_id: Option<u64>) {
    match (action, instance_id) {
        (UiAction::None, _) => {}
        (UiAction::CloseFullscreen, _) => session::close(true),
        (UiAction::ConvertToTab, _) => session_convert::convert_to_tab(),
        (UiAction::RequestRename, _) => handle.request_rename(),
        (UiAction::CloseActiveTab, _) => handle.close_active_tab(),
        (UiAction::OpenTabFullscreen, Some(id)) => handle.open_tab_fullscreen(id),
        (UiAction::ActivateTab, Some(id)) => handle.activate_tab(id),
        (UiAction::ScrollGrid { up }, _) => handle.scroll_grid(up),
        (other, _) => {
            tracing::debug!(?other, "[Multiview: Router] The action needs a Multiview");
        }
    }
}
