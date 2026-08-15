use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, FolderRecord};
use std::path::{Path, PathBuf};

/// POSIX Linux folder component name validator.
pub struct FolderValidator;

impl FolderValidator {
    /// Validates directory name according to Linux POSIX naming rules.
    pub fn validate(name: &str) -> Result<String, LibraryError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Folder name cannot be empty".to_string(),
            ));
        }

        if trimmed == "." || trimmed == ".." {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Folder name cannot be '.' or '..'".to_string(),
            ));
        }

        if trimmed.contains('/') || trimmed.contains('\0') {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Folder name cannot contain '/' or NUL characters".to_string(),
            ));
        }

        if trimmed.len() > 255 {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Folder name cannot exceed 255 bytes (NAME_MAX limit)".to_string(),
            ));
        }

        if trimmed.chars().any(|c| (c as u32) < 32 || c == '\x7f') {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Folder name cannot contain control characters".to_string(),
            ));
        }

        Ok(trimmed.to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct FolderSubtreeMetrics {
    pub folder_id: FolderId,
    pub path: PathBuf,
    pub subfolders_count: usize,
    pub media_files_count: usize,
    pub affected_folder_ids: Vec<FolderId>,
}

#[derive(Debug, Clone, Default)]
pub struct FolderDeleteReport {
    pub deleted_folder_ids: Vec<FolderId>,
    pub deleted_media_count: usize,
    pub trashed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FolderMoveReport {
    pub moved_folder_id: FolderId,
    pub affected_folders_count: usize,
    pub affected_media_count: usize,
}

/// Subsystem managing atomic physical & database folder operations (Rename, Trash, Permanent Delete, Relocate).
pub struct FolderOperations<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderOperations<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Computes subtree metrics (subfolders and total media files) for confirmation dialogs.
    pub fn get_metrics(&self, folder_id: FolderId) -> Result<FolderSubtreeMetrics, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let folder = self
            .db
            .folders
            .get(&rtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let folder_path = PathBuf::from(&folder.path);
        let mut affected_folders = Vec::new();
        let mut total_media = 0;

        for entry in self.db.folders.iter(&rtxn)? {
            let (id, rec) = entry?;
            let rec_path = Path::new(&rec.path);
            if rec_path.starts_with(&folder_path) {
                affected_folders.push(id);
                if let Some(media_ids) = self.db.media_by_folder.get(&rtxn, &id)? {
                    total_media += media_ids.len();
                }
            }
        }

        let subfolders_count = affected_folders.len().saturating_sub(1);

        Ok(FolderSubtreeMetrics {
            folder_id,
            path: folder_path,
            subfolders_count,
            media_files_count: total_media,
            affected_folder_ids: affected_folders,
        })
    }

    /// Atomically renames a folder physically on disk and updates all LMDB path indices recursively.
    pub fn rename_folder(
        &self,
        folder_id: FolderId,
        new_name: &str,
    ) -> Result<FolderRecord, LibraryError> {
        let valid_name = FolderValidator::validate(new_name)?;

        let mut wtxn = self.db.env.write_txn()?;

        let mut record = self
            .db
            .folders
            .get(&wtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let old_path = PathBuf::from(&record.path);
        let parent_path = old_path.parent().ok_or_else(|| {
            LibraryError::FileOperationRenameFailed("Cannot rename root directory".into())
        })?;

        let new_path = parent_path.join(&valid_name);

        if old_path == new_path {
            return Ok(record);
        }

        if new_path.exists() {
            return Err(LibraryError::FileOperationAlreadyExists(valid_name));
        }

        let new_path_str = new_path.to_string_lossy().to_string();

        if self.db.folders_by_path.get(&wtxn, &new_path_str)?.is_some() {
            return Err(LibraryError::DuplicateFolder(new_path_str));
        }

        tracing::info!(
            folder_id = folder_id,
            old_path = ?old_path,
            new_path = ?new_path,
            "Executing physical folder rename operation..."
        );

        // 1. Filesystem rename
        std::fs::rename(&old_path, &new_path)?;

        // 2. Collect all descendant folders to rewrite paths in LMDB
        let mut affected = Vec::new();
        for entry in self.db.folders.iter(&wtxn)? {
            let (id, rec) = entry?;
            let rec_p = PathBuf::from(&rec.path);
            if rec_p.starts_with(&old_path) {
                affected.push((id, rec));
            }
        }

        for (id, mut rec) in affected {
            let old_sub_p = PathBuf::from(&rec.path);
            let suffix = old_sub_p.strip_prefix(&old_path).unwrap_or(Path::new(""));
            let updated_p = if suffix.as_os_str().is_empty() {
                new_path.clone()
            } else {
                new_path.join(suffix)
            };
            let updated_p_str = updated_p.to_string_lossy().to_string();

            self.db.folders_by_path.delete(&mut wtxn, &rec.path)?;
            self.db
                .folders_by_path
                .put(&mut wtxn, &updated_p_str, &id)?;

            rec.path = updated_p_str;
            self.db.folders.put(&mut wtxn, &id, &rec)?;

            if id == folder_id {
                record = rec;
            }
        }

        wtxn.commit()?;

        tracing::info!(
            folder_id = folder_id,
            new_path = %record.path,
            "Folder rename committed successfully to LMDB"
        );

        Ok(record)
    }

    /// Moves a folder and all its contents to the system Trash using FreeDesktop spec (or permanent delete).
    /// Completely purges the folder and its contents from the LMDB database.
    pub fn delete_folder(
        &self,
        folder_id: FolderId,
        to_trash: bool,
    ) -> Result<FolderDeleteReport, LibraryError> {
        let metrics = self.get_metrics(folder_id)?;
        let folder_path = metrics.path;

        tracing::info!(
            folder_id = folder_id,
            path = ?folder_path,
            to_trash = to_trash,
            subfolders = metrics.subfolders_count,
            files = metrics.media_files_count,
            "Deleting directory and contents..."
        );

        // 1. Physical Filesystem Operation
        if folder_path.exists() {
            if to_trash {
                trash::delete(&folder_path).map_err(|e| {
                    LibraryError::FileOperationRenameFailed(format!(
                        "Failed to move folder to Trash: {}",
                        e
                    ))
                })?;
            } else {
                std::fs::remove_dir_all(&folder_path)?;
            }
        }

        // 2. Atomic Database Cleanup (Purge from LMDB)
        let mut wtxn = self.db.env.write_txn()?;
        let mut deleted_media_count = 0;

        for id in &metrics.affected_folder_ids {
            if let Some(folder_rec) = self.db.folders.get(&wtxn, id)? {
                // Delete all media associated with folder
                if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, id)? {
                    deleted_media_count += media_ids.len();
                    for media_id in media_ids {
                        if let Some(media_record) = self.db.media.get(&wtxn, &media_id)?
                            && let Some(mut hash_list) = self
                                .db
                                .media_by_hash
                                .get(&wtxn, &media_record.file_hash[..])?
                        {
                            hash_list.retain(|&m| m != media_id);
                            if hash_list.is_empty() {
                                self.db
                                    .media_by_hash
                                    .delete(&mut wtxn, &media_record.file_hash[..])?;
                            } else {
                                self.db.media_by_hash.put(
                                    &mut wtxn,
                                    &media_record.file_hash[..],
                                    &hash_list,
                                )?;
                            }
                        }
                        self.db.media.delete(&mut wtxn, &media_id)?;
                        self.db.tags_by_media.delete(&mut wtxn, &media_id)?;
                        self.db.thumbnails.delete(&mut wtxn, &media_id)?;
                    }
                }

                self.db.media_by_folder.delete(&mut wtxn, id)?;
                self.db
                    .folders_by_path
                    .delete(&mut wtxn, &folder_rec.path)?;
                self.db.folders.delete(&mut wtxn, id)?;
            }
        }

        wtxn.commit()?;

        tracing::info!(
            folder_id = folder_id,
            deleted_folders = metrics.affected_folder_ids.len(),
            deleted_media = deleted_media_count,
            "Committed full folder deletion from LMDB"
        );

        Ok(FolderDeleteReport {
            deleted_folder_ids: metrics.affected_folder_ids,
            deleted_media_count,
            trashed: to_trash,
        })
    }

    /// Moves a folder physically under a new parent directory and updates LMDB path indices recursively.
    pub fn move_folder(
        &self,
        folder_id: FolderId,
        new_parent_id: FolderId,
    ) -> Result<FolderMoveReport, LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        let record = self
            .db
            .folders
            .get(&wtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let parent_record = self
            .db
            .folders
            .get(&wtxn, &new_parent_id)?
            .ok_or(LibraryError::FolderNotFound(new_parent_id))?;

        let src_path = PathBuf::from(&record.path);
        let dst_parent_path = PathBuf::from(&parent_record.path);

        let folder_name = src_path.file_name().ok_or_else(|| {
            LibraryError::FileOperationRenameFailed("Invalid source folder name".into())
        })?;

        let dst_path = dst_parent_path.join(folder_name);

        // Cycle check: Cannot move folder into itself or into one of its descendants
        if dst_parent_path.starts_with(&src_path) || src_path == dst_path {
            return Err(LibraryError::FileOperationRenameFailed(
                "Cannot move a folder into itself or its own subdirectories".into(),
            ));
        }

        if dst_path.exists() {
            return Err(LibraryError::FileOperationAlreadyExists(
                folder_name.to_string_lossy().to_string(),
            ));
        }

        let dst_path_str = dst_path.to_string_lossy().to_string();
        if self.db.folders_by_path.get(&wtxn, &dst_path_str)?.is_some() {
            return Err(LibraryError::DuplicateFolder(dst_path_str));
        }

        tracing::info!(
            folder_id = folder_id,
            src = ?src_path,
            dst = ?dst_path,
            "Relocating folder directory on physical filesystem..."
        );

        // 1. Filesystem move
        std::fs::rename(&src_path, &dst_path)?;

        // 2. Collect affected descendants
        let mut affected = Vec::new();
        for entry in self.db.folders.iter(&wtxn)? {
            let (id, rec) = entry?;
            let rec_p = PathBuf::from(&rec.path);
            if rec_p.starts_with(&src_path) {
                affected.push((id, rec));
            }
        }

        let mut affected_media_count = 0;

        for (id, mut rec) in affected {
            let old_sub_p = PathBuf::from(&rec.path);
            let suffix = old_sub_p.strip_prefix(&src_path).unwrap_or(Path::new(""));
            let updated_p = if suffix.as_os_str().is_empty() {
                dst_path.clone()
            } else {
                dst_path.join(suffix)
            };
            let updated_p_str = updated_p.to_string_lossy().to_string();

            self.db.folders_by_path.delete(&mut wtxn, &rec.path)?;
            self.db
                .folders_by_path
                .put(&mut wtxn, &updated_p_str, &id)?;

            rec.path = updated_p_str;
            self.db.folders.put(&mut wtxn, &id, &rec)?;

            if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, &id)? {
                affected_media_count += media_ids.len();
            }
        }

        wtxn.commit()?;

        tracing::info!(
            folder_id = folder_id,
            new_path = ?dst_path,
            "Committed folder relocation to LMDB"
        );

        Ok(FolderMoveReport {
            moved_folder_id: folder_id,
            affected_folders_count: 1,
            affected_media_count,
        })
    }
}
