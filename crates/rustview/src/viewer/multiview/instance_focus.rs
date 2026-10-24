//! Instance behavior around the selected tile (`focus`): selecting it, keeping it in line with the tiles, the marks a host draws
//! for it and the corner marks of a video, which mpv draws itself (`focus_overlay`).

use super::focus_overlay::{self, CornerMarks};
use super::follow::TileKey;
use super::hint::HintAnchor;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::media_feedback::{FADE, HOLD};
use super::model::TileKind;
use super::prefs;
use super::route::HostMode;
use super::view::FocusMarkView;
use rustview_core::library::models::MediaId;
use rustview_core::settings::schema::MULTIVIEW_FOCUS_CORNER_THICKNESS_LOGICAL_PX;
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use std::time::Instant;

impl MultiviewInstance {
    fn tile_keys(&self) -> Vec<TileKey> {
        self.tiles.iter().map(|t| t.key).collect()
    }

    /// The host shows the marks and takes the clicks (not the tab-grid miniature).
    fn host_is_interactive(&self) -> bool {
        matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView)
    }

    /// A plain left click on the medium of tile `index` selects it. Returns whether the selection changed.
    pub fn select_tile_at(&mut self, index: usize) -> bool {
        self.tile_key_at(index).is_some_and(|key| self.select_tile(key))
    }

    /// Selects tile `key` (a click on its medium, on its video or on its media bar). Nothing changes while a dialog locks the
    /// view. Returns whether the selection changed.
    pub fn select_tile(&mut self, key: TileKey) -> bool {
        if self.host.modal_locked || !self.host_is_interactive() {
            return false;
        }
        let keys = self.tile_keys();
        if !self.focus.select(key, &keys) {
            return false;
        }
        let media_id = self.tiles.iter().find(|t| t.key == key).map(|t| t.media_id);
        tracing::info!(
            instance = self.id,
            tile = key,
            media_id,
            tiles = keys.len(),
            "[Multiview: Focus] Medium selected"
        );
        self.sync_focus_overlays();
        self.bump();
        true
    }

    /// The tile a key acts on: the only tile, or the selected one of several (`None` = several and none selected).
    pub fn focus_target(&self) -> Option<TileKey> {
        self.focus.target(&self.tile_keys())
    }

    /// The medium of `focus_target`.
    pub fn focus_target_media(&self) -> Option<MediaId> {
        let key = self.focus_target()?;
        self.tiles.iter().find(|t| t.key == key).map(|t| t.media_id)
    }

    /// The selected tile of a view of two or more tiles (the one that carries the marks).
    pub fn focus_marked(&self) -> Option<TileKey> {
        self.focus.marked(&self.tile_keys())
    }

    /// Brings the selection in line with the tiles after they changed (every change of the tile set ends in `apply_layout`):
    /// a tile that left is no longer selected, a lone tile is.
    pub(super) fn normalize_focus(&mut self) {
        let keys = self.tile_keys();
        let before = self.focus.target(&keys);
        if self.focus.normalize(&keys) {
            tracing::debug!(
                instance = self.id,
                selected = ?self.focus.target(&keys),
                before = ?before,
                tiles = keys.len(),
                "[Multiview: Focus] Selection follows the tiles"
            );
        }
    }

    /// Where a hint of this Multiview goes: the middle of its view in the host window. `None` while no interactive host shows it.
    pub fn hint_anchor(&self) -> Option<HintAnchor> {
        self.host_is_interactive().then_some(HintAnchor {
            host_xid: self.host.surface.parent_xid,
            view: self.host.surface.rect,
            dpi: self.host.surface.dpi,
        })
    }

    /// Length and thickness of the corner marks in host pixels.
    fn focus_mark_size(&self) -> (i32, i32) {
        let dpi = f64::from(self.host.surface.dpi);
        let px = |logical: u32| (f64::from(logical) * dpi).round().to_int::<i32>().max(1);
        (
            px(prefs::current().focus_marks.corner_logical_px),
            px(MULTIVIEW_FOCUS_CORNER_THICKNESS_LOGICAL_PX),
        )
    }

    /// The marks are on screen: not hidden with the resting cursor, or shown for a moment by a key.
    fn marks_shown(&self) -> bool {
        let hidden_with_cursor = prefs::current().focus_marks.hide_with_cursor && self.cursor_hidden;
        !hidden_with_cursor || self.marks_revealed_until.is_some_and(|until| Instant::now() < until)
    }

    /// A key reached the selected tile: its marks show for about a second (as long as the feedback of a rating), also while
    /// they are hidden with the cursor.
    pub(super) fn reveal_marks(&mut self, now: Instant) {
        let was_shown = self.marks_shown();
        self.marks_revealed_until = Some(now + HOLD + FADE);
        if !was_shown {
            tracing::debug!(
                instance = self.id,
                "[Multiview: Focus] Marks shown for a moment by a key"
            );
            self.sync_focus_overlays();
            self.bump();
        }
    }

    /// Called every tick: a moment of shown marks that is over hides them again (while the cursor is hidden).
    pub(super) fn poll_mark_reveal(&mut self, now: Instant) {
        if self.marks_revealed_until.is_some_and(|until| now >= until) {
            self.marks_revealed_until = None;
            self.marks_visibility_changed();
        }
    }

    /// The marks appeared or went (the cursor was hidden or shown, a moment of shown marks ended).
    pub(super) fn marks_visibility_changed(&mut self) {
        if self.focus_marked().is_some() {
            self.sync_focus_overlays();
            self.bump();
        }
    }

    /// The marks tile `index` carries while it is the selected one of several.
    pub(super) fn focus_view(&self, index: usize) -> Option<FocusMarkView> {
        let tile = self.tiles.get(index)?;
        if !self.host_is_interactive() || self.focus_marked() != Some(tile.key) || !self.marks_shown() {
            return None;
        }
        let placed = self.layout.tiles.get(index)?;
        let projection = &self.host.projection;
        let (column, content) = (projection.rect(placed.column), projection.rect(placed.content));
        let (corner, thickness) = self.focus_mark_size();
        Some(FocusMarkView {
            column: Rect::new(column.x - content.x, column.y - content.y, column.w, column.h),
            brighten: f32::from(prefs::current().focus_marks.brighten_percent) / 100.0,
            corner,
            thickness,
        })
    }

    /// Brings the corner marks that mpv draws over the videos in line with the selection: the selected video of several gets
    /// them (in the corners of its window), every other video has none. Costs a comparison per video when nothing changed, so
    /// the host tick may call it to retry a mark mpv did not take yet (no picture).
    pub(super) fn sync_focus_overlays(&mut self) {
        let marked = if self.host_is_interactive() && self.marks_shown() {
            self.focus_marked()
        } else {
            None
        };
        let (length, thickness) = self.focus_mark_size();
        let projection = self.host.projection;
        for (tile, placed) in self.tiles.iter_mut().zip(&self.layout.tiles) {
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            let wanted = if marked == Some(tile.key) && tile.kind == TileKind::Video && tile.error.is_none() {
                let Rect { w, h, .. } = projection.rect(placed.content);
                CornerMarks::new(
                    (w.saturating_as::<u32>(), h.saturating_as::<u32>()),
                    length.saturating_as::<u32>(),
                    thickness.saturating_as::<u32>(),
                )
            } else {
                None
            };
            if video.focus_overlay == wanted {
                continue;
            }
            if let Some(marks) = wanted {
                video.focus_overlay = focus_overlay::show(&video.player, &marks).then_some(marks);
                if video.focus_overlay.is_some() {
                    tracing::debug!(tile = tile.key, window = ?marks.window, "[Multiview: Focus] Video marks shown");
                }
            } else {
                focus_overlay::hide(&video.player);
                video.focus_overlay = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::tile::TileSeed;
    use crate::viewer::multiview::{HostSurface, follow::TileKey};
    use rustview_core::library::models::MediaId;
    use slint::{Rgb8Pixel, SharedPixelBuffer};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
    }

    /// An image that is decoded already, so a host starts nothing for it.
    fn seed(media: MediaId) -> TileSeed {
        TileSeed {
            pixels: Some(SharedPixelBuffer::<Rgb8Pixel>::new(8, 6)),
            orig_size: Some((800, 600)),
            ..TileSeed::new(
                media,
                TileKind::Image,
                1.5,
                PathBuf::from(format!("/nowhere/{media}")),
                format!("m{media}"),
            )
        }
    }

    fn shown(media: &[MediaId]) -> SharedInstance {
        let shared = MultiviewInstance::create(Arc::new(NullSink), media.iter().map(|&m| seed(m)).collect());
        shared.lock().set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        shared
    }

    fn keys(mv: &MultiviewInstance) -> Vec<TileKey> {
        (0..mv.tile_count()).filter_map(|i| mv.tile_key_at(i)).collect()
    }

    #[test]
    fn a_view_of_several_media_opens_without_a_target_and_a_click_selects() {
        let shared = shown(&[1, 2, 3]);
        let mut mv = shared.lock();
        assert_eq!(mv.focus_target(), None);
        assert!(mv.select_tile_at(1));
        assert_eq!(mv.focus_target_media(), Some(2));
        assert_eq!(mv.focus_marked(), mv.tile_key_at(1));
        assert!(mv.view().tiles[1].focus.is_some());
        assert!(mv.view().tiles[0].focus.is_none());
    }

    #[test]
    fn the_lone_medium_is_the_target_without_marks_and_stays_selected_when_duplicated() {
        let shared = shown(&[5]);
        let mut mv = shared.lock();
        assert_eq!(mv.focus_target_media(), Some(5));
        assert!(mv.view().tiles[0].focus.is_none());
        let lone = mv.tile_key_at(0).unwrap();
        let _ = mv.duplicate_tile(lone);
        assert_eq!(mv.tile_count(), 2);
        assert_eq!(mv.focus_marked(), Some(lone));
    }

    #[test]
    fn the_selected_tile_leaving_leaves_nothing_selected() {
        let shared = shown(&[1, 2, 3]);
        let mut mv = shared.lock();
        mv.select_tile_at(1);
        mv.drop_deleted(&[2]);
        assert_eq!(mv.tile_count(), 2);
        assert_eq!(mv.focus_target(), None);
        // Another tile leaving keeps a selection.
        mv.select_tile_at(0);
        let first = keys(&mv)[0];
        mv.drop_deleted(&[99]);
        assert_eq!(mv.focus_target(), Some(first));
    }

    #[test]
    fn a_media_bar_click_selects_its_tile() {
        let shared = shown(&[1, 2]);
        let mut mv = shared.lock();
        let second = mv.tile_key_at(1).unwrap();
        let _ = mv.perform_click(
            second,
            crate::viewer::multiview::media_actions::BarClick::Action(
                crate::viewer::multiview::media_actions::MediaActionId::Sticky,
            ),
        );
        assert_eq!(mv.focus_target(), Some(second));
    }

    #[test]
    fn nothing_is_selected_while_a_dialog_locks_the_view() {
        let shared = shown(&[1, 2]);
        let mut mv = shared.lock();
        mv.set_modal_locked(true);
        assert!(!mv.select_tile_at(0));
        mv.set_modal_locked(false);
        assert!(mv.select_tile_at(0));
    }
}
