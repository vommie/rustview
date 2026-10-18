//! The records of the open folder (or query result) with an id index, so that "the record of media X" is O(1).
//!
//! The view code looks records up by id on every drained thumbnail, selection change and context menu; a linear scan per lookup
//! turns a folder of a million items into a trillion comparisons. The store reads like a slice (iterate, `len`, `get`), and every
//! operation that changes which records exist keeps the index in step.
//!
//! The records live behind an `Arc`, so a background job can work on a `snapshot` while the UI keeps reading and editing the store:
//! the first edit while a snapshot is alive copies the records (copy-on-write), later edits do not. `generation` identifies the
//! set of records and their order; it changes with every structural change (replace, clear, retain, remove) and never repeats, so
//! a result computed from a snapshot can tell whether it still fits.

use super::filters::IndexedMediaRecord;
use crate::library::models::MediaId;
use crate::thumbnails::ram_cache::IdBuildHasher;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
}

type IdIndex = HashMap<MediaId, u32, IdBuildHasher>;

#[derive(Debug, Clone)]
pub struct MediaStore {
    records: Arc<Vec<IndexedMediaRecord>>,
    by_id: Arc<IdIndex>,
    generation: u64,
}

impl Default for MediaStore {
    fn default() -> Self {
        Self { records: Arc::default(), by_id: Arc::default(), generation: next_generation() }
    }
}

fn index_of(records: &[IndexedMediaRecord]) -> IdIndex {
    let mut by_id = IdIndex::with_capacity_and_hasher(records.len(), IdBuildHasher::default());
    for (position, record) in records.iter().enumerate() {
        by_id.insert(record.record.id, position as u32);
    }
    by_id
}

impl MediaStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a store (and its index) without touching any shared state, so it can be done off the UI thread.
    pub fn from_records(records: Vec<IndexedMediaRecord>) -> Self {
        let by_id = Arc::new(index_of(&records));
        Self { records: Arc::new(records), by_id, generation: next_generation() }
    }

    /// Replaces all records and rebuilds the index.
    pub fn replace(&mut self, records: Vec<IndexedMediaRecord>) {
        *self = Self::from_records(records);
    }

    /// Renumbers `IndexedMediaRecord::index` (the layout stores these positions in `active_indices`) and rebuilds the id index.
    fn reindex(&mut self) {
        for (position, record) in Arc::make_mut(&mut self.records).iter_mut().enumerate() {
            record.index = position;
        }
        self.by_id = Arc::new(index_of(&self.records));
        self.generation = next_generation();
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Identifies the records and their order; changes with every structural change and never repeats.
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A cheap, immutable view of the current records for a background job.
    pub fn snapshot(&self) -> Arc<Vec<IndexedMediaRecord>> {
        self.records.clone()
    }

    /// The record of the media `id`.
    #[inline]
    pub fn find(&self, id: MediaId) -> Option<&IndexedMediaRecord> {
        self.by_id.get(&id).and_then(|&position| self.records.get(position as usize))
    }

    #[inline]
    pub fn find_mut(&mut self, id: MediaId) -> Option<&mut IndexedMediaRecord> {
        let position = *self.by_id.get(&id)?;
        Arc::make_mut(&mut self.records).get_mut(position as usize)
    }

    /// Position of the record in the store.
    #[inline]
    pub fn position_of(&self, id: MediaId) -> Option<usize> {
        self.by_id.get(&id).map(|&position| position as usize)
    }

    pub fn retain(&mut self, keep: impl FnMut(&IndexedMediaRecord) -> bool) {
        Arc::make_mut(&mut self.records).retain(keep);
        self.reindex();
    }

    pub fn remove(&mut self, position: usize) -> IndexedMediaRecord {
        let removed = Arc::make_mut(&mut self.records).remove(position);
        self.reindex();
        removed
    }
}

impl<'a> IntoIterator for &'a MediaStore {
    type Item = &'a IndexedMediaRecord;
    type IntoIter = std::slice::Iter<'a, IndexedMediaRecord>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.iter()
    }
}

/// Read access like a slice. Mutable slice access (`iter_mut`) is fine as long as no record changes its id.
impl Deref for MediaStore {
    type Target = [IndexedMediaRecord];

    fn deref(&self) -> &Self::Target {
        &self.records
    }
}

impl DerefMut for MediaStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.records).as_mut_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::models::MediaRecord;
    use crate::media::{MediaFormat, MediaType};
    use crate::query::folder_dir::FolderDir;

    fn record(id: MediaId, index: usize) -> IndexedMediaRecord {
        let record = MediaRecord {
            id,
            folder_id: 1,
            filename: format!("{id}.jpg"),
            file_hash: [0; 16],
            file_size: 1,
            modification_time: 0,
            media_type: MediaType::Image,
            format: MediaFormat::Jpeg,
            width: 1,
            height: 1,
            rating: 0,
            last_seen: 0,
            is_deleted: false,
            creation_time: 0,
            duration_ms: 0,
            is_corrupted: false,
            source: None,
        };
        IndexedMediaRecord::new(index, record, FolderDir::new("/tmp"), 0)
    }

    #[test]
    fn retain_renumbers_the_record_positions() {
        let mut store = MediaStore::from_records((0..5).map(|i| record(i + 10, i as usize)).collect());
        store.retain(|m| m.record.id != 11 && m.record.id != 13);
        let positions: Vec<(MediaId, usize)> = store.iter().map(|m| (m.record.id, m.index)).collect();
        assert_eq!(positions, vec![(10, 0), (12, 1), (14, 2)]);
        assert_eq!(store.position_of(14), Some(2));
    }

    #[test]
    fn remove_renumbers_the_record_positions() {
        let mut store = MediaStore::from_records((0..3).map(|i| record(i + 1, i as usize)).collect());
        store.remove(0);
        assert!(store.iter().enumerate().all(|(position, m)| m.index == position));
    }
}
