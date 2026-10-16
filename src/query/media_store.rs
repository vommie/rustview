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

    fn reindex(&mut self) {
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
    use crate::query::FolderDir;

    fn record(id: MediaId, index: usize) -> IndexedMediaRecord {
        let record = MediaRecord {
            id,
            folder_id: 1,
            filename: format!("{id}.webp"),
            file_hash: [0; 16],
            file_size: 1,
            modification_time: 0,
            media_type: MediaType::Image,
            format: MediaFormat::Webp,
            width: 0,
            height: 0,
            rating: 0,
            last_seen: 0,
            is_deleted: false,
            creation_time: 0,
            duration_ms: 0,
            is_corrupted: false,
            source: None,
        };
        IndexedMediaRecord::new(index, record, FolderDir::new("/m"), 0)
    }

    fn store(ids: &[MediaId]) -> MediaStore {
        let mut store = MediaStore::new();
        store.replace(ids.iter().enumerate().map(|(i, &id)| record(id, i)).collect());
        store
    }

    #[test]
    fn lookups_by_id_find_the_record() {
        let store = store(&[10, 20, 30]);
        assert_eq!(store.find(20).map(|r| r.record.id), Some(20));
        assert_eq!(store.position_of(30), Some(2));
        assert!(store.find(99).is_none());
    }

    #[test]
    fn mutable_lookup_changes_the_stored_record() {
        let mut store = store(&[1, 2]);
        store.find_mut(2).unwrap().record.rating = 5;
        assert_eq!(store.find(2).unwrap().record.rating, 5);
    }

    #[test]
    fn retain_keeps_the_index_in_step() {
        let mut store = store(&[1, 2, 3, 4]);
        store.retain(|r| r.record.id % 2 == 0);
        assert_eq!(store.len(), 2);
        assert_eq!(store.position_of(4), Some(1));
        assert!(store.find(1).is_none());
    }

    #[test]
    fn remove_shifts_the_positions_after_it() {
        let mut store = store(&[1, 2, 3]);
        let removed = store.remove(0);
        assert_eq!(removed.record.id, 1);
        assert_eq!(store.position_of(3), Some(1));
    }

    #[test]
    fn replace_and_clear_reset_the_index() {
        let mut store = store(&[1, 2]);
        store.replace(vec![record(7, 0)]);
        assert!(store.find(1).is_none() && store.find(7).is_some());
        store.clear();
        assert!(store.is_empty() && store.find(7).is_none());
    }

    #[test]
    fn the_store_reads_like_a_slice() {
        let store = store(&[5, 6, 7]);
        assert_eq!(store.iter().map(|r| r.record.id).collect::<Vec<_>>(), vec![5, 6, 7]);
        assert_eq!(store.get(1).map(|r| r.record.id), Some(6));
    }

    #[test]
    fn a_snapshot_is_unaffected_by_later_edits_and_generations_never_repeat() {
        let mut store = store(&[1, 2, 3]);
        let before = store.generation();
        let snapshot = store.snapshot();

        store.find_mut(2).unwrap().record.rating = 4;
        assert_eq!(snapshot[1].record.rating, 0);
        assert_eq!(store.generation(), before, "an edit of a record is not a structural change");

        store.remove(0);
        assert_ne!(store.generation(), before);
        assert_eq!(snapshot.len(), 3);

        let after_remove = store.generation();
        store.replace(vec![record(9, 0)]);
        assert!(store.generation() != after_remove && store.generation() != before);
    }

    #[test]
    fn a_store_built_from_records_is_indexed() {
        let store = MediaStore::from_records(vec![record(4, 0), record(8, 1)]);
        assert_eq!(store.position_of(8), Some(1));
        assert!(store.find(4).is_some());
    }
}
