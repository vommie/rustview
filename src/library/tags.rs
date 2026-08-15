use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{MediaId, TagId, TagRecord};
use super::schema::SEQ_TAG_ID;
use std::collections::{HashMap, HashSet};

/// Pure, decoupled database repository managing tag persistence and hierarchical relations.
pub struct TagManager<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> TagManager<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    #[inline]
    pub fn composite_key(parent_id: Option<TagId>, name: &str) -> String {
        format!("{}:{}", parent_id.unwrap_or(0), name.trim().to_lowercase())
    }

    /// Creates a new tag under an optional parent. Rejects duplicates under the same parent.
    pub fn create_tag(
        &self,
        name: &str,
        parent_id: Option<TagId>,
    ) -> Result<TagRecord, LibraryError> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(LibraryError::InvalidTagName(name.to_string()));
        }

        let mut wtxn = self.db.env.write_txn()?;

        if let Some(pid) = parent_id
            && self.db.tags.get(&wtxn, &pid)?.is_none()
        {
            return Err(LibraryError::TagNotFound(pid));
        }

        let key = Self::composite_key(parent_id, trimmed_name);
        if self.db.tags_by_parent_name.get(&wtxn, &key)?.is_some() {
            return Err(LibraryError::DuplicateTag(trimmed_name.to_string()));
        }

        let tag_id = self.db.next_id(&mut wtxn, SEQ_TAG_ID)? as u32;
        let record = TagRecord {
            id: tag_id,
            parent_id,
            name: trimmed_name.to_string(),
        };

        self.db.tags.put(&mut wtxn, &tag_id, &record)?;
        self.db.tags_by_parent_name.put(&mut wtxn, &key, &tag_id)?;
        self.db.media_by_tag.put(&mut wtxn, &tag_id, &Vec::new())?;

        wtxn.commit()?;
        tracing::info!(tag_id = tag_id, name = %trimmed_name, parent_id = ?parent_id, "Created tag record in LMDB");
        Ok(record)
    }

    /// Renames an existing tag, updating its record and secondary name index.
    pub fn rename_tag(&self, tag_id: TagId, new_name: &str) -> Result<TagRecord, LibraryError> {
        let trimmed_name = new_name.trim();
        if trimmed_name.is_empty() {
            return Err(LibraryError::InvalidTagName(new_name.to_string()));
        }

        let mut wtxn = self.db.env.write_txn()?;

        let mut record = self
            .db
            .tags
            .get(&wtxn, &tag_id)?
            .ok_or(LibraryError::TagNotFound(tag_id))?;

        if record.name == trimmed_name {
            return Ok(record);
        }

        let new_key = Self::composite_key(record.parent_id, trimmed_name);
        if let Some(existing_id) = self.db.tags_by_parent_name.get(&wtxn, &new_key)?
            && existing_id != tag_id
        {
            return Err(LibraryError::DuplicateTag(trimmed_name.to_string()));
        }

        let old_key = Self::composite_key(record.parent_id, &record.name);
        self.db.tags_by_parent_name.delete(&mut wtxn, &old_key)?;
        self.db
            .tags_by_parent_name
            .put(&mut wtxn, &new_key, &tag_id)?;

        record.name = trimmed_name.to_string();
        self.db.tags.put(&mut wtxn, &tag_id, &record)?;

        wtxn.commit()?;
        tracing::info!(tag_id = tag_id, new_name = %trimmed_name, "Renamed tag in LMDB");
        Ok(record)
    }

    /// Moves a tag (and its entire subtree) under a new parent or to root, preventing cyclic references.
    pub fn move_tag(
        &self,
        tag_id: TagId,
        new_parent_id: Option<TagId>,
    ) -> Result<TagRecord, LibraryError> {
        if new_parent_id == Some(tag_id) {
            return Err(LibraryError::InvalidTagHierarchy);
        }

        let mut wtxn = self.db.env.write_txn()?;

        let mut record = self
            .db
            .tags
            .get(&wtxn, &tag_id)?
            .ok_or(LibraryError::TagNotFound(tag_id))?;

        if record.parent_id == new_parent_id {
            return Ok(record);
        }

        if let Some(target_parent) = new_parent_id {
            if self.db.tags.get(&wtxn, &target_parent)?.is_none() {
                return Err(LibraryError::TagNotFound(target_parent));
            }

            // Cycle prevention: verify target_parent is not a descendant of tag_id
            let mut curr = Some(target_parent);
            while let Some(pid) = curr {
                if pid == tag_id {
                    return Err(LibraryError::InvalidTagHierarchy);
                }
                curr = self.db.tags.get(&wtxn, &pid)?.and_then(|t| t.parent_id);
            }
        }

        let new_key = Self::composite_key(new_parent_id, &record.name);
        if let Some(existing_id) = self.db.tags_by_parent_name.get(&wtxn, &new_key)?
            && existing_id != tag_id
        {
            return Err(LibraryError::DuplicateTag(record.name.clone()));
        }

        let old_key = Self::composite_key(record.parent_id, &record.name);
        self.db.tags_by_parent_name.delete(&mut wtxn, &old_key)?;
        self.db
            .tags_by_parent_name
            .put(&mut wtxn, &new_key, &tag_id)?;

        record.parent_id = new_parent_id;
        self.db.tags.put(&mut wtxn, &tag_id, &record)?;

        wtxn.commit()?;
        tracing::info!(tag_id = tag_id, new_parent = ?new_parent_id, "Moved tag record in LMDB");
        Ok(record)
    }

    /// Recursively retrieves all descendants (children, grandchildren, etc.) of the specified tag.
    pub fn get_descendants(&self, root_tag_id: TagId) -> Result<Vec<TagRecord>, LibraryError> {
        let all_tags = self.list_all_tags()?;
        let mut descendants = Vec::new();
        let mut parent_set = HashSet::new();
        parent_set.insert(root_tag_id);

        let mut added = true;
        while added {
            added = false;
            for tag in &all_tags {
                if let Some(pid) = tag.parent_id
                    && parent_set.contains(&pid)
                    && !parent_set.contains(&tag.id)
                {
                    parent_set.insert(tag.id);
                    descendants.push(tag.clone());
                    added = true;
                }
            }
        }

        Ok(descendants)
    }

    /// Recursively deletes a tag and all its children/descendants from LMDB,
    /// cleaning up media associations across all indices in a single atomic transaction.
    pub fn delete_tag_subtree(&self, root_tag_id: TagId) -> Result<Vec<TagRecord>, LibraryError> {
        let descendants = self.get_descendants(root_tag_id)?;
        let mut wtxn = self.db.env.write_txn()?;

        let root_record = self
            .db
            .tags
            .get(&wtxn, &root_tag_id)?
            .ok_or(LibraryError::TagNotFound(root_tag_id))?;

        let mut to_delete = Vec::with_capacity(descendants.len() + 1);
        to_delete.push(root_record);
        to_delete.extend(descendants);

        for tag in &to_delete {
            let key = Self::composite_key(tag.parent_id, &tag.name);
            self.db.tags_by_parent_name.delete(&mut wtxn, &key)?;
            self.db.tags.delete(&mut wtxn, &tag.id)?;

            if let Some(media_ids) = self.db.media_by_tag.get(&wtxn, &tag.id)? {
                for media_id in media_ids {
                    if let Some(mut media_tags) = self.db.tags_by_media.get(&wtxn, &media_id)? {
                        media_tags.retain(|&t| t != tag.id);
                        if media_tags.is_empty() {
                            self.db.tags_by_media.delete(&mut wtxn, &media_id)?;
                        } else {
                            self.db
                                .tags_by_media
                                .put(&mut wtxn, &media_id, &media_tags)?;
                        }
                    }
                }
            }
            self.db.media_by_tag.delete(&mut wtxn, &tag.id)?;
        }

        wtxn.commit()?;
        tracing::info!(
            root_id = root_tag_id,
            deleted_count = to_delete.len(),
            "Atomically purged tag subtree and updated media associations in LMDB"
        );

        Ok(to_delete)
    }

    pub fn get_tag(&self, tag_id: TagId) -> Result<Option<TagRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        Ok(self.db.tags.get(&rtxn, &tag_id)?)
    }

    pub fn assign_tag(&self, media_id: MediaId, tag_id: TagId) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        if self.db.media.get(&wtxn, &media_id)?.is_none() {
            return Err(LibraryError::MediaNotFound(media_id));
        }
        if self.db.tags.get(&wtxn, &tag_id)?.is_none() {
            return Err(LibraryError::TagNotFound(tag_id));
        }

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
            if media_tags.is_empty() {
                self.db.tags_by_media.delete(&mut wtxn, &media_id)?;
            } else {
                self.db
                    .tags_by_media
                    .put(&mut wtxn, &media_id, &media_tags)?;
            }
        }

        if let Some(mut tag_media) = self.db.media_by_tag.get(&wtxn, &tag_id)? {
            tag_media.retain(|&id| id != media_id);
            if tag_media.is_empty() {
                self.db.media_by_tag.delete(&mut wtxn, &tag_id)?;
            } else {
                self.db.media_by_tag.put(&mut wtxn, &tag_id, &tag_media)?;
            }
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

    /// Computes the frequency count of each tag across a given set/slice of media items in a single read transaction.
    pub fn get_tag_frequencies(
        &self,
        media_ids: &[MediaId],
    ) -> Result<HashMap<TagId, usize>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut counts = HashMap::new();

        for &media_id in media_ids {
            if let Some(tags) = self.db.tags_by_media.get(&rtxn, &media_id)? {
                for tag_id in tags {
                    *counts.entry(tag_id).or_insert(0) += 1;
                }
            }
        }

        Ok(counts)
    }
}
