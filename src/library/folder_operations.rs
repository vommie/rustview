use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, FolderRecord};
use crate::settings::LogObscurer;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// POSIX Linux folder component name validator.
pub struct FolderValidator;

impl FolderValidator {
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

        if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Folder name cannot contain path separators or NUL characters".to_string(),
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

/// Subsystem managing atomic physical & database folder operations.
pub struct FolderOperations<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderOperations<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Computes subtree metrics using relational parent indices.
    pub fn get_metrics(&self, folder_id: FolderId) -> Result<FolderSubtreeMetrics, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let folder = self
            .db
            .folders
            .get(&rtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let folder_path = PathBuf::from(&folder.path);
        let mut affected_folders = vec![folder_id];
        let mut queue = vec![folder_id];

        while let Some(current) = queue.pop() {
            if let Some(children) = self.db.folders_by_parent.get(&rtxn, &current)? {
                for cid in children {
                    affected_folders.push(cid);
                    queue.push(cid);
                }
            }
        }

        let mut total_media = 0;
        for &id in &affected_folders {
            if let Some(media_ids) = self.db.media_by_folder.get(&rtxn, &id)? {
                for media_id in media_ids {
                    if let Some(media_rec) = self.db.media.get(&rtxn, &media_id)?
                        && !media_rec.is_deleted
                    {
                        total_media += 1;
                    }
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

    pub fn check_rename_conflict(
        &self,
        folder_id: FolderId,
        new_name: &str,
    ) -> Result<Option<PathBuf>, LibraryError> {
        let valid_name = FolderValidator::validate(new_name)?;

        let rtxn = self.db.env.read_txn()?;
        let record = self
            .db
            .folders
            .get(&rtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let old_path = PathBuf::from(&record.path);
        let parent_path = match old_path.parent() {
            Some(p) => p,
            None => return Ok(None),
        };

        let new_path = parent_path.join(&valid_name);

        if old_path == new_path {
            return Ok(None);
        }

        let new_path_str = new_path.to_string_lossy().to_string();
        let exists_in_db = self.db.folders_by_path.get(&rtxn, &new_path_str)?.is_some();

        if new_path.exists() || exists_in_db {
            Ok(Some(new_path))
        } else {
            Ok(None)
        }
    }

    pub fn generate_unique_folder_name(parent_path: &Path, original_name: &str) -> String {
        let trimmed = original_name.trim();
        let mut counter = 1u32;
        loop {
            let candidate = format!("{} ({})", trimmed, counter);
            if !parent_path.join(&candidate).exists() {
                return candidate;
            }
            counter += 1;
            if counter > 100_000 {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                return format!("{}_{}", trimmed, timestamp);
            }
        }
    }

    /// Atomically renames a folder physically and updates all descendant path indices in LMDB.
    pub fn rename_folder(
        &self,
        folder_id: FolderId,
        new_name: &str,
    ) -> Result<FolderRecord, LibraryError> {
        let t0 = Instant::now();
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
            folder = %LogObscurer::folder(Some(folder_id), &old_path),
            new_path = %LogObscurer::path(&new_path),
            "[FolderOperations: Rename] Executing physical folder rename operation..."
        );

        std::fs::rename(&old_path, &new_path)?;

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
            if id == folder_id {
                rec.name = valid_name.clone();
                record = rec.clone();
            }
            self.db.folders.put(&mut wtxn, &id, &rec)?;
        }

        wtxn.commit()?;

        tracing::info!(
            folder = %LogObscurer::folder(Some(folder_id), &record.path),
            elapsed_us = t0.elapsed().as_micros(),
            "[FolderOperations: Rename] Folder rename committed successfully to LMDB"
        );

        Ok(record)
    }

    pub fn delete_folder(
        &self,
        folder_id: FolderId,
        to_trash: bool,
    ) -> Result<FolderDeleteReport, LibraryError> {
        let metrics = self.get_metrics(folder_id)?;
        let folder_path = metrics.path;

        tracing::info!(
            folder_id,
            path = %LogObscurer::path(&folder_path),
            to_trash,
            subfolders = metrics.subfolders_count,
            media = metrics.media_files_count,
            "[FolderOperations: Delete] Deleting folder and cleaning records..."
        );

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

        let mut wtxn = self.db.env.write_txn()?;
        let mut deleted_media_count = 0;

        // Unlink from parent's children list
        if let Some(rec) = self.db.folders.get(&wtxn, &folder_id)?
            && let Some(pid) = rec.parent_id
            && let Some(mut siblings) = self.db.folders_by_parent.get(&wtxn, &pid)?
        {
            siblings.retain(|&x| x != folder_id);
            self.db.folders_by_parent.put(&mut wtxn, &pid, &siblings)?;
        }

        for id in &metrics.affected_folder_ids {
            if let Some(folder_rec) = self.db.folders.get(&wtxn, id)? {
                if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, id)? {
                    deleted_media_count += media_ids.len();
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
                        self.db.thumbnails.delete(&mut wtxn, &media_id)?;
                        self.db.video_previews.delete(&mut wtxn, &media_id)?;
                    }
                }

                self.db.media_by_folder.delete(&mut wtxn, id)?;
                self.db.folders_by_parent.delete(&mut wtxn, id)?;
                self.db
                    .folders_by_path
                    .delete(&mut wtxn, &folder_rec.path)?;
                self.db.folders.delete(&mut wtxn, id)?;
            }
        }

        wtxn.commit()?;

        tracing::info!(
            folder_id,
            deleted_folders = metrics.affected_folder_ids.len(),
            deleted_media_count,
            "[FolderOperations: Delete] Folder deletion committed to LMDB"
        );

        Ok(FolderDeleteReport {
            deleted_folder_ids: metrics.affected_folder_ids,
            deleted_media_count,
            trashed: to_trash,
        })
    }

    pub fn check_and_restore_if_exists(&self, folder_id: FolderId) -> Result<bool, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let folder = self
            .db
            .folders
            .get(&rtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let path = PathBuf::from(&folder.path);
        drop(rtxn);

        if path.exists() {
            let mut wtxn = self.db.env.write_txn()?;
            if let Some(mut rec) = self.db.folders.get(&wtxn, &folder_id)?
                && rec.is_deleted
            {
                rec.is_deleted = false;
                self.db.folders.put(&mut wtxn, &folder_id, &rec)?;
                wtxn.commit()?;
                tracing::info!(
                    folder_id,
                    path = %LogObscurer::path(&path),
                    "[FolderOperations: Reborn] Restored missing folder to active status"
                );
            }
            return Ok(true);
        }

        Ok(false)
    }

    pub fn purge_folder_from_db(
        &self,
        folder_id: FolderId,
    ) -> Result<FolderDeleteReport, LibraryError> {
        let metrics = self.get_metrics(folder_id)?;

        let mut wtxn = self.db.env.write_txn()?;
        let mut deleted_media_count = 0;

        if let Some(rec) = self.db.folders.get(&wtxn, &folder_id)?
            && let Some(pid) = rec.parent_id
            && let Some(mut siblings) = self.db.folders_by_parent.get(&wtxn, &pid)?
        {
            siblings.retain(|&x| x != folder_id);
            self.db.folders_by_parent.put(&mut wtxn, &pid, &siblings)?;
        }

        for id in &metrics.affected_folder_ids {
            if let Some(folder_rec) = self.db.folders.get(&wtxn, id)? {
                if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, id)? {
                    deleted_media_count += media_ids.len();
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
                        self.db.thumbnails.delete(&mut wtxn, &media_id)?;
                        self.db.video_previews.delete(&mut wtxn, &media_id)?;
                    }
                }

                self.db.media_by_folder.delete(&mut wtxn, id)?;
                self.db.folders_by_parent.delete(&mut wtxn, id)?;
                self.db
                    .folders_by_path
                    .delete(&mut wtxn, &folder_rec.path)?;
                self.db.folders.delete(&mut wtxn, id)?;
            }
        }

        wtxn.commit()?;

        tracing::info!(
            folder_id,
            purged_folders = metrics.affected_folder_ids.len(),
            deleted_media_count,
            "[FolderOperations: Purge] Purged folder records from LMDB"
        );

        Ok(FolderDeleteReport {
            deleted_folder_ids: metrics.affected_folder_ids,
            deleted_media_count,
            trashed: false,
        })
    }

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
            folder = %LogObscurer::folder(Some(folder_id), &src_path),
            new_parent = %LogObscurer::folder(Some(new_parent_id), &dst_parent_path),
            "[FolderOperations: Move] Relocating folder on filesystem..."
        );

        std::fs::rename(&src_path, &dst_path)?;

        // Update old parent
        if let Some(old_pid) = record.parent_id
            && let Some(mut siblings) = self.db.folders_by_parent.get(&wtxn, &old_pid)?
        {
            siblings.retain(|&x| x != folder_id);
            self.db.folders_by_parent.put(&mut wtxn, &old_pid, &siblings)?;
        }

        // Update new parent
        let mut new_siblings = self
            .db
            .folders_by_parent
            .get(&wtxn, &new_parent_id)?
            .unwrap_or_default();
        if !new_siblings.contains(&folder_id) {
            new_siblings.push(folder_id);
            self.db
                .folders_by_parent
                .put(&mut wtxn, &new_parent_id, &new_siblings)?;
        }

        let old_depth = record.depth;
        let new_depth = parent_record.depth + 1;
        let depth_delta = new_depth as i32 - old_depth as i32;

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
            rec.depth = (rec.depth as i32 + depth_delta).max(0) as u32;

            if id == folder_id {
                rec.parent_id = Some(new_parent_id);
            }

            self.db.folders.put(&mut wtxn, &id, &rec)?;

            if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, &id)? {
                affected_media_count += media_ids.len();
            }
        }

        wtxn.commit()?;

        tracing::info!(
            folder_id,
            new_path = %LogObscurer::path(&dst_path),
            affected_media_count,
            "[FolderOperations: Move] Folder reparenting committed to LMDB"
        );

        Ok(FolderMoveReport {
            moved_folder_id: folder_id,
            affected_folders_count: 1,
            affected_media_count,
        })
    }
}
