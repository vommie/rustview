//! Keeps the tile pool of the grid and the table filled for the viewport.
//!
//! The Slint side reports the viewport (first visible row, visible rows, columns, total); this module decides which list
//! positions the pool must hold, rewrites only the model rows whose position changed (`set_row_data` on a stable
//! `VecModel`, so tiles are never destroyed while scrolling) and asks the thumbnail loader for the tiles that still lack their
//! picture. It runs on the UI thread, reads memory only and does no decoding.

use super::geometry::ViewGeometry;
use super::view_builder::build_view;
use crate::ui::browse_folders::{Slot, folder_row_view};
use crate::ui::state::UiState;
use crate::ui::{MainWindow, MediaItemView};
use rustview_core::library::models::MediaId;
use rustview_core::thumbnails::ThumbnailManager;
use slint::{Image, Model, ModelRc, VecModel};
use std::rc::Rc;
use std::time::Instant;

/// What one synchronization did, for the benchmark counters and the logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncOutcome {
    /// Rows that were rewritten.
    pub rows_written: usize,
    /// The whole pool was rebuilt (new model).
    pub rebuilt: bool,
    /// Thumbnails requested from the loader.
    pub requested: usize,
}

/// Rows that carry no item (the pool is larger than the window at the end of the list) are parked off the grid.
fn placeholder_row() -> MediaItemView {
    MediaItemView {
        index: -1,
        ..Default::default()
    }
}

/// The picture for `item`, if it is in RAM; the id is queued for loading otherwise.
fn tile_image(thumbnails: &ThumbnailManager, id: MediaId, is_table: bool) -> Image {
    if is_table {
        return Image::default();
    }
    thumbnails
        .cached_thumbnail(id)
        .map_or_else(Image::default, Image::from_rgb8)
}

fn build_row(st: &UiState, thumbnails: &ThumbnailManager, position: usize, is_table: bool) -> MediaItemView {
    match st.slot_at(position) {
        Slot::Media(row) => match st.item_at(row) {
            Some(item) => {
                let image = tile_image(thumbnails, item.record.id, is_table);
                build_view(item, position, st.file_selection.is_selected(item.record.id), image)
            }
            None => placeholder_row(),
        },
        Slot::Folder(folder) => {
            let selected = st.folder_rows.is_selected(folder);
            folder_row_view(
                st.folder_hierarchy.as_deref(),
                folder,
                position,
                selected,
                &st.folder_times,
            )
        }
        Slot::Empty => placeholder_row(),
    }
}

/// Brings the pool in line with the viewport. `force` refills every tile (new list, new sort order, new view mode).
pub fn sync_window(
    window: &MainWindow,
    st: &mut UiState,
    thumbnails: &ThumbnailManager,
    geometry: ViewGeometry,
    first_row: usize,
    total: usize,
    force: bool,
) -> SyncOutcome {
    let is_table = window.get_is_table_view();
    let target = geometry.window(first_row, total);
    let pool = geometry.pool_size(total);
    let model = window.get_media_items();
    let rebuild = force || pool != st.ring.pool() || model.row_count() != pool;
    let mut outcome = SyncOutcome {
        rebuilt: rebuild,
        ..Default::default()
    };

    if rebuild {
        st.ring.reset(pool);
        let fresh = st.ring.advance(target.clone());
        let rows = Rc::new(VecModel::from(vec![placeholder_row(); pool]));
        for &position in &fresh {
            rows.set_row_data(st.ring.slot_of(position), build_row(st, thumbnails, position, is_table));
        }
        outcome.rows_written = fresh.len();
        window.set_media_items(ModelRc::from(rows));
    } else {
        let fresh = st.ring.advance(target.clone());
        for &position in &fresh {
            model.set_row_data(st.ring.slot_of(position), build_row(st, thumbnails, position, is_table));
        }
        outcome.rows_written = fresh.len();
    }

    if !is_table {
        outcome.requested = request_missing_thumbnails(st, thumbnails, &target, first_row, geometry);
    }
    st.view_geometry = Some(geometry);
    outcome
}

/// Queues every tile of the window that has no picture in RAM, the visible ones first.
fn request_missing_thumbnails(
    st: &UiState,
    thumbnails: &ThumbnailManager,
    window: &std::ops::Range<usize>,
    first_row: usize,
    geometry: ViewGeometry,
) -> usize {
    let center = first_row * geometry.columns + geometry.visible_rows * geometry.columns / 2;
    let mut missing: Vec<(usize, MediaId)> = window
        .clone()
        .filter_map(|position| {
            let Slot::Media(row) = st.slot_at(position) else {
                return None;
            };
            let id = st.item_at(row)?.record.id;
            (!thumbnails.cache().contains(id)).then_some((position.abs_diff(center), id))
        })
        .collect();
    missing.sort_unstable_by_key(|&(distance, _)| distance);
    thumbnails.request_view_thumbnails(missing.into_iter().map(|(_, id)| id))
}

/// Wall-clock cost of a closure, in microseconds (for the scroll benchmark).
pub fn timed<T>(work: impl FnOnce() -> T) -> (T, u64) {
    let started = Instant::now();
    let value = work();
    (value, started.elapsed().as_micros() as u64)
}
