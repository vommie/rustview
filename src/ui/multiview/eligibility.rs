//! Which selected items the Multiview shows, and what the context menu entry says about it.
//!
//! Single source of truth for the menu label, its enablement/tooltip and the list of tiles. Only in-memory library state
//! is used (`is_deleted`, `is_pending`, `is_corrupted`, the loaded row map); the drive is never touched. The Multiview
//! shows the first `MULTIVIEW_MAX_TILES` playable items in the current view order (the selection itself has no order).

use crate::library::models::MediaId;
use crate::media::MediaType;
use crate::query::filters::IndexedMediaRecord;
use crate::settings::schema::{MULTIVIEW_FALLBACK_ASPECT, MULTIVIEW_MAX_TILES};
use crate::ui::bridge_state::UiState;
use crate::ui::thumbnail_regeneration::eligibility::loaded_item;
use crate::viewer::multiview::follow::Wanted;
use crate::viewer::multiview::model::{TileKind, aspect_from_size};
use crate::viewer::multiview::tile::TileSeed;

/// What the context menu needs without resolving any tile (cheap, `Copy`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenSummary {
    pub selected: usize,
    pub playable: usize,
}

impl OpenSummary {
    /// The Multiview entry replaces "Open in Fullscreen" only for multi-selections.
    pub fn is_multi(&self) -> bool {
        self.selected > 1
    }

    pub fn is_actionable(&self) -> bool {
        self.playable >= 2
    }

    pub fn shown(&self) -> usize {
        self.playable.min(MULTIVIEW_MAX_TILES)
    }

    pub fn menu_label(&self) -> String {
        if self.playable > MULTIVIEW_MAX_TILES {
            format!("Open First {MULTIVIEW_MAX_TILES} in Fullscreen (Multiview)")
        } else {
            "Open in Fullscreen (Multiview)".to_string()
        }
    }

    pub fn hint(&self) -> String {
        if !self.is_actionable() {
            return "Multiview needs at least two playable files in the selection\n\
                    Disabled: deleted, pending and corrupted files are skipped"
                .to_string();
        }
        let mut text = format!(
            "Show {} media side by side in one fullscreen window",
            self.shown()
        );
        if self.playable > MULTIVIEW_MAX_TILES {
            text.push_str(&format!(
                " (only the first {MULTIVIEW_MAX_TILES} in view order)"
            ));
        }
        let skipped = self.selected - self.playable;
        match skipped {
            0 => {}
            1 => text.push_str(" (1 selected file is skipped)"),
            n => text.push_str(&format!(" ({n} selected files are skipped)")),
        }
        text
    }
}

#[derive(Debug, Clone, Default)]
pub struct OpenPlan {
    pub summary: OpenSummary,
    pub tiles: Vec<TileSeed>,
}

fn is_playable(item: &IndexedMediaRecord) -> bool {
    !item.is_deleted && !item.is_pending && !item.is_corrupted
}

/// Selected playable items with their row in the current view, sorted by row (O(k log k) for k selected items).
fn playable_in_view_order(st: &UiState) -> Vec<(usize, &IndexedMediaRecord)> {
    let mut items: Vec<(usize, &IndexedMediaRecord)> = st
        .file_selection
        .selected_ids()
        .iter()
        .filter_map(|&id| Some((*st.media_row_map.get(&id)?, loaded_item(st, id)?)))
        .filter(|(_, item)| is_playable(item))
        .collect();
    items.sort_by_key(|&(row, _)| row);
    items
}

pub fn summarize(st: &UiState) -> OpenSummary {
    let playable = st
        .file_selection
        .selected_ids()
        .iter()
        .filter_map(|&id| loaded_item(st, id))
        .filter(|item| is_playable(item))
        .count();
    OpenSummary {
        selected: st.file_selection.count(),
        playable,
    }
}

/// All selected playable media in view order (not capped; following the selection applies the tile cap).
pub fn wanted(st: &UiState) -> Vec<Wanted> {
    playable_in_view_order(st)
        .into_iter()
        .map(|(_, item)| Wanted {
            id: item.record.id,
            is_video: matches!(item.record.media_type, MediaType::Video),
        })
        .collect()
}

pub fn plan_tile(item: &IndexedMediaRecord) -> TileSeed {
    TileSeed {
        media_id: item.record.id,
        kind: if matches!(item.record.media_type, MediaType::Video) {
            TileKind::Video
        } else {
            TileKind::Image
        },
        aspect: aspect_from_size(
            item.record.width,
            item.record.height,
            MULTIVIEW_FALLBACK_ASPECT,
        ),
        path: item.full_path.clone(),
        filename: item.record.filename.clone(),
    }
}

/// The tiles to open with: the first `MULTIVIEW_MAX_TILES` playable items in view order.
pub fn plan_open(st: &UiState) -> OpenPlan {
    let items = playable_in_view_order(st);
    let summary = OpenSummary {
        selected: st.file_selection.count(),
        playable: items.len(),
    };
    let tiles = items
        .into_iter()
        .take(MULTIVIEW_MAX_TILES)
        .map(|(_, item)| plan_tile(item))
        .collect();
    OpenPlan { summary, tiles }
}

/// A single tile for an item that joined the selection later (following the selection).
pub fn plan_tile_for(st: &UiState, id: MediaId) -> Option<TileSeed> {
    loaded_item(st, id)
        .filter(|item| is_playable(item))
        .map(plan_tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(selected: usize, playable: usize) -> OpenSummary {
        OpenSummary { selected, playable }
    }

    #[test]
    fn needs_two_playable_files() {
        assert!(!s(1, 1).is_actionable());
        assert!(!s(3, 1).is_actionable());
        assert!(s(2, 2).is_actionable());
        assert!(s(9, 4).is_actionable());
    }

    #[test]
    fn label_names_the_cap_only_when_it_applies() {
        assert_eq!(s(3, 3).menu_label(), "Open in Fullscreen (Multiview)");
        assert_eq!(s(4, 4).menu_label(), "Open in Fullscreen (Multiview)");
        assert_eq!(
            s(7, 7).menu_label(),
            "Open First 4 in Fullscreen (Multiview)"
        );
    }

    #[test]
    fn hint_explains_cap_skips_and_why_it_is_disabled() {
        assert!(s(2, 1).hint().contains("at least two playable files"));
        assert_eq!(
            s(3, 3).hint(),
            "Show 3 media side by side in one fullscreen window"
        );
        assert!(s(7, 7).hint().contains("only the first 4 in view order"));
        assert!(s(3, 2).hint().ends_with("(1 selected file is skipped)"));
        assert!(s(6, 3).hint().ends_with("(3 selected files are skipped)"));
    }

    #[test]
    fn only_multi_selections_use_the_multiview_entry() {
        assert!(!s(1, 1).is_multi());
        assert!(s(2, 0).is_multi());
    }
}
