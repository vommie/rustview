//! Instance behavior around the action buttons of the hover bar: which buttons a tile offers, what a click does to the
//! Multiview itself (sticky, sound, remove) and what the application has to do about it (open as tab, deselect, close).

use super::follow::{FollowPlan, Slot, TileKey};
use super::instance::MultiviewInstance;
use super::media_actions::{BarClick, MediaActionId, MediaBarContext, SaveMenuItem, media_actions_for};
use super::model::TileKind;
use super::prefs;
use super::route::{HostMode, OriginKind, UiAction};
use super::view::{MediaActionView, Navigation};
use crate::ui::button_feedback::FeedbackKind;
use rustview_core::library::models::MediaId;

impl MultiviewInstance {
    pub(super) fn tile_index(&self, key: TileKey) -> Option<usize> {
        self.tiles.iter().position(|t| t.key == key)
    }

    fn action_context(&self, index: usize) -> Option<MediaBarContext> {
        let tile = self.tiles.get(index)?;
        Some(MediaBarContext {
            host: self.host.surface.mode,
            origin: self.origin,
            tile_count: self.tiles.len(),
            kind: tile.kind,
            muted: tile
                .video
                .as_ref()
                .filter(|_| tile.audio.is_some())
                .map(|video| video.last_mute),
            sticky: tile.sticky,
            broken: tile.error.is_some(),
            zoom: self.zoom_button_state(index),
            edit: self.edit_bar_state(index),
        })
    }

    /// The buttons of the hover bar of tile `index`, with their current state.
    pub fn action_views(&self, index: usize) -> Vec<MediaActionView> {
        let Some(ctx) = self.action_context(index) else {
            return Vec::new();
        };
        let tile = &self.tiles[index];
        let bar = prefs::current().media_bar;
        media_actions_for(&ctx)
            .into_iter()
            .filter(|spec| bar.allows(spec.id))
            .map(|spec| MediaActionView {
                id: spec.id,
                icon: spec.icon,
                tooltip: spec.tooltip,
                active: spec.active,
                enabled: spec.enabled,
                feedback: tile.feedback.get(&spec.id).copied().unwrap_or_default(),
                look: spec.look,
                split: spec.split,
            })
            .collect()
    }

    /// A click in the media bar window of tile `key`: a button, or an item of its "Open With" drop-down.
    pub fn perform_click(&mut self, key: TileKey, click: BarClick) -> UiAction {
        match click {
            BarClick::Action(action) => self.perform_action(key, action),
            BarClick::MenuItem(item) => match self.tile_index(key) {
                Some(index) => UiAction::OpenWithItem(self.tiles[index].media_id, item),
                None => UiAction::None,
            },
            BarClick::ZoomTo(_) | BarClick::ZoomPos(_) | BarClick::ZoomStep { .. } | BarClick::ZoomReset => {
                self.zoom_slider_click(key, click);
                UiAction::None
            }
            BarClick::SaveMenuItem(item) => match item {
                SaveMenuItem::Save => UiAction::SaveTile(key),
                SaveMenuItem::SaveAs => UiAction::SaveTileAs(key),
                SaveMenuItem::Discard => {
                    self.discard_edits(key);
                    UiAction::None
                }
            },
        }
    }

    /// A click on an action button of tile `key`. Buttons the tile does not offer or that are disabled (a stale click) are
    /// ignored. The result is what the application has to do on top of the change the Multiview made to itself.
    pub fn perform_action(&mut self, key: TileKey, action: MediaActionId) -> UiAction {
        let Some(index) = self.tile_index(key) else {
            return UiAction::None;
        };
        let id = self.tiles[index].media_id;
        // The arrow of a split button belongs to it.
        let offered = |a: &MediaActionView| match action {
            MediaActionId::SaveMenu => a.id == MediaActionId::Save && a.split,
            _ => a.id == action && a.enabled,
        };
        if !self.action_views(index).iter().any(offered) {
            tracing::debug!(
                media_id = id,
                ?action,
                "[Multiview: Actions] Ignored a click on a button the tile does not offer"
            );
            return UiAction::None;
        }
        tracing::info!(media_id = id, ?action, "[Multiview: Actions] Button clicked");
        match action {
            MediaActionId::Mute => {
                self.toggle_mute_at(index);
                UiAction::None
            }
            MediaActionId::Sticky => {
                self.toggle_sticky_at(index);
                UiAction::None
            }
            MediaActionId::Duplicate => {
                self.duplicate_tile(key);
                // A tab of one medium is a tab of several now: the application catches up (title bar, inspector, tab description).
                if self.origin == OriginKind::Tab {
                    UiAction::TabMembersChanged
                } else {
                    UiAction::None
                }
            }
            MediaActionId::Zoom => {
                self.toggle_zoom_slider(index);
                UiAction::None
            }
            MediaActionId::OpenAsTab => UiAction::OpenTileAsTab(id),
            MediaActionId::OpenWith => {
                // One bubble per bar: the drop-down replaces the zoom slider of this tile's bar (other tiles keep theirs).
                self.zoom_slider.retain(|k| *k != key);
                UiAction::ToggleOpenWith(id)
            }
            MediaActionId::Delete => UiAction::DeleteTile(id),
            MediaActionId::Remove if self.tile_is_dirty(index) => UiAction::ConfirmRemoveTile(key),
            MediaActionId::Remove => self.remove_tile(index),
            MediaActionId::Close => self.close_action(),
            MediaActionId::Crop => {
                // One bubble per bar: the crop tool is on the picture, the zoom slider stays where it is.
                self.toggle_crop(key);
                UiAction::None
            }
            MediaActionId::Undo => {
                self.undo_edit(key);
                UiAction::None
            }
            MediaActionId::Redo => {
                self.redo_edit(key);
                UiAction::None
            }
            MediaActionId::Save => UiAction::SaveTile(key),
            MediaActionId::SaveMenu => {
                self.zoom_slider.retain(|k| *k != key);
                UiAction::ToggleSaveMenu(key)
            }
        }
    }

    /// DEL deletes the single medium the Multiview shows. With several media there is no telling which one DEL means; the
    /// delete buttons of the tiles are the way then.
    pub fn delete_single(&self) -> UiAction {
        match self.solo_tile() {
            Some((id, _, _)) => UiAction::DeleteTile(id),
            None => UiAction::None,
        }
    }

    /// What the close button does: the fullscreen closes, a tab that shows the Multiview closes with its tab.
    pub(super) fn close_action(&self) -> UiAction {
        match self.host.surface.mode {
            HostMode::TabView => UiAction::CloseActiveTab,
            _ => UiAction::CloseFullscreen,
        }
    }

    /// Makes tile `key` sticky or not (a tile with unsaved edits stays while the user decides about them).
    pub fn set_sticky(&mut self, key: TileKey, sticky: bool) {
        if let Some(tile) = self.tiles.iter_mut().find(|t| t.key == key)
            && tile.sticky != sticky
        {
            tile.sticky = sticky;
            self.bump();
        }
    }

    fn toggle_sticky_at(&mut self, index: usize) {
        let Some(tile) = self.tiles.get_mut(index) else {
            return;
        };
        tile.sticky = !tile.sticky;
        tracing::info!(
            media_id = tile.media_id,
            sticky = tile.sticky,
            "[Multiview: Actions] Sticky toggled"
        );
        self.bump();
    }

    /// Removes tile `key` after the question about its unsaved edits (they were saved or discarded).
    pub fn remove_tile_by_key(&mut self, key: TileKey) -> UiAction {
        match self.tile_index(key) {
            Some(index) => self.remove_tile(index),
            None => UiAction::None,
        }
    }

    /// Takes the tile out of the view (the last tile cannot leave: the button is a close button then). A Multiview that
    /// follows the Browse selection has the medium deselected there afterwards, unless another tile still shows it (a
    /// duplicate): then the selection is still what the view shows. The Multiview of a tab just loses a tile, and the
    /// application lets everything that follows the tab's media catch up.
    fn remove_tile(&mut self, index: usize) -> UiAction {
        if self.tiles.len() <= 1 {
            return self.close_action();
        }
        let (key, id) = (self.tiles[index].key, self.tiles[index].media_id);
        self.leave_tiles(&[key]);
        match self.origin {
            OriginKind::Browse if self.tiles.iter().any(|t| t.media_id == id) => UiAction::None,
            OriginKind::Browse => UiAction::DeselectInBrowse(id),
            OriginKind::Tab => {
                tracing::info!(
                    media_id = id,
                    remaining = self.tiles.len(),
                    "[Multiview: Actions] A medium was removed from the tab"
                );
                UiAction::TabMembersChanged
            }
        }
    }

    /// Takes the tiles of `keys` out of the view; the other tiles keep running.
    fn leave_tiles(&mut self, keys: &[TileKey]) {
        let plan = FollowPlan {
            remove: keys.to_vec(),
            slots: self
                .tiles
                .iter()
                .map(|t| t.key)
                .filter(|key| !keys.contains(key))
                .map(Slot::Keep)
                .collect(),
            ..FollowPlan::default()
        };
        self.apply_follow(&plan, Vec::new());
    }

    /// The user deleted media through this Multiview (button or DEL): every tile that shows them leaves the view instead of
    /// staying behind as an error tile. The last tile cannot leave, so what shows the Multiview closes. The Browse selection
    /// was cleared by the deletion itself, a tab's other members only need the application to catch up.
    pub fn drop_deleted(&mut self, ids: &[MediaId]) -> UiAction {
        let gone: Vec<TileKey> = self
            .tiles
            .iter()
            .filter(|t| ids.contains(&t.media_id))
            .map(|t| t.key)
            .collect();
        if gone.is_empty() {
            return UiAction::None;
        }
        if gone.len() >= self.tiles.len() {
            return self.close_action();
        }
        self.leave_tiles(&gone);
        tracing::info!(
            deleted = gone.len(),
            remaining = self.tiles.len(),
            "[Multiview: Actions] Deleted media left the view"
        );
        match self.origin {
            OriginKind::Browse => UiAction::None,
            OriginKind::Tab => UiAction::TabMembersChanged,
        }
    }

    /// Plays the feedback of a button of every tile that shows `id` (success or error flash). Only the bar of the hovered
    /// tile is ever drawn, so the others flash unseen.
    pub fn trigger_feedback(&mut self, id: MediaId, action: MediaActionId, kind: FeedbackKind) {
        let mut found = false;
        for tile in self.tiles.iter_mut().filter(|t| t.media_id == id) {
            tile.feedback.entry(action).or_default().trigger(kind);
            found = true;
        }
        if found {
            self.bump();
        }
    }

    /// The application finished what a button asked for: the button flashes green, or red if it failed.
    pub fn action_finished(&mut self, id: MediaId, action: MediaActionId, ok: bool) {
        let kind = if ok { FeedbackKind::Success } else { FeedbackKind::Error };
        self.trigger_feedback(id, action, kind);
    }

    /// The single medium of the Multiview, if there is exactly one.
    pub fn solo_tile(&self) -> Option<(MediaId, TileKind, bool)> {
        match self.tiles.as_slice() {
            [only] => Some((only.media_id, only.kind, only.sticky)),
            _ => None,
        }
    }

    /// The file name of the single medium, if there is exactly one (a tab that holds such a Multiview is named after it).
    pub fn solo_filename(&self) -> Option<String> {
        match self.tiles.as_slice() {
            [only] => Some(only.filename.clone()),
            _ => None,
        }
    }

    /// Previous/next applies to a single image of a Multiview that follows the Browse selection. A video keeps all keys and
    /// the wheel for mpv (the Browse view is how one moves on), a snapshot that belongs to a tab is isolated.
    pub fn navigation(&self) -> Navigation {
        if self.origin != OriginKind::Browse {
            return Navigation::Off;
        }
        match self.solo_tile() {
            Some((_, TileKind::Image, false)) => Navigation::Allowed,
            Some((_, TileKind::Image, true)) => Navigation::Blocked,
            _ => Navigation::Off,
        }
    }

    /// A previous/next request was refused because the single medium is sticky: its sticky button flashes red.
    pub fn flash_blocked_navigation(&mut self) {
        if let Some((id, _, true)) = self.solo_tile() {
            self.trigger_feedback(id, MediaActionId::Sticky, FeedbackKind::Error);
        }
    }
}
