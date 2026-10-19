//! The two action bars of a normal image or video tab (pure).
//!
//! A tab that was opened directly (double click in the grid, Enter, "Open in new tab") is not a Multiview: it shows its one
//! medium with the regular viewer. It gets the same bars as the Multiview of one tile in a tab, built from the same provider
//! lists (`media_actions`, `primary_actions`) and the same preferences, so a medium offers the same buttons however its tab
//! came to be. This module is the Slint- and X11-free part: which buttons exist, where the bars sit and what a click means.
//! The presenter (`ui::multiview::tab_plain`) owns the pointer polling and applies the outcomes.

use super::bar_placement::{BarCorner, near_bar_corner};
use super::layout::Rect;
use super::media_actions::{BarClick, MediaActionId, MediaBarContext, media_actions_for};
use super::model::TileKind;
use super::prefs;
use super::primary_actions::{PrimaryActionId, PrimaryBarContext, primary_actions_for};
use super::route::{HostMode, OriginKind, UiAction};
use super::view::{MediaActionView, MediaBarTarget, PrimaryActionView, PrimaryBarTarget};
use crate::library::models::MediaId;
use crate::ui::button_feedback::ButtonFeedback;
use std::time::{Duration, Instant};

/// What the bars of a plain tab need to know, all of it in-memory state of the tab at this moment.
#[derive(Debug, Clone, Copy)]
pub struct PlainTabState {
    pub media_id: MediaId,
    pub kind: TileKind,
    /// `Some(muted)` for a video (it has sound control), else `None`.
    pub muted: Option<bool>,
    /// A video that the user has not paused.
    pub playing: bool,
    /// The viewer area of the tab in the main window (physical pixels).
    pub area: Rect,
    /// Pointer in the coordinates of the main window.
    pub pointer: Option<(i32, i32)>,
    /// Physical pixels per logical pixel.
    pub dpi: f32,
}

/// What the application has to do after a click on a bar of a plain tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlainOutcome {
    None,
    /// An application-level action, handled like the same action of a Multiview.
    Ui(UiAction),
    SetMuted(bool),
    SetPaused(bool),
}

impl PlainTabState {
    fn is_video(&self) -> bool {
        self.kind == TileKind::Video
    }

    fn media_context(&self) -> MediaBarContext {
        MediaBarContext {
            host: HostMode::TabView,
            origin: OriginKind::Tab,
            tile_count: 1,
            kind: self.kind,
            muted: self.muted,
            sticky: false,
            // The tab already exists: there is nothing to open as a tab.
            single_tab: true,
        }
    }

    fn primary_context(&self) -> PrimaryBarContext {
        let bar = prefs::current().primary;
        let video = self.is_video();
        PrimaryBarContext {
            host: HostMode::TabView,
            origin: OriginKind::Tab,
            tile_count: 1,
            video_count: usize::from(video),
            any_audible: video && self.muted == Some(false),
            any_playing: video && self.playing,
            layout_custom: false,
            pinned: false,
            hide_idle_cursor: bar.hide_idle_cursor,
            idle_cursor_ms: bar.idle_cursor_ms,
        }
    }

    /// The buttons of the media bar the user allows.
    pub fn media_actions(&self) -> Vec<MediaActionView> {
        let bar = prefs::current().media_bar;
        media_actions_for(&self.media_context())
            .into_iter()
            .filter(|spec| bar.allows(spec.id))
            .map(|spec| MediaActionView {
                id: spec.id,
                icon: spec.icon,
                tooltip: spec.tooltip,
                active: spec.active,
                feedback: ButtonFeedback::default(),
            })
            .collect()
    }

    /// The buttons of the primary bar the user allows.
    pub fn primary_actions(&self) -> Vec<PrimaryActionView> {
        let bar = prefs::current().primary;
        primary_actions_for(&self.primary_context())
            .into_iter()
            .filter(|spec| bar.allows(spec.id))
            .map(|spec| PrimaryActionView {
                attention: false,
                id: spec.id,
                icon: spec.icon,
                tooltip: spec.tooltip,
                enabled: spec.enabled,
                active: spec.active,
            })
            .collect()
    }

    /// The media bar the host may show: the pointer is near the top right corner of the viewer area and a button is on offer.
    pub fn media_bar_target(&self) -> Option<MediaBarTarget> {
        let bar = prefs::current().media_bar;
        let pointer = self.pointer?;
        if !bar.enabled || !near_bar_corner(BarCorner::TopRight, self.area, pointer, self.dpi, bar.reveal_distance_px) {
            return None;
        }
        let actions = self.media_actions();
        (!actions.is_empty()).then_some(MediaBarTarget {
            tile: 0,
            rect: self.area,
            actions,
            pointer,
            reveal_distance_px: bar.reveal_distance_px,
        })
    }

    /// The primary bar the host may show: the pointer is near the top left corner of the viewer area and a button is on offer.
    pub fn primary_bar_target(&self) -> Option<PrimaryBarTarget> {
        let bar = prefs::current().primary;
        let pointer = self.pointer?;
        if !bar.enabled || !near_bar_corner(BarCorner::TopLeft, self.area, pointer, self.dpi, bar.reveal_distance_px) {
            return None;
        }
        let actions = self.primary_actions();
        (!actions.is_empty()).then_some(PrimaryBarTarget {
            area: self.area,
            actions,
            pointer: Some(pointer),
            reveal_distance_px: bar.reveal_distance_px,
            forced: false,
        })
    }

    /// A click on the media bar: a button, or an item of its "Open With" drop-down. Buttons the tab does not offer (a stale
    /// click) do nothing.
    pub fn media_click(&self, click: BarClick) -> PlainOutcome {
        let id = self.media_id;
        match click {
            BarClick::MenuItem(item) => PlainOutcome::Ui(UiAction::OpenWithItem(id, item)),
            BarClick::Action(action) => {
                if !self.media_actions().iter().any(|a| a.id == action) {
                    return PlainOutcome::None;
                }
                match action {
                    MediaActionId::Mute => PlainOutcome::SetMuted(self.muted != Some(true)),
                    MediaActionId::OpenWith => PlainOutcome::Ui(UiAction::ToggleOpenWith(id)),
                    MediaActionId::Delete => PlainOutcome::Ui(UiAction::DeleteTile(id)),
                    MediaActionId::Close => PlainOutcome::Ui(UiAction::CloseActiveTab),
                    MediaActionId::Sticky | MediaActionId::OpenAsTab | MediaActionId::Remove => PlainOutcome::None,
                }
            }
        }
    }

    /// A click on a button of the primary bar.
    pub fn primary_click(&self, id: PrimaryActionId) -> PlainOutcome {
        let Some(spec) = self.primary_actions().into_iter().find(|a| a.id == id) else {
            return PlainOutcome::None;
        };
        if !spec.enabled {
            return PlainOutcome::None;
        }
        match id {
            PrimaryActionId::Close => PlainOutcome::Ui(UiAction::CloseActiveTab),
            PrimaryActionId::PlayPauseAll => PlainOutcome::SetPaused(self.playing),
            PrimaryActionId::HideIdleCursor => {
                PlainOutcome::Ui(UiAction::SetIdleCursorHide(!prefs::current().primary.hide_idle_cursor))
            }
            PrimaryActionId::SendToTab
            | PrimaryActionId::MuteAll
            | PrimaryActionId::ResetLayout
            | PrimaryActionId::Pin => PlainOutcome::None,
        }
    }

    /// The mouse cursor should be hidden now: the user wants idle cursors hidden, the pointer rests inside the viewer area.
    pub fn cursor_should_hide(&self, last_move: Option<Instant>) -> bool {
        let prefs = prefs::current().primary;
        prefs.hide_idle_cursor
            && self.pointer.is_some_and(|(x, y)| self.area.contains(x, y))
            && last_move.is_some_and(|at| at.elapsed() >= Duration::from_millis(u64::from(prefs.idle_cursor_ms)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(kind: TileKind, muted: Option<bool>, playing: bool) -> PlainTabState {
        PlainTabState {
            media_id: 7,
            kind,
            muted,
            playing,
            area: Rect::new(0, 64, 1600, 900),
            pointer: Some((1590, 70)),
            dpi: 1.0,
        }
    }

    fn media_ids(state: &PlainTabState) -> Vec<MediaActionId> {
        state.media_actions().into_iter().map(|a| a.id).collect()
    }

    fn primary_ids(state: &PlainTabState) -> Vec<PrimaryActionId> {
        state.primary_actions().into_iter().map(|a| a.id).collect()
    }

    #[test]
    fn video_media_bar_matches_the_tab_view_providers() {
        let video = state(TileKind::Video, Some(false), true);
        assert_eq!(
            media_ids(&video),
            [MediaActionId::Mute, MediaActionId::OpenWith, MediaActionId::Delete, MediaActionId::Close]
        );
    }

    #[test]
    fn image_media_bar_has_no_mute() {
        let image = state(TileKind::Image, None, false);
        assert_eq!(media_ids(&image), [MediaActionId::OpenWith, MediaActionId::Delete, MediaActionId::Close]);
    }

    #[test]
    fn primary_bar_of_a_video_has_close_play_pause_and_cursor() {
        let video = state(TileKind::Video, Some(true), false);
        assert_eq!(
            primary_ids(&video),
            [PrimaryActionId::Close, PrimaryActionId::PlayPauseAll, PrimaryActionId::HideIdleCursor]
        );
        let image = state(TileKind::Image, None, false);
        assert_eq!(primary_ids(&image), [PrimaryActionId::Close, PrimaryActionId::HideIdleCursor]);
    }

    #[test]
    fn bars_appear_only_near_their_corner() {
        let mut video = state(TileKind::Video, Some(false), true);
        assert!(video.media_bar_target().is_some());
        assert!(video.primary_bar_target().is_none());
        video.pointer = Some((5, 70));
        assert!(video.media_bar_target().is_none());
        assert!(video.primary_bar_target().is_some());
        video.pointer = Some((800, 500));
        assert!(video.media_bar_target().is_none() && video.primary_bar_target().is_none());
        video.pointer = None;
        assert!(video.media_bar_target().is_none() && video.primary_bar_target().is_none());
    }

    #[test]
    fn media_clicks_map_to_outcomes() {
        let video = state(TileKind::Video, Some(false), true);
        let click = |action| video.media_click(BarClick::Action(action));
        assert_eq!(click(MediaActionId::Mute), PlainOutcome::SetMuted(true));
        assert_eq!(click(MediaActionId::Delete), PlainOutcome::Ui(UiAction::DeleteTile(7)));
        assert_eq!(click(MediaActionId::OpenWith), PlainOutcome::Ui(UiAction::ToggleOpenWith(7)));
        assert_eq!(click(MediaActionId::Close), PlainOutcome::Ui(UiAction::CloseActiveTab));
        assert_eq!(click(MediaActionId::OpenAsTab), PlainOutcome::None);
        assert_eq!(click(MediaActionId::Sticky), PlainOutcome::None);
        assert_eq!(video.media_click(BarClick::MenuItem(1001)), PlainOutcome::Ui(UiAction::OpenWithItem(7, 1001)));
        let muted = state(TileKind::Video, Some(true), true);
        assert_eq!(muted.media_click(BarClick::Action(MediaActionId::Mute)), PlainOutcome::SetMuted(false));
        let image = state(TileKind::Image, None, false);
        assert_eq!(image.media_click(BarClick::Action(MediaActionId::Mute)), PlainOutcome::None);
    }

    #[test]
    fn primary_clicks_map_to_outcomes() {
        let playing = state(TileKind::Video, Some(false), true);
        assert_eq!(playing.primary_click(PrimaryActionId::PlayPauseAll), PlainOutcome::SetPaused(true));
        assert_eq!(playing.primary_click(PrimaryActionId::Close), PlainOutcome::Ui(UiAction::CloseActiveTab));
        let paused = state(TileKind::Video, Some(false), false);
        assert_eq!(paused.primary_click(PrimaryActionId::PlayPauseAll), PlainOutcome::SetPaused(false));
        assert_eq!(paused.primary_click(PrimaryActionId::Pin), PlainOutcome::None);
        let image = state(TileKind::Image, None, false);
        assert_eq!(image.primary_click(PrimaryActionId::PlayPauseAll), PlainOutcome::None);
    }
}
