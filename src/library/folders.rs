use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, FolderRecord};
use super::schema::SEQ_FOLDER_ID;
use std::path::{Path, PathBuf};

pub struct FolderManager<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderManager<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    pub fn add_folder(&self, path: &Path) -> Result<FolderRecord, LibraryError> {
        let canonical_path = fs_err_canonicalize(path)?;
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
        let canonical_path = fs_err_canonicalize(path)?;
        let path_str = canonical_path.to_string_lossy().to_string();

        let rtxn = self.db.env.read_txn()?;
        if let Some(id) = self.db.folders_by_path.get(&rtxn, &path_str)? {
            Ok(self.db.folders.get(&rtxn, &id)?)
        } else {
            Ok(None)
        }
    }

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
        let count = self
            .db
            .media_by_folder
            .get(&rtxn, &folder_id)?
            .map(|v| v.len())
            .unwrap_or(0);
        Ok(count)
    }

    pub fn remove_folder(&self, id: FolderId) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        if let Some(folder) = self.db.folders.get(&wtxn, &id)? {
            // Delete media associations
            if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, &id)? {
                for media_id in media_ids {
                    if let Some(media_record) = self.db.media.get(&wtxn, &media_id)? {
                        // Unset hash index
                        if let Some(mut hash_media_ids) = self
                            .db
                            .media_by_hash
                            .get(&wtxn, &media_record.file_hash[..])?
                        {
                            hash_media_ids.retain(|&m_id| m_id != media_id);
                            if hash_media_ids.is_empty() {
                                self.db
                                    .media_by_hash
                                    .delete(&mut wtxn, &media_record.file_hash[..])?;
                            } else {
                                self.db.media_by_hash.put(
                                    &mut wtxn,
                                    &media_record.file_hash[..],
                                    &hash_media_ids,
                                )?;
                            }
                        }
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

fn fs_err_canonicalize(path: &Path) -> Result<PathBuf, std::io::Error> {
    match path.canonicalize() {
        Ok(p) => Ok(p),
        Err(_) => Ok(path.to_path_buf()),
    }
}
