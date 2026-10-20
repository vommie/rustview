use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, MediaId, MediaRecord, MediaSearchParams, TagId};
use crate::media::{FileHash, FormatPolicy, MediaFormat};
use crate::query::{MatchMode, ScopeMode};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

    /// Resolves the absolute physical filesystem path of a media file via its registered folder_id in LMDB.
    pub fn resolve_path(&self, id: MediaId) -> Result<PathBuf, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let rec = self
            .db
            .media
            .get(&rtxn, &id)?
            .ok_or(LibraryError::MediaNotFound(id))?;
        let folder = self
            .db
            .folders
            .get(&rtxn, &rec.folder_id)?
            .ok_or(LibraryError::FolderNotFound(rec.folder_id))?;
        Ok(Path::new(&folder.path).join(&rec.filename))
    }

    /// Retrieves all active (non-deleted) media records for the specified folder with ID deduplication.
    pub fn get_by_folder(&self, folder_id: FolderId) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &folder_id)?
            .unwrap_or_default();
        let mask = FormatPolicy::mask();
        let mut records = Vec::with_capacity(media_ids.len());
        let mut seen = HashSet::new();

        for id in media_ids {
            if seen.insert(id)
                && let Some(record) = self.db.media.get(&rtxn, &id)?
                && !record.is_deleted
                && mask.shows(record.format)
            {
                records.push(record);
            }
        }

        Ok(records)
    }

    /// Retrieves all media records (including soft-deleted/missing) for the specified folder with ID deduplication.
    pub fn get_all_by_folder(&self, folder_id: FolderId) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &folder_id)?
            .unwrap_or_default();
        let mut records = Vec::with_capacity(media_ids.len());
        let mut seen = HashSet::new();

        for id in media_ids {
            if seen.insert(id)
                && let Some(record) = self.db.media.get(&rtxn, &id)?
            {
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
        let mask = FormatPolicy::mask();
        let mut count = 0;
        let mut seen = HashSet::new();

        for id in media_ids {
            if seen.insert(id)
                && let Some(rec) = self.db.media.get(&rtxn, &id)?
                && !rec.is_deleted
                && mask.shows(rec.format)
            {
                count += 1;
            }
        }
        Ok(count)
    }

    /// How many active (not missing) records each format owns, switched-off formats included: the numbers shown next to
    /// the switches of Settings > File Formats. One pass over the media table; call it from a worker thread.
    pub fn count_by_format(&self) -> Result<HashMap<MediaFormat, u64>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut counts: HashMap<MediaFormat, u64> = HashMap::new();
        for entry in self.db.media.iter(&rtxn)? {
            let (_, record) = entry?;
            if !record.is_deleted {
                *counts.entry(record.format).or_default() += 1;
            }
        }
        Ok(counts)
    }

    pub fn get_by_hash(&self, hash: &FileHash) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let media_ids = self
            .db
            .media_by_hash
            .get(&rtxn, &hash[..])?
            .unwrap_or_default();
        let mut records = Vec::with_capacity(media_ids.len());
        let mut seen = HashSet::new();

        for id in media_ids {
            if seen.insert(id)
                && let Some(record) = self.db.media.get(&rtxn, &id)?
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

    pub fn set_corrupted(&self, media_id: MediaId, is_corrupted: bool) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;
        if let Some(mut record) = self.db.media.get(&wtxn, &media_id)? {
            if record.is_corrupted != is_corrupted {
                record.is_corrupted = is_corrupted;
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

    /// Performs a high-speed database search evaluating combinatorial tags, ratings, and hierarchical scopes.
    /// Preserves and finds both active and soft-deleted records matching the requested tags, ratings, and media type filter.
    pub fn search(&self, params: &MediaSearchParams) -> Result<Vec<MediaRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;

        let is_rating_active = match params.rating {
            Some(r) => r > 0,
            None => false,
        };
        let target_rating = params.rating.unwrap_or(0);
        let has_tags = !params.tag_ids.is_empty();

        if !is_rating_active && !has_tags {
            if params.scope_mode == ScopeMode::Folder
                && let Some(f_id) = params.folder_id
            {
                let mask = FormatPolicy::mask();
                return Ok(self
                    .get_all_by_folder(f_id)?
                    .into_iter()
                    .filter(|m| mask.shows(m.format) && params.media_type_filter.matches(m.media_type))
                    .collect());
            }
            return Ok(Vec::new());
        }

        let mut candidate_ids: HashSet<MediaId> = HashSet::new();

        match params.scope_mode {
            ScopeMode::Folder => {
                let Some(f_id) = params.folder_id else {
                    return Ok(Vec::new());
                };
                let folder_media = self
                    .db
                    .media_by_folder
                    .get(&rtxn, &f_id)?
                    .unwrap_or_default();
                candidate_ids.extend(folder_media);
            }
            ScopeMode::FolderAndSubfolders => {
                let Some(f_id) = params.folder_id else {
                    return Ok(Vec::new());
                };
                if let Some(folder_rec) = self.db.folders.get(&rtxn, &f_id)?
                    && !folder_rec.is_deleted
                {
                    let root_path = std::path::PathBuf::from(&folder_rec.path);
                    let mut target_folder_ids = vec![f_id];

                    for entry in self.db.folders.iter(&rtxn)? {
                        let (id, rec) = entry?;
                        if id != f_id && !rec.is_deleted && Path::new(&rec.path).starts_with(&root_path) {
                            target_folder_ids.push(id);
                        }
                    }

                    for fid in target_folder_ids {
                        if let Some(media_list) = self.db.media_by_folder.get(&rtxn, &fid)? {
                            candidate_ids.extend(media_list);
                        }
                    }
                }
            }
            ScopeMode::Global => {
                if params.match_mode == MatchMode::All && has_tags {
                    let mut iter = params.tag_ids.iter();
                    if let Some(&first_tag) = iter.next() {
                        let first_set: HashSet<MediaId> = self
                            .db
                            .media_by_tag
                            .get(&rtxn, &first_tag)?
                            .unwrap_or_default()
                            .into_iter()
                            .collect();
                        let mut intersected = first_set;
                        for &next_tag in iter {
                            let tag_set: HashSet<MediaId> = self
                                .db
                                .media_by_tag
                                .get(&rtxn, &next_tag)?
                                .unwrap_or_default()
                                .into_iter()
                                .collect();
                            intersected.retain(|id| tag_set.contains(id));
                        }
                        candidate_ids = intersected;
                    }
                } else if params.match_mode == MatchMode::Any && has_tags {
                    for &tag_id in &params.tag_ids {
                        if let Some(ids) = self.db.media_by_tag.get(&rtxn, &tag_id)? {
                            candidate_ids.extend(ids);
                        }
                    }
                    if is_rating_active {
                        for entry in self.db.media.iter(&rtxn)? {
                            let (id, rec) = entry?;
                            if rec.rating == target_rating {
                                candidate_ids.insert(id);
                            }
                        }
                    }
                } else {
                    for entry in self.db.media.iter(&rtxn)? {
                        let (id, rec) = entry?;
                        if rec.rating == target_rating {
                            candidate_ids.insert(id);
                        }
                    }
                }
            }
        }

        let mask = FormatPolicy::mask();
        let mut results = Vec::new();
        let mut folder_active_cache: HashMap<FolderId, bool> = HashMap::new();

        for media_id in candidate_ids {
            let Some(rec) = self.db.media.get(&rtxn, &media_id)? else {
                continue;
            };

            // Fast Media Type Filter Check (and the formats the user switched off)
            if !mask.shows(rec.format) || !params.media_type_filter.matches(rec.media_type) {
                continue;
            }

            let is_folder_active = *folder_active_cache.entry(rec.folder_id).or_insert_with(|| {
                self.db
                    .folders
                    .get(&rtxn, &rec.folder_id)
                    .ok()
                    .flatten()
                    .map(|f| !f.is_deleted)
                    .unwrap_or(false)
            });

            if !is_folder_active {
                continue;
            }

            let rating_matches = if is_rating_active {
                rec.rating == target_rating
            } else {
                true
            };

            let item_tag_ids: HashSet<TagId> = self
                .db
                .tags_by_media
                .get(&rtxn, &media_id)?
                .unwrap_or_default()
                .into_iter()
                .collect();

            let tag_matches = if has_tags {
                match params.match_mode {
                    MatchMode::All => params.tag_ids.is_subset(&item_tag_ids),
                    MatchMode::Any => !params.tag_ids.is_disjoint(&item_tag_ids),
                }
            } else {
                true
            };

            let is_match = match params.match_mode {
                MatchMode::All => {
                    if is_rating_active && has_tags {
                        rating_matches && tag_matches
                    } else if is_rating_active {
                        rating_matches
                    } else if has_tags {
                        tag_matches
                    } else {
                        true
                    }
                }
                MatchMode::Any => {
                    if is_rating_active && has_tags {
                        rating_matches || tag_matches
                    } else if is_rating_active {
                        rating_matches
                    } else if has_tags {
                        tag_matches
                    } else {
                        true
                    }
                }
            };

            if is_match {
                results.push(rec);
            }
        }

        Ok(results)
    }
}
