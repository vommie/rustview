use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, FolderRecord};
use super::schema::SEQ_FOLDER_ID;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Normalizes a filesystem path by resolving symlinks/canonical form and removing any trailing slash.
pub fn normalize_path(path: &Path) -> PathBuf {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let mut normalized = PathBuf::new();
            for comp in path.components() {
                match comp {
                    std::path::Component::CurDir => {}
                    _ => normalized.push(comp.as_os_str()),
                }
            }
            normalized
        }
    };
    let s = canonical.to_string_lossy();
    if s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) {
        PathBuf::from(s.trim_end_matches(['/', '\\']))
    } else {
        canonical
    }
}

pub struct FolderManager<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderManager<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    pub fn add_folder(&self, path: &Path) -> Result<FolderRecord, LibraryError> {
        let canonical_path = normalize_path(path);
        let path_str = canonical_path.to_string_lossy().to_string();

        let mut wtxn = self.db.env.write_txn()?;

        if let Some(existing_id) = self.db.folders_by_path.get(&wtxn, &path_str)? {
            if let Some(mut record) = self.db.folders.get(&wtxn, &existing_id)?
                && record.is_deleted
            {
                record.is_deleted = false;
                self.db.folders.put(&mut wtxn, &existing_id, &record)?;
                wtxn.commit()?;
                return Ok(record);
            }
            return Err(LibraryError::DuplicateFolder(path_str));
        }

        let folder_id = self.db.next_id(&mut wtxn, SEQ_FOLDER_ID)?;
        let record = FolderRecord {
            id: folder_id,
            path: path_str.clone(),
            added_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            is_deleted: false,
        };

        self.db.folders.put(&mut wtxn, &folder_id, &record)?;
        self.db
            .folders_by_path
            .put(&mut wtxn, &path_str, &folder_id)?;
        self.db
            .media_by_folder
            .put(&mut wtxn, &folder_id, &Vec::new())?;

        wtxn.commit()?;
        Ok(record)
    }

    pub fn get_folder(&self, id: FolderId) -> Result<Option<FolderRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        Ok(self.db.folders.get(&rtxn, &id)?)
    }

    pub fn get_folder_by_path(&self, path: &Path) -> Result<Option<FolderRecord>, LibraryError> {
        let canonical_path = normalize_path(path);
        let path_str = canonical_path.to_string_lossy().to_string();

        let rtxn = self.db.env.read_txn()?;
        if let Some(id) = self.db.folders_by_path.get(&rtxn, &path_str)? {
            Ok(self.db.folders.get(&rtxn, &id)?)
        } else {
            Ok(None)
        }
    }

    /// Ultra-fast sequential streaming of all folder records from LMDB.
    pub fn list_folders(&self) -> Result<Vec<FolderRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut folders = Vec::new();

        for entry in self.db.folders.iter(&rtxn)? {
            let (_, record) = entry?;
            folders.push(record);
        }

        Ok(folders)
    }

    pub fn list_active_folders(&self) -> Result<Vec<FolderRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut folders = Vec::new();

        for entry in self.db.folders.iter(&rtxn)? {
            let (_, record) = entry?;
            if !record.is_deleted {
                folders.push(record);
            }
        }

        Ok(folders)
    }

    pub fn set_deleted(&self, id: FolderId, is_deleted: bool) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;
        if let Some(mut record) = self.db.folders.get(&wtxn, &id)? {
            if record.is_deleted != is_deleted {
                record.is_deleted = is_deleted;
                self.db.folders.put(&mut wtxn, &id, &record)?;
            }
        } else {
            return Err(LibraryError::FolderNotFound(id));
        }
        wtxn.commit()?;
        Ok(())
    }

    pub fn get_media_count(&self, folder_id: FolderId) -> Result<usize, LibraryError> {
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

    /// Retrieves active media file counts for ALL folders in a single ultra-fast read transaction.
    pub fn get_all_media_counts(&self) -> Result<HashMap<FolderId, usize>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut counts = HashMap::new();

        for entry in self.db.media_by_folder.iter(&rtxn)? {
            let (folder_id, media_ids) = entry?;
            let mut count = 0;
            for id in media_ids {
                if let Some(rec) = self.db.media.get(&rtxn, &id)?
                    && !rec.is_deleted
                {
                    count += 1;
                }
            }
            counts.insert(folder_id, count);
        }

        Ok(counts)
    }

    pub fn remove_folder(&self, id: FolderId) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        if let Some(folder) = self.db.folders.get(&wtxn, &id)? {
            // Delete media associations
            if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, &id)? {
                for media_id in media_ids {
                    if let Some(media_record) = self.db.media.get(&wtxn, &media_id)? {
                        self.db.update_media_hash_index(
                            &mut wtxn,
                            media_id,
                            Some(&media_record.file_hash),
                            None,
                        )?;
                    }
                    self.db.media.delete(&mut wtxn, &media_id)?;
                    self.db.tags_by_media.delete(&mut wtxn, &media_id)?;
                }
            }

            self.db.media_by_folder.delete(&mut wtxn, &id)?;
            self.db.folders_by_path.delete(&mut wtxn, &folder.path)?;
            self.db.folders.delete(&mut wtxn, &id)?;
        }

        wtxn.commit()?;
        Ok(())
    }
}
