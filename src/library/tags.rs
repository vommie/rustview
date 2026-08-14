use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{MediaId, TagId, TagRecord};
use super::schema::SEQ_TAG_ID;

pub struct TagManager<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> TagManager<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    fn composite_key(parent_id: Option<TagId>, name: &str) -> String {
        format!("{}:{}", parent_id.unwrap_or(0), name.trim().to_lowercase())
    }

    pub fn create_tag(
        &self,
        name: &str,
        parent_id: Option<TagId>,
    ) -> Result<TagRecord, LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        if let Some(pid) = parent_id
            && self.db.tags.get(&wtxn, &pid)?.is_none()
        {
            return Err(LibraryError::TagNotFound(pid));
        }

        let key = Self::composite_key(parent_id, name);
        if let Some(existing_id) = self.db.tags_by_parent_name.get(&wtxn, &key)? {
            let record = self.db.tags.get(&wtxn, &existing_id)?.unwrap();
            return Ok(record);
        }

        let tag_id = self.db.next_id(&mut wtxn, SEQ_TAG_ID)? as u32;
        let record = TagRecord {
            id: tag_id,
            parent_id,
            name: name.trim().to_string(),
        };

        self.db.tags.put(&mut wtxn, &tag_id, &record)?;
        self.db.tags_by_parent_name.put(&mut wtxn, &key, &tag_id)?;
        self.db.media_by_tag.put(&mut wtxn, &tag_id, &Vec::new())?;

        wtxn.commit()?;
        Ok(record)
    }

    pub fn assign_tag(&self, media_id: MediaId, tag_id: TagId) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        if self.db.media.get(&wtxn, &media_id)?.is_none() {
            return Err(LibraryError::MediaNotFound(media_id));
        }
        if self.db.tags.get(&wtxn, &tag_id)?.is_none() {
            return Err(LibraryError::TagNotFound(tag_id));
        }

        // tags_by_media
        let mut media_tags = self
            .db
            .tags_by_media
            .get(&wtxn, &media_id)?
            .unwrap_or_default();
        if !media_tags.contains(&tag_id) {
            media_tags.push(tag_id);
            self.db
                .tags_by_media
                .put(&mut wtxn, &media_id, &media_tags)?;
        }

        // media_by_tag
        let mut tag_media = self
            .db
            .media_by_tag
            .get(&wtxn, &tag_id)?
            .unwrap_or_default();
        if !tag_media.contains(&media_id) {
            tag_media.push(media_id);
            self.db.media_by_tag.put(&mut wtxn, &tag_id, &tag_media)?;
        }

        wtxn.commit()?;
        Ok(())
    }

    pub fn remove_tag_from_media(
        &self,
        media_id: MediaId,
        tag_id: TagId,
    ) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        if let Some(mut media_tags) = self.db.tags_by_media.get(&wtxn, &media_id)? {
            media_tags.retain(|&id| id != tag_id);
            self.db
                .tags_by_media
                .put(&mut wtxn, &media_id, &media_tags)?;
        }

        if let Some(mut tag_media) = self.db.media_by_tag.get(&wtxn, &tag_id)? {
            tag_media.retain(|&id| id != media_id);
            self.db.media_by_tag.put(&mut wtxn, &tag_id, &tag_media)?;
        }

        wtxn.commit()?;
        Ok(())
    }

    pub fn get_tags_for_media(&self, media_id: MediaId) -> Result<Vec<TagRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let tag_ids = self
            .db
            .tags_by_media
            .get(&rtxn, &media_id)?
            .unwrap_or_default();
        let mut records = Vec::with_capacity(tag_ids.len());

        for tag_id in tag_ids {
            if let Some(rec) = self.db.tags.get(&rtxn, &tag_id)? {
                records.push(rec);
            }
        }

        Ok(records)
    }

    pub fn get_media_for_tag(&self, tag_id: TagId) -> Result<Vec<MediaId>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        Ok(self
            .db
            .media_by_tag
            .get(&rtxn, &tag_id)?
            .unwrap_or_default())
    }

    pub fn list_all_tags(&self) -> Result<Vec<TagRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut tags = Vec::new();
        for entry in self.db.tags.iter(&rtxn)? {
            let (_, record) = entry?;
            tags.push(record);
        }
        Ok(tags)
    }
}
