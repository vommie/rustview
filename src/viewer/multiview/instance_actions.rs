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

    /// True if the tile of medium `id` is sticky.
    pub fn is_sticky(&self, id: MediaId) -> bool {
        self.tiles.iter().any(|t| t.media_id == id && t.sticky)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::instance::SharedInstance;
    use crate::viewer::multiview::layout::Rect;
    use crate::viewer::multiview::route::OriginKind;
    use crate::viewer::multiview::tile::TileSeed;
    use std::path::PathBuf;

    fn instance(count: u64) -> SharedInstance {
        let seeds = (1..=count)
            .map(|id| TileSeed {
                media_id: id,
                kind: TileKind::Image,
                aspect: 1.78,
                path: PathBuf::new(),
                filename: format!("{id}.png"),
            })
            .collect();
        let shared = MultiviewInstance::detached(seeds);
        shared
            .lock()
            .set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        shared
    }

    fn ids(mv: &MultiviewInstance, index: usize) -> Vec<ActionId> {
        mv.action_views(index).iter().map(|a| a.id).collect()
    }

    #[test]
    fn sticky_toggles_and_shows_in_the_bar() {
        let shared = instance(2);
        let mut mv = shared.lock();
        assert_eq!(mv.perform_action(1, ActionId::Sticky), UiAction::None);
        assert!(mv.tiles[0].sticky && !mv.tiles[1].sticky);
        let bar = mv.action_views(0);
        let sticky = bar.iter().find(|a| a.id == ActionId::Sticky).expect("sticky");
        assert_eq!(sticky.active, Some(true));
        mv.perform_action(1, ActionId::Sticky);
        assert!(!mv.tiles[0].sticky);
    }

    #[test]
    fn remove_takes_the_tile_out_and_asks_for_the_deselection() {
        let shared = instance(3);
        let mut mv = shared.lock();
        mv.tiles[0].sticky = true;
        assert_eq!(
            mv.perform_action(2, ActionId::Remove),
            UiAction::DeselectInBrowse(2)
        );
        assert_eq!(mv.member_ids(), vec![1, 3]);
        assert!(mv.tiles[0].sticky, "the others keep their state");
    }

    #[test]
    fn the_last_tile_offers_close_instead_of_remove() {
        let shared = instance(2);
        let mut mv = shared.lock();
        mv.perform_action(1, ActionId::Remove);
        assert_eq!(mv.member_ids(), vec![2]);
        assert!(ids(&mv, 0).contains(&ActionId::Close));
        assert!(!ids(&mv, 0).contains(&ActionId::Remove));
        assert_eq!(
            mv.perform_action(2, ActionId::Remove),
            UiAction::None,
            "a stale remove click is ignored"
        );
        assert_eq!(mv.perform_action(2, ActionId::Close), UiAction::CloseFullscreen);
        assert_eq!(mv.member_ids(), vec![2]);
    }

    #[test]
    fn open_as_tab_is_left_to_the_application_and_reports_back() {
        let shared = instance(2);
        let mut mv = shared.lock();
        assert_eq!(
            mv.perform_action(2, ActionId::OpenAsTab),
            UiAction::OpenTileAsTab(2)
        );
        let before = mv.action_views(1);
        assert_eq!(before.iter().map(|a| a.feedback.seq).sum::<u32>(), 0);
        mv.action_finished(2, ActionId::OpenAsTab, true);
        let flashed = mv
            .action_views(1)
            .into_iter()
            .find(|a| a.id == ActionId::OpenAsTab)
            .expect("open as tab");
        assert_eq!(flashed.feedback.kind, FeedbackKind::Success);
        mv.action_finished(2, ActionId::OpenAsTab, false);
        let failed = mv
            .action_views(1)
            .into_iter()
            .find(|a| a.id == ActionId::OpenAsTab)
            .expect("open as tab");
        assert_eq!(failed.feedback.kind, FeedbackKind::Error);
        assert_eq!(failed.feedback.seq, flashed.feedback.seq + 1);
    }

    #[test]
    fn a_blocked_navigation_flashes_the_sticky_button_of_the_lone_tile() {
        let shared = instance(1);
        let mut mv = shared.lock();
        mv.flash_blocked_navigation();
        assert_eq!(mv.tiles[0].feedback.get(&ActionId::Sticky), None, "not sticky: no flash");
        mv.tiles[0].sticky = true;
        mv.flash_blocked_navigation();
        let flash = mv.tiles[0].feedback[&ActionId::Sticky];
        assert_eq!(flash.kind, FeedbackKind::Error);
    }

    #[test]
    fn a_tab_in_the_fullscreen_loses_media_without_touching_the_browse_view() {
        let shared = instance(3);
        let mut mv = shared.lock();
        mv.set_origin(OriginKind::Tab);
        assert_eq!(ids(&mv, 0), vec![ActionId::OpenAsTab, ActionId::Remove]);
        mv.drag_started(0);
        mv.dragged(120.0);
        let pinned = mv.tiles[0].pin;
        assert!(pinned.is_some());

        assert_eq!(mv.perform_action(2, ActionId::Remove), UiAction::TabMemberRemoved);
        assert_eq!(mv.member_ids(), vec![1, 3], "the tab keeps the other media");
        assert_eq!(mv.tiles[0].pin, pinned, "the arrangement of the others stays");

        assert_eq!(mv.perform_action(1, ActionId::Sticky), UiAction::None);
        assert!(!mv.tiles[0].sticky, "a tab has no selection to stay independent of");
    }

    #[test]
    fn the_last_medium_of_a_tab_in_the_fullscreen_closes_the_fullscreen() {
        let shared = instance(2);
        let mut mv = shared.lock();
        mv.set_origin(OriginKind::Tab);
        assert_eq!(mv.perform_action(1, ActionId::Remove), UiAction::TabMemberRemoved);
        assert_eq!(ids(&mv, 0), vec![ActionId::OpenAsTab, ActionId::Close]);
        assert_eq!(mv.perform_action(2, ActionId::Remove), UiAction::None, "a stale click");
        assert_eq!(mv.perform_action(2, ActionId::Close), UiAction::CloseFullscreen);
        assert_eq!(mv.member_ids(), vec![2], "the view is never empty");
    }

    fn tab_view_instance(count: u64) -> SharedInstance {
        let shared = instance(count);
        {
            let mut mv = shared.lock();
            mv.set_origin(OriginKind::Tab);
            mv.set_host(&HostSurface::tab_view(0, Rect::new(0, 64, 1280, 640), 1.0, Vec::new()));
        }
        shared
    }

    #[test]
    fn every_image_of_a_tab_view_has_a_bar_with_open_as_tab_and_remove() {
        let shared = tab_view_instance(3);
        let mv = shared.lock();
        for index in 0..3 {
            assert_eq!(
                ids(&mv, index),
                vec![ActionId::OpenAsTab, ActionId::Remove],
                "tile {index}"
            );
        }
    }

    #[test]
    fn a_tab_view_removes_media_and_the_last_one_closes_the_tab() {
        let shared = tab_view_instance(2);
        let mut mv = shared.lock();
        assert_eq!(mv.perform_action(1, ActionId::Remove), UiAction::TabMemberRemoved);
        assert_eq!(mv.member_ids(), vec![2]);
        assert_eq!(ids(&mv, 0), vec![ActionId::OpenAsTab, ActionId::Close]);
        assert_eq!(mv.perform_action(2, ActionId::Close), UiAction::CloseActiveTab);
        assert_eq!(mv.member_ids(), vec![2], "closing the tab is the application's job");
    }

    #[test]
    fn a_tab_view_opens_a_member_as_its_own_tab() {
        let shared = tab_view_instance(2);
        let mut mv = shared.lock();
        assert_eq!(mv.perform_action(2, ActionId::OpenAsTab), UiAction::OpenTileAsTab(2));
        assert_eq!(mv.member_ids(), vec![1, 2], "the member stays in the tab");
    }

    #[test]
    fn the_fullscreen_of_a_normal_tab_only_offers_closing() {
        let shared = instance(1);
        let mut mv = shared.lock();
        mv.set_origin(OriginKind::Tab);
        mv.set_single_tab(true);
        assert_eq!(ids(&mv, 0), vec![ActionId::Close]);
        assert_eq!(mv.perform_action(1, ActionId::OpenAsTab), UiAction::None);
        assert_eq!(mv.perform_action(1, ActionId::Close), UiAction::CloseFullscreen);
    }
}
