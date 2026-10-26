//! Turns player events into UI actions.
//!
//! mpv observer threads and decode workers post `PlayerEvent`s together with a weak reference to their instance; the sink hops to
//! the UI thread and hands them to the instance. A dead weak reference means the Multiview is gone, so the event is dropped.
//! `execute` carries out the `UiAction`s the hosts decide (`route`, a pure table, for clicks on the canvas).

use super::handle::MultiviewHandle;
use super::session::{self, Session};
use super::session_convert;
use super::tab_host;
use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
use crate::viewer::multiview::instance::WeakInstance;
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::route::{HostMode, UiAction};

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
    let mode = {
        let mut guard = instance.lock();
        match event {
            PlayerEvent::ImageDecoded { tile, result, source } => guard.on_image_loaded(tile, result, source),
            PlayerEvent::EditPreview { tile, revision, result } => guard.on_edit_preview(tile, revision, result),
            PlayerEvent::ZoomSource { tile, result } => guard.on_zoom_source(tile, result),
            PlayerEvent::ZoomRendered { tile, done } => guard.on_zoom_rendered(tile, done),
            PlayerEvent::MuteChanged { tile, muted } => guard.on_player_mute_changed(tile, muted),
            PlayerEvent::AbRepeat { tile, change } => guard.on_ab_repeat_changed(tile, change),
            PlayerEvent::PauseChanged { tile, paused } => guard.on_player_pause_changed(tile, paused),
        }
        guard.host_mode()
    };
    drop(instance);
    refresh_host(handle, mode);
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
