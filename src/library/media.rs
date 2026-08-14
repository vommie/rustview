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

    pub fn get_by_folder(&self, folder_id: FolderId) -> Result<Vec<MediaRecord>, LibraryError> {
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

    pub fn get_by_hash(&self, hash: &FileHash) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_hash
            .get(&rtxn, &hash[..])?
            .unwrap_or_default();
        let mut records = Vec::with_capacity(media_ids.len());

        for id in media_ids {
            if let Some(record) = self.db.media.get(&rtxn, &id)? {
                records.push(record);
            }
        }

        Ok(records)
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
