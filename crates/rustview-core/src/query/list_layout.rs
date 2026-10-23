//! Turns a record list into the order the grid and the table show: filter, sort and the id -> row map.
//!
//! Pure and thread-safe: for large lists it runs on a worker thread (`ui::list_loader`) against a snapshot of the records, and the
//! UI thread only swaps the finished result in.

use super::filters::{IndexedMediaRecord, QueryFilter};
use super::sorting::{MediaSorter, SortColumn, SortOrder};
use crate::library::Library;
use crate::library::models::MediaId;
use crate::thumbnails::ram_cache::IdBuildHasher;
use crate::util::cast::SaturatingAs;
use std::collections::HashMap;
use std::time::Instant;

/// Media id -> position in the sorted list (the cheap id hasher: ids are dense counters).
pub type RowMap = HashMap<MediaId, usize, IdBuildHasher>;

#[derive(Debug, Clone)]
pub struct ListRequest {
    pub filter: QueryFilter,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    /// When the new query only narrows the previous one, the record positions of the previous result (the search starts there).
    pub refine_from: Option<Vec<usize>>,
}

#[derive(Debug, Default)]
pub struct ListLayout {
    /// Record positions in display order.
    pub indices: Vec<usize>,
    pub row_map: RowMap,
    /// Sum of the file sizes of the listed records.
    pub total_bytes: u64,
    pub filter_us: u64,
    pub sort_us: u64,
    pub map_us: u64,
}

pub fn lay_out(library: &Library, records: &[IndexedMediaRecord], request: &ListRequest) -> ListLayout {
    let started = Instant::now();
    let mut matched = request
        .filter
        .execute_indexed_refs(library, records, request.refine_from.as_deref());
    let filter_us = started.elapsed().as_micros().saturating_as::<u64>();

    let started = Instant::now();
    MediaSorter::sort_indexed_refs(&mut matched, request.sort_column, request.sort_order);
    let sort_us = started.elapsed().as_micros().saturating_as::<u64>();

    let started = Instant::now();
    let mut indices = Vec::with_capacity(matched.len());
    let mut row_map = RowMap::with_capacity_and_hasher(matched.len(), IdBuildHasher::default());
    let mut total_bytes = 0u64;
    for (row, item) in matched.iter().enumerate() {
        indices.push(item.index);
        row_map.insert(item.record.id, row);
        total_bytes = total_bytes.saturating_add(item.record.file_size);
    }
    let map_us = started.elapsed().as_micros().saturating_as::<u64>();

    ListLayout {
        indices,
        row_map,
        total_bytes,
        filter_us,
        sort_us,
        map_us,
    }
}
