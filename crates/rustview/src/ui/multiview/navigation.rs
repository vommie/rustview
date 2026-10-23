//! Previous/next medium for a Multiview that shows a single image: the neighbor in the current Browse view.
//!
//! Decided from in-memory state only (the sorted and filtered view, the library flags of its items); the walk itself is
//! the pure `cyclic_neighbor`. Deleted, pending and corrupted items are skipped because the Multiview cannot show them.

use super::eligibility::is_playable;
use rustview_core::library::models::MediaId;
use crate::ui::bridge_state::UiState;
use crate::viewer::multiview::navigation::cyclic_neighbor;

/// The medium after (`forward`) or before `current` in the Browse view, wrapping around. `None` if there is no other
/// showable medium.
pub fn neighbor(st: &UiState, current: MediaId, forward: bool) -> Option<MediaId> {
    let item_at = |row: usize| {
        st.active_indices
            .get(row)
            .and_then(|&index| st.all_folder_media.get(index))
    };
    let row = cyclic_neighbor(
        st.active_indices.len(),
        st.media_row_map.get(&current).copied(),
        forward,
        |row| item_at(row).is_some_and(is_playable),
    )?;
    item_at(row).map(|item| item.record.id)
}
