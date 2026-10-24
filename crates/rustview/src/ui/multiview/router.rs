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
use crate::viewer::multiview::command::MultiviewCommand;
use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
use crate::viewer::multiview::instance::WeakInstance;
use crate::viewer::multiview::media_actions::MediaActionId;
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
        let _ = slint::invoke_from_event_loop(move || route_event(&handle, &instance, event));
    }
}

fn route_event(handle: &MultiviewHandle, weak: &WeakInstance, event: PlayerEvent) {
    let Some(instance) = weak.upgrade() else {
        return;
    };
    let (routed, click) = match event {
        PlayerEvent::ImageDecoded { tile, result, source } => {
            let mode = {
                let mut guard = instance.lock();
                guard.on_image_loaded(tile, result, source);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(handle, mode);
            return;
        }
        PlayerEvent::EditPreview { tile, revision, result } => {
            let mode = {
                let mut guard = instance.lock();
                guard.on_edit_preview(tile, revision, result);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(handle, mode);
            return;
        }
        PlayerEvent::ZoomSource { tile, result } => {
            let mode = {
                let mut guard = instance.lock();
                guard.on_zoom_source(tile, result);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(handle, mode);
            return;
        }
        PlayerEvent::ZoomRendered { tile, done } => {
            let mode = {
                let mut guard = instance.lock();
                guard.on_zoom_rendered(tile, done);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(handle, mode);
            return;
        }
        PlayerEvent::MuteChanged { tile, muted } => {
            let mode = {
                let mut guard = instance.lock();
                guard.on_player_mute_changed(tile, muted);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(handle, mode);
            return;
        }
        PlayerEvent::PauseChanged { tile, paused } => {
            let mode = {
                let mut guard = instance.lock();
                guard.on_player_pause_changed(tile, paused);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(handle, mode);
            return;
        }
        // Esc in a video's window runs the same chain as Esc anywhere else: a crop tool (the selected tile's first), then the
        // fullscreen closes or, in the tab view, a selection of tabs ends.
        PlayerEvent::EscapePressed => {
            let (cancelled, mode) = {
                let mut guard = instance.lock();
                (guard.escape_crop(), guard.host_mode())
            };
            drop(instance);
            if cancelled {
                refresh_host(handle, mode);
            } else {
                match mode {
                    HostMode::Fullscreen => {
                        session::close_by_user();
                    }
                    HostMode::TabView => handle.end_tab_selection(),
                    HostMode::TabGrid | HostMode::Hidden => {}
                }
            }
            return;
        }
        PlayerEvent::ToggleFullscreen => (RoutedEvent::ToggleFullscreen, ClickKind::None),
        PlayerEvent::CloseTab => (RoutedEvent::CloseTab, ClickKind::None),
        // F2 and DEL in a video's window act on the selected medium, whichever video reported them.
        PlayerEvent::RenameRequested => {
            let mode = instance.lock().host_mode();
            match mode {
                HostMode::Fullscreen => {
                    let action = super::commands::run(&instance, MultiviewCommand::Rename);
                    drop(instance);
                    execute(action, handle, None);
                }
                HostMode::TabView => handle.request_tab_rename(),
                HostMode::TabGrid | HostMode::Hidden => {}
            }
            return;
        }
        PlayerEvent::DeleteRequested => {
            let action = super::commands::run(&instance, MultiviewCommand::Delete);
            let mode = instance.lock().host_mode();
            drop(instance);
            execute(action, handle, None);
            refresh_host(handle, mode);
            return;
        }
        PlayerEvent::RatingRequested(rating) => {
            handle.rate_visible(&instance, rating);
            return;
        }
        // A plain click on a video selects its tile (Slint never sees clicks on the native window).
        PlayerEvent::Clicked(tile) => {
            super::bar_overlay::close_menu(super::bar_overlay::BarKind::Media);
            let mode = {
                let mut guard = instance.lock();
                guard.select_tile(tile);
                guard.host_mode()
            };
            drop(instance);
            refresh_host(handle, mode);
            return;
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
pub(super) fn refresh_host(handle: &MultiviewHandle, mode: HostMode) {
    match mode {
        HostMode::Fullscreen => session::with_session(Session::refresh),
        HostMode::TabView => tab_host::refresh(),
        // A tab that was never shown starts in its grid item and its picture arrives later: the grid draws it again.
        HostMode::TabGrid => handle.refresh_tab_grid(),
        HostMode::Hidden => {}
    }
}

/// Carries out an action. Called without any session borrow, so actions may close the session. `instance_id` names the
/// Multiview the action comes from (needed for the actions that address its tab).
pub(super) fn execute(action: UiAction, handle: &MultiviewHandle, instance_id: Option<u64>) {
    match (action, instance_id) {
        (UiAction::None, _) => {}
        (UiAction::CloseFullscreen, _) => {
            session::close_by_user();
        }
        (UiAction::ConvertToTab, _) => {
            // Moving the view into a tab ends the fullscreen, which a pinned fullscreen refuses.
            if !session::close_refused() {
                session_convert::convert_to_tab();
            }
        }
        (UiAction::SetIdleCursorHide(on), _) => handle.set_idle_cursor_hide(on),
        (UiAction::RequestRename, _) => handle.request_rename(),
        (UiAction::DeleteTile(media_id), _) => handle.request_delete(media_id),
        (UiAction::ToggleOpenWith(media_id), _) => handle.toggle_open_with(media_id),
        (UiAction::OpenWithItem(media_id, item), _) => handle.open_with_item(media_id, item),
        (UiAction::CloseActiveTab, _) => handle.close_active_tab(),
        (UiAction::OpenTabFullscreen, Some(id)) => handle.open_tab_fullscreen(id),
        (UiAction::ActivateTab, Some(id)) => handle.activate_tab(id),
        (UiAction::ToggleTabSelection, Some(id)) => handle.toggle_tab_selection(id),
        (UiAction::ScrollGrid { up }, _) => handle.scroll_grid(up),
        (UiAction::Navigate { forward }, _) => handle.navigate(forward),
        (UiAction::DeselectInBrowse(media_id), _) => handle.deselect_in_browse(media_id),
        (UiAction::TabMembersChanged, _) => handle.tab_members_changed(),
        (UiAction::SaveTile(key), _) => handle.save_tile(key, crate::ui::image_edit::SaveIntent::Save),
        (UiAction::SaveTileAs(key), _) => handle.save_tile(key, crate::ui::image_edit::SaveIntent::SaveAs),
        (UiAction::ToggleSaveMenu(_), _) => MultiviewHandle::toggle_save_menu(),
        (UiAction::ConfirmRemoveTile(key), _) => handle.confirm_remove_tile(key),
        (UiAction::OpenTileAsTab(media_id), _) => {
            let opened = handle.open_tile_as_tab(media_id);
            session::with_session(|s| s.action_finished(media_id, MediaActionId::OpenAsTab, opened));
            tab_host::action_finished(media_id, MediaActionId::OpenAsTab, opened);
        }
        (other, _) => {
            tracing::debug!(?other, "[Multiview: Router] The action needs a Multiview");
        }
    }
}
