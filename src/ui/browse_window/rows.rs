//! Targeted updates of the tiles that exist: the one place that maps a media id to the model row (slot) that shows it.

use crate::library::models::MediaId;
use crate::ui::bridge_state::UiState;
use crate::ui::{MainWindow, MediaItemView};
use slint::Model;

/// The model row that shows `media_id`, if a tile holds it right now.
pub fn slot_of_media(state: &UiState, media_id: MediaId) -> Option<usize> {
    let position = *state.media_row_map.get(&media_id)?;
    state.ring.slot_if_held(position)
}

/// Changes the tile of `media_id` in place (the Slint instance and its other properties stay as they are). Returns whether a
/// tile was updated.
pub fn update_tile(
    window: &MainWindow,
    state: &UiState,
    media_id: MediaId,
    update: impl FnOnce(&mut MediaItemView),
) -> bool {
    let Some(slot) = slot_of_media(state, media_id) else {
        return false;
    };
    let model = window.get_media_items();
    let Some(mut row) = model.row_data(slot) else {
        return false;
    };
    if row.id != media_id as i32 {
        return false;
    }
    update(&mut row);
    model.set_row_data(slot, row);
    true
}
