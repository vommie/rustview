//! How many of the visible media tiles of the thumbnail view still wait for their thumbnail: what the user sees as empty
//! tiles while scrolling fast.

use crate::ui::MainWindow;
use crate::ui::browse_folders::Slot;
use crate::ui::state::UiState;
use parking_lot::RwLock;
use rustview_core::util::cast::SaturatingAs;
use slint::Model;

/// (tiles without a thumbnail, visible media tiles); `None` where no thumbnails are shown (table, tabs) or the state is busy.
pub fn count(window: &MainWindow, state: &RwLock<UiState>) -> Option<(u32, u32)> {
    if window.get_is_table_view() || window.get_active_view_mode() != 0 {
        return None;
    }
    let st = state.try_read()?;
    let geometry = st.view_geometry?;
    let total = st.view_len();
    let pool = st.ring.pool();
    if pool == 0 || total == 0 || geometry.columns == 0 {
        return Some((0, 0));
    }
    let top: usize = window.invoke_get_top_visible_index().saturating_as();
    let first = top - top % geometry.columns;
    let end = (first + geometry.visible_rows * geometry.columns).min(total);
    let model = window.get_media_items();
    let (mut blank, mut shown) = (0u32, 0u32);
    for position in first..end {
        if !matches!(st.slot_at(position), Slot::Media(_)) {
            continue;
        }
        shown += 1;
        let ready = model
            .row_data(st.ring.slot_of(position))
            .is_some_and(|row| row.index == position.saturating_as::<i32>() && row.thumbnail.size().width > 0);
        if !ready {
            blank += 1;
        }
    }
    Some((blank, shown))
}
