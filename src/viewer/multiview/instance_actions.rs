//! Instance behavior around the action buttons of the hover bar: which buttons a tile offers, what a click does to the
//! Multiview itself (sticky, sound, remove) and what the application has to do about it (open as tab, deselect, close).

use super::actions::{ActionContext, ActionId, actions_for};
use super::follow::FollowPlan;
use super::instance::MultiviewInstance;
use super::model::TileKind;
use super::route::{HostMode, OriginKind, UiAction};
use super::view::{ActionView, Navigation};
use crate::button_feedback::FeedbackKind;
use crate::library::models::MediaId;
use std::collections::HashMap;

impl MultiviewInstance {
    fn tile_index(&self, id: MediaId) -> Option<usize> {
        self.tiles.iter().position(|t| t.media_id == id)
    }

    fn action_context(&self, index: usize) -> Option<ActionContext> {
        let tile = self.tiles.get(index)?;
        Some(ActionContext {
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
            single_tab: self.single_tab,
        })
    }

    /// The buttons of the hover bar of tile `index`, with their current state.
    pub fn action_views(&self, index: usize) -> Vec<ActionView> {
        let Some(ctx) = self.action_context(index) else {
            return Vec::new();
        };
        let tile = &self.tiles[index];
        actions_for(&ctx)
            .into_iter()
            .map(|spec| ActionView {
                id: spec.id,
                icon: spec.icon,
                tooltip: spec.tooltip,
                active: spec.active,
                feedback: tile.feedback.get(&spec.id).copied().unwrap_or_default(),
            })
            .collect()
    }

    /// A click on an action button of tile `id`. Buttons the tile does not offer (a stale click) are ignored. The result
    /// is what the application has to do on top of the change the Multiview made to itself.
    pub fn perform_action(&mut self, id: MediaId, action: ActionId) -> UiAction {
        let Some(index) = self.tile_index(id) else {
            return UiAction::None;
        };
        if !self.action_views(index).iter().any(|a| a.id == action) {
            tracing::debug!(
                media_id = id,
                ?action,
                "[Multiview: Actions] Ignored a click on a button the tile does not offer"
            );
            return UiAction::None;
        }
        tracing::info!(media_id = id, ?action, "[Multiview: Actions] Button clicked");
        match action {
            ActionId::Mute => {
                self.toggle_mute_at(index);
                UiAction::None
            }
            ActionId::Sticky => {
                self.toggle_sticky_at(index);
                UiAction::None
            }
            ActionId::OpenAsTab => UiAction::OpenTileAsTab(id),
            ActionId::Remove => self.remove_tile(index),
            ActionId::Close => self.close_action(),
        }
    }

    /// What the close button does: the fullscreen closes, a tab that shows the Multiview closes with its tab.
    fn close_action(&self) -> UiAction {
        match self.host.surface.mode {
            HostMode::TabView => UiAction::CloseActiveTab,
            _ => UiAction::CloseFullscreen,
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

    /// Takes the tile out of the view (the last tile cannot leave: the button is a close button then). A Multiview that
    /// follows the Browse selection has the medium deselected there afterwards; the Multiview of a tab just loses a member,
    /// and the application lets everything that follows the tab's media catch up.
    fn remove_tile(&mut self, index: usize) -> UiAction {
        if self.tiles.len() <= 1 {
            return self.close_action();
        }
        let id = self.tiles[index].media_id;
        let plan = FollowPlan {
            remove: vec![id],
            slots: self
                .tiles
                .iter()
                .map(|t| t.media_id)
                .filter(|&m| m != id)
                .collect(),
            ..FollowPlan::default()
        };
        self.apply_follow(&plan, HashMap::new());
        match self.origin {
            OriginKind::Browse => UiAction::DeselectInBrowse(id),
            OriginKind::Tab => {
                tracing::info!(
                    media_id = id,
                    remaining = self.tiles.len(),
                    "[Multiview: Actions] A medium was removed from the tab"
                );
                UiAction::TabMemberRemoved
            }
        }
    }

    /// Plays the feedback of a button (success or error flash).
    pub fn trigger_feedback(&mut self, id: MediaId, action: ActionId, kind: FeedbackKind) {
        let Some(index) = self.tile_index(id) else {
            return;
        };
        self.tiles[index]
            .feedback
            .entry(action)
            .or_default()
            .trigger(kind);
        self.bump();
    }

    /// The application finished what a button asked for: the button flashes green, or red if it failed.
    pub fn action_finished(&mut self, id: MediaId, action: ActionId, ok: bool) {
        let kind = if ok {
            FeedbackKind::Success
        } else {
            FeedbackKind::Error
        };
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
            self.trigger_feedback(id, ActionId::Sticky, FeedbackKind::Error);
        }
    }
}
