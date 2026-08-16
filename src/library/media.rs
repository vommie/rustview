use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, MediaId, MediaRecord};
use crate::media::FileHash;

pub struct MediaRepository<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> MediaRepository<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    pub fn get(&self, id: MediaId) -> Result<Option<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        Ok(self.db.media.get(&rtxn, &id)?)
    }

    /// Retrieves all active (non-deleted) media records for the specified folder.
    pub fn get_by_folder(&self, folder_id: FolderId) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &folder_id)?
            .unwrap_or_default();
        let mut records = Vec::with_capacity(media_ids.len());

        for id in media_ids {
            if let Some(record) = self.db.media.get(&rtxn, &id)?
                && !record.is_deleted
            {
                records.push(record);
            }
        }

        Ok(records)
    }

    /// Retrieves all media records (including soft-deleted) for the specified folder.
    pub fn get_all_by_folder(&self, folder_id: FolderId) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &folder_id)?
            .unwrap_or_default();
        let mut records = Vec::with_capacity(media_ids.len());

        for id in media_ids {
            if let Some(record) = self.db.media.get(&rtxn, &id)? {
                records.push(record);
            }
        }

        Ok(records)
    }

    /// Counts only active (non-deleted) media files associated with the folder.
    pub fn get_active_count(&self, folder_id: FolderId) -> Result<usize, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &folder_id)?
            .unwrap_or_default();
        let mut count = 0;
        for id in media_ids {
            if let Some(rec) = self.db.media.get(&rtxn, &id)?
                && !rec.is_deleted
            {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn get_by_hash(&self, hash: &FileHash) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_hash
            .get(&rtxn, &hash[..])?
            .unwrap_or_default();
        let mut records = Vec::with_capacity(media_ids.len());

        for id in media_ids {
            if let Some(record) = self.db.media.get(&rtxn, &id)?
                && !record.is_deleted
            {
                records.push(record);
            }
        }

        Ok(records)
    }

    pub fn set_deleted(&self, media_id: MediaId, is_deleted: bool) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;
        if let Some(mut record) = self.db.media.get(&wtxn, &media_id)? {
            if record.is_deleted != is_deleted {
                record.is_deleted = is_deleted;
                self.db.media.put(&mut wtxn, &media_id, &record)?;
            }
        } else {
            return Err(LibraryError::MediaNotFound(media_id));
        }
        wtxn.commit()?;
        Ok(())
    }

    pub fn set_rating(&self, media_id: MediaId, rating: u8) -> Result<(), LibraryError> {
        let rating = rating.min(5);
        let mut wtxn = self.db.env.write_txn()?;

        if let Some(mut record) = self.db.media.get(&wtxn, &media_id)? {
            record.rating = rating;
            self.db.media.put(&mut wtxn, &media_id, &record)?;
        } else {
            return Err(LibraryError::MediaNotFound(media_id));
        }

        wtxn.commit()?;
        Ok(())
    }
}
