use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folder_move::FolderMoveExecutor;
use super::folders::FolderManager;
use super::models::{FolderId, FolderRecord, MediaId};
use crate::media::FormatPolicy;
use crate::settings::LogObscurer;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub use super::folder_move::FolderMoveReport;

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
    /// Subfolders (below the folder) that are flagged as missing on disk already.
    pub missing_subfolders: usize,
    /// Media records in the subtree that are flagged as missing already (not part of `media_files_count`).
    pub missing_media: usize,
}

/// Missing folders by parent (see `FolderOperations::missing_children`).
#[derive(Debug, Clone, Default)]
pub struct MissingChildren(HashMap<FolderId, Vec<FolderId>>);

#[derive(Debug, Clone, Default)]
pub struct FolderDeleteReport {
    pub deleted_folder_ids: Vec<FolderId>,
    pub deleted_media_ids: Vec<MediaId>,
    pub deleted_media_count: usize,
    pub trashed: bool,
}

/// Subsystem managing atomic physical & database folder operations.
pub struct FolderOperations<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderOperations<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// The folders that a scan flagged as missing, by parent. A scan rebuilds the child list of a parent from what exists on disk, so a
    /// missing folder is no longer in `folders_by_parent`; this is the one place that still finds it (one pass over the folder table).
    pub fn missing_children(&self) -> Result<MissingChildren, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut by_parent: HashMap<FolderId, Vec<FolderId>> = HashMap::new();
        for entry in self.db.folders.iter(&rtxn)? {
            let (id, record) = entry?;
            if record.is_deleted
                && let Some(parent) = record.parent_id
            {
                by_parent.entry(parent).or_default().push(id);
            }
        }
        Ok(MissingChildren(by_parent))
    }

    /// Computes subtree metrics using relational parent indices, missing subfolders included.
    pub fn get_metrics(&self, folder_id: FolderId) -> Result<FolderSubtreeMetrics, LibraryError> {
        self.get_metrics_with(folder_id, &self.missing_children()?)
    }

    /// `get_metrics` with the missing folders looked up once for many calls.
    pub fn get_metrics_with(
        &self,
        folder_id: FolderId,
        missing: &MissingChildren,
    ) -> Result<FolderSubtreeMetrics, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let folder = self
            .db
            .folders
            .get(&rtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let folder_path = PathBuf::from(&folder.path);
        let mut affected_folders = vec![folder_id];
        let mut seen: HashSet<FolderId> = HashSet::from([folder_id]);
        let mut queue = vec![folder_id];

        while let Some(current) = queue.pop() {
            let indexed = self.db.folders_by_parent.get(&rtxn, &current)?.unwrap_or_default();
            for cid in indexed
                .into_iter()
                .chain(missing.0.get(&current).into_iter().flatten().copied())
            {
                if seen.insert(cid) {
                    affected_folders.push(cid);
                    queue.push(cid);
                }
            }
        }

        let mut total_media = 0;
        let mut missing_media = 0;
        let mut missing_subfolders = 0;
        for &id in &affected_folders {
            // Everything below a folder that is missing is missing with it, whatever its own flag says.
            let folder_is_missing =
                id != folder_id && self.db.folders.get(&rtxn, &id)?.is_some_and(|rec| rec.is_deleted);
            if folder_is_missing {
                missing_subfolders += 1;
            }
            if let Some(media_ids) = self.db.media_by_folder.get(&rtxn, &id)? {
                for media_id in media_ids {
                    if let Some(media_rec) = self.db.media.get(&rtxn, &media_id)? {
                        if media_rec.is_deleted || folder_is_missing {
                            missing_media += 1;
                        } else {
                            total_media += 1;
                        }
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
            missing_subfolders,
            missing_media,
        })
    }

    pub fn check_rename_conflict(&self, folder_id: FolderId, new_name: &str) -> Result<Option<PathBuf>, LibraryError> {
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
    pub fn rename_folder(&self, folder_id: FolderId, new_name: &str) -> Result<FolderRecord, LibraryError> {
        let t0 = Instant::now();
        let valid_name = FolderValidator::validate(new_name)?;

        let mut wtxn = self.db.env.write_txn()?;

        let mut record = self
            .db
            .folders
            .get(&wtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let old_path = PathBuf::from(&record.path);
        let parent_path = old_path
            .parent()
            .ok_or_else(|| LibraryError::FileOperationRenameFailed("Cannot rename root directory".into()))?;

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
            self.db.folders_by_path.put(&mut wtxn, &updated_p_str, &id)?;

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

    /// Deletes a folder physically on disk (Trash or Permanent Delete) and flags the folder and all
    /// contained media records in the database with `is_deleted = true`, adjusting count rollups.
    pub fn delete_folder(&self, folder_id: FolderId, to_trash: bool) -> Result<FolderDeleteReport, LibraryError> {
        let metrics = self.get_metrics(folder_id)?;
        let folder_path = metrics.path;

        tracing::info!(
            folder_id,
            path = %LogObscurer::path(&folder_path),
            to_trash,
            subfolders = metrics.subfolders_count,
            media = metrics.media_files_count,
            "[FolderOperations: Delete] Deleting folder on disk and flagging database records..."
        );

        if folder_path.exists() {
            if to_trash {
                trash::delete(&folder_path).map_err(|e| {
                    LibraryError::FileOperationRenameFailed(format!("Failed to move folder to Trash: {}", e))
                })?;
            } else {
                std::fs::remove_dir_all(&folder_path)?;
            }
        }

        let mut wtxn = self.db.env.write_txn()?;
        let mut deleted_media_ids = Vec::new();
        let mut deleted_media_count = 0;
        // The counts of the ancestors only hold what their folders list, so records of switched-off formats do not leave them.
        let mut deleted_shown_count = 0i64;
        let mask = FormatPolicy::mask();

        let mut parent_id_opt = None;
        if let Some(rec) = self.db.folders.get(&wtxn, &folder_id)? {
            parent_id_opt = rec.parent_id;
        }

        for &id in &metrics.affected_folder_ids {
            // Flag folder record as deleted
            if let Some(mut folder_rec) = self.db.folders.get(&wtxn, &id)? {
                folder_rec.is_deleted = true;
                self.db.folders.put(&mut wtxn, &id, &folder_rec)?;
            }

            // Flag all associated media records in this folder as deleted
            if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, &id)? {
                for &media_id in &media_ids {
                    if let Some(mut media_record) = self.db.media.get(&wtxn, &media_id)? {
                        if !media_record.is_deleted {
                            media_record.is_deleted = true;
                            self.db.media.put(&mut wtxn, &media_id, &media_record)?;
                            deleted_media_count += 1;
                            if mask.shows(media_record.format) {
                                deleted_shown_count += 1;
                            }
                        }
                        deleted_media_ids.push(media_id);
                    }
                }
            }
        }

        // Adjust parent direct subfolder count and roll up media delta through ancestors
        if let Some(pid) = parent_id_opt {
            if let Some(mut p_rec) = self.db.folders.get(&wtxn, &pid)? {
                p_rec.subfolder_count = p_rec.subfolder_count.saturating_sub(1);
                self.db.folders.put(&mut wtxn, &pid, &p_rec)?;
            }
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, pid, -deleted_shown_count)?;
        }

        wtxn.commit()?;

        tracing::info!(
            folder_id,
            deleted_folders = metrics.affected_folder_ids.len(),
            deleted_media_count,
            trashed = to_trash,
            "[FolderOperations: Delete] Folder deletion committed to LMDB (records flagged is_deleted = true)"
        );

        Ok(FolderDeleteReport {
            deleted_folder_ids: metrics.affected_folder_ids,
            deleted_media_ids,
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

    /// Permanently purges folder records and media from LMDB tables when cleaning missing/orphaned folders.
    pub fn purge_folder_from_db(&self, folder_id: FolderId) -> Result<FolderDeleteReport, LibraryError> {
        let metrics = self.get_metrics(folder_id)?;

        let mut wtxn = self.db.env.write_txn()?;
        let mut deleted_media_ids = Vec::new();
        let mut deleted_media_count = 0;

        let mut parent_id_opt = None;
        if let Some(rec) = self.db.folders.get(&wtxn, &folder_id)? {
            parent_id_opt = rec.parent_id;
            if let Some(pid) = rec.parent_id
                && let Some(mut siblings) = self.db.folders_by_parent.get(&wtxn, &pid)?
            {
                siblings.retain(|&x| x != folder_id);
                self.db.folders_by_parent.put(&mut wtxn, &pid, &siblings)?;
            }
        }

        for &id in &metrics.affected_folder_ids {
            if let Some(folder_rec) = self.db.folders.get(&wtxn, &id)? {
                if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, &id)? {
                    for media_id in media_ids {
                        deleted_media_ids.push(media_id);
                        deleted_media_count += 1;
                        if let Some(media_record) = self.db.media.get(&wtxn, &media_id)? {
                            self.db.update_media_hash_index(
                                &mut wtxn,
                                media_id,
                                Some(&media_record.file_hash),
                                None,
                            )?;
                        }
                        self.db.media.delete(&mut wtxn, &media_id)?;

                        if let Some(tags) = self.db.tags_by_media.get(&wtxn, &media_id)? {
                            for tag_id in tags {
                                if let Some(mut t_media) = self.db.media_by_tag.get(&wtxn, &tag_id)? {
                                    t_media.retain(|&m| m != media_id);
                                    if t_media.is_empty() {
                                        self.db.media_by_tag.delete(&mut wtxn, &tag_id)?;
                                    } else {
                                        self.db.media_by_tag.put(&mut wtxn, &tag_id, &t_media)?;
                                    }
                                }
                            }
                            self.db.tags_by_media.delete(&mut wtxn, &media_id)?;
                        }

                        self.db.thumbnails.delete(&mut wtxn, &media_id)?;
                        self.db.video_previews.delete(&mut wtxn, &media_id)?;
                    }
                }

                self.db.media_by_folder.delete(&mut wtxn, &id)?;
                self.db.folders_by_parent.delete(&mut wtxn, &id)?;
                self.db.folders_by_path.delete(&mut wtxn, &folder_rec.path)?;
                self.db.folders.delete(&mut wtxn, &id)?;
            }
        }

        if let Some(pid) = parent_id_opt
            && let Some(mut p_rec) = self.db.folders.get(&wtxn, &pid)?
        {
            p_rec.subfolder_count = p_rec.subfolder_count.saturating_sub(1);
            self.db.folders.put(&mut wtxn, &pid, &p_rec)?;
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
            deleted_media_ids,
            deleted_media_count,
            trashed: false,
        })
    }

    /// Moves a folder (including its subtree) below another folder, on disk and in LMDB.
    /// Delegates to `FolderMoveExecutor`, which stages database changes, renames on disk and commits atomically.
    pub fn move_folder(&self, folder_id: FolderId, new_parent_id: FolderId) -> Result<FolderMoveReport, LibraryError> {
        FolderMoveExecutor::new(self.db).execute(folder_id, new_parent_id)
    }
}
