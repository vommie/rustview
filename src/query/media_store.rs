//! The records of the open folder (or query result) with an id index, so that "the record of media X" is O(1).
//!
//! The view code looks records up by id on every drained thumbnail, selection change and context menu; a linear scan per lookup
//! turns a folder of a million items into a trillion comparisons. The store reads like a slice (iterate, `len`, `get`), and every
//! operation that changes which records exist keeps the index in step.

use super::filters::IndexedMediaRecord;
use crate::library::models::MediaId;
use crate::thumbnails::ram_cache::IdBuildHasher;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

#[derive(Debug, Default, Clone)]
pub struct MediaStore {
    records: Vec<IndexedMediaRecord>,
    by_id: HashMap<MediaId, u32, IdBuildHasher>,
}

impl MediaStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces all records and rebuilds the index.
    pub fn replace(&mut self, records: Vec<IndexedMediaRecord>) {
        self.records = records;
        self.reindex();
    }

    fn reindex(&mut self) {
        self.by_id.clear();
        self.by_id.reserve(self.records.len());
        for (position, record) in self.records.iter().enumerate() {
            self.by_id.insert(record.record.id, position as u32);
        }
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.by_id.clear();
    }

    /// The record of the media `id`.
    #[inline]
    pub fn find(&self, id: MediaId) -> Option<&IndexedMediaRecord> {
        self.by_id.get(&id).and_then(|&position| self.records.get(position as usize))
    }

    #[inline]
    pub fn find_mut(&mut self, id: MediaId) -> Option<&mut IndexedMediaRecord> {
        let position = *self.by_id.get(&id)?;
        self.records.get_mut(position as usize)
    }

    /// Position of the record in the store.
    #[inline]
    pub fn position_of(&self, id: MediaId) -> Option<usize> {
        self.by_id.get(&id).map(|&position| position as usize)
    }

    pub fn retain(&mut self, keep: impl FnMut(&IndexedMediaRecord) -> bool) {
        self.records.retain(keep);
        self.reindex();
    }

    pub fn remove(&mut self, position: usize) -> IndexedMediaRecord {
        let removed = self.records.remove(position);
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
        &mut self.records
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
}
