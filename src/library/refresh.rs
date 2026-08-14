use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, MediaId, MediaRecord};
use super::schema::SEQ_MEDIA_ID;
use crate::media::{DirectoryScanner, MediaHasher, MetadataExtractor};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Default)]
pub struct RefreshReport {
    pub total_scanned: usize,
    pub new_media: usize,
    pub updated_media: usize,
    pub unchanged_media: usize,
    pub deleted_media: usize,
}

pub struct FolderRefresher<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderRefresher<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Performs an incremental refresh on the target folder:
    /// 1. Short-circuits unchanged items by size and mtime without recalculating hashes.
    /// 2. Hashes and extracts metadata for new or changed files.
    /// 3. Detects and purges deleted files using generation markers.
    /// 4. Executes all mutations in optimized batched transactions.
    pub fn refresh_folder(&self, folder_id: FolderId) -> Result<RefreshReport, LibraryError> {
        let mut report = RefreshReport::default();

        let folder_record = {
            let rtxn = self.db.env.read_txn()?;
            self.db
                .folders
                .get(&rtxn, &folder_id)?
                .ok_or(LibraryError::FolderNotFound(folder_id))?
        };

        let folder_path = Path::new(&folder_record.path);
        let scanned_files = DirectoryScanner::scan(folder_path, false);
        report.total_scanned = scanned_files.len();

        let mut wtxn = self.db.env.write_txn()?;
        let current_generation = self.db.next_generation(&mut wtxn)?;

        // Build index of existing items in the folder: filename -> (MediaId, MediaRecord)
        let existing_ids = self
            .db
            .media_by_folder
            .get(&wtxn, &folder_id)?
            .unwrap_or_default();

        let mut existing_map: HashMap<String, (MediaId, MediaRecord)> =
            HashMap::with_capacity(existing_ids.len());
        for id in existing_ids {
            if let Some(rec) = self.db.media.get(&wtxn, &id)? {
                existing_map.insert(rec.filename.clone(), (id, rec));
            }
        }

        let mut active_media_ids = Vec::with_capacity(scanned_files.len());

        for scanned in scanned_files {
            let fs_meta = match std::fs::metadata(&scanned.path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let file_size = fs_meta.len();
            let mtime = fs_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            if let Some((existing_id, mut existing_record)) = existing_map.remove(&scanned.filename)
            {
                // Check if unchanged (Fast path: no hash recalculation)
                if existing_record.file_size == file_size
                    && existing_record.modification_time == mtime
                {
                    existing_record.last_seen = current_generation;
                    self.db
                        .media
                        .put(&mut wtxn, &existing_id, &existing_record)?;
                    active_media_ids.push(existing_id);
                    report.unchanged_media += 1;
                    continue;
                }

                // File modified: Compute BLAKE3 hash
                let new_hash = MediaHasher::hash_file(&scanned.path)?;
                if new_hash == existing_record.file_hash {
                    // Content identical, update file timestamps only
                    existing_record.modification_time = mtime;
                    existing_record.file_size = file_size;
                    existing_record.last_seen = current_generation;
                    self.db
                        .media
                        .put(&mut wtxn, &existing_id, &existing_record)?;
                    active_media_ids.push(existing_id);
                    report.unchanged_media += 1;
                } else {
                    // Content changed: Re-extract metadata and re-index hash
                    let raw_meta = match MetadataExtractor::extract(&scanned.path) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    // Remove old hash index
                    if let Some(mut hash_list) = self
                        .db
                        .media_by_hash
                        .get(&wtxn, &existing_record.file_hash[..])?
                    {
                        hash_list.retain(|&id| id != existing_id);
                        if hash_list.is_empty() {
                            self.db
                                .media_by_hash
                                .delete(&mut wtxn, &existing_record.file_hash[..])?;
                        } else {
                            self.db.media_by_hash.put(
                                &mut wtxn,
                                &existing_record.file_hash[..],
                                &hash_list,
                            )?;
                        }
                    }

                    existing_record.file_hash = new_hash;
                    existing_record.file_size = file_size;
                    existing_record.modification_time = mtime;
                    existing_record.width = raw_meta.width;
                    existing_record.height = raw_meta.height;
                    existing_record.format = raw_meta.format;
                    existing_record.media_type = raw_meta.media_type;
                    existing_record.last_seen = current_generation;

                    self.db
                        .media
                        .put(&mut wtxn, &existing_id, &existing_record)?;

                    // Insert new hash index
                    let mut hash_list = self
                        .db
                        .media_by_hash
                        .get(&wtxn, &new_hash[..])?
                        .unwrap_or_default();
                    if !hash_list.contains(&existing_id) {
                        hash_list.push(existing_id);
                        self.db
                            .media_by_hash
                            .put(&mut wtxn, &new_hash[..], &hash_list)?;
                    }

                    active_media_ids.push(existing_id);
                    report.updated_media += 1;
                }
            } else {
                // New file
                let new_hash = match MediaHasher::hash_file(&scanned.path) {
                    Ok(h) => h,
                    Err(_) => continue,
                };

                let raw_meta = match MetadataExtractor::extract(&scanned.path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let new_media_id = self.db.next_id(&mut wtxn, SEQ_MEDIA_ID)?;
                let new_record = MediaRecord {
                    id: new_media_id,
                    folder_id,
                    filename: scanned.filename,
                    file_hash: new_hash,
                    file_size,
                    modification_time: mtime,
                    media_type: raw_meta.media_type,
                    format: raw_meta.format,
                    width: raw_meta.width,
                    height: raw_meta.height,
                    rating: 0,
                    last_seen: current_generation,
                };

                self.db.media.put(&mut wtxn, &new_media_id, &new_record)?;

                let mut hash_list = self
                    .db
                    .media_by_hash
                    .get(&wtxn, &new_hash[..])?
                    .unwrap_or_default();
                hash_list.push(new_media_id);
                self.db
                    .media_by_hash
                    .put(&mut wtxn, &new_hash[..], &hash_list)?;

                active_media_ids.push(new_media_id);
                report.new_media += 1;
            }
        }

        // Remaining elements in `existing_map` are no longer present on the filesystem -> Purge
        for (deleted_id, deleted_record) in existing_map.values() {
            if let Some(mut hash_list) = self
                .db
                .media_by_hash
                .get(&wtxn, &deleted_record.file_hash[..])?
            {
                hash_list.retain(|id| id != deleted_id);
                if hash_list.is_empty() {
                    self.db
                        .media_by_hash
                        .delete(&mut wtxn, &deleted_record.file_hash[..])?;
                } else {
                    self.db.media_by_hash.put(
                        &mut wtxn,
                        &deleted_record.file_hash[..],
                        &hash_list,
                    )?;
                }
            }
            self.db.media.delete(&mut wtxn, deleted_id)?;
            self.db.tags_by_media.delete(&mut wtxn, deleted_id)?;
            report.deleted_media += 1;
        }

        // Update folder contents index
        self.db
            .media_by_folder
            .put(&mut wtxn, &folder_id, &active_media_ids)?;

        wtxn.commit()?;
        Ok(report)
    }
}
