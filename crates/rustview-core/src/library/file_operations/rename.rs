//! Renaming a medium: filename validation, conflict check and the rename itself.

use super::{FileOperations, robust_move_file};
use crate::library::error::LibraryError;
use crate::library::folders::FolderManager;
use crate::library::models::{FileConflictReport, FolderRecord, MediaId, MediaRecord};
use crate::media::{FormatPolicy, MediaFormat};
use crate::settings::LogObscurer;
use crate::util::cast::SaturatingAs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct FilenameValidator;

impl FilenameValidator {
    pub fn validate(name: &str) -> Result<String, LibraryError> {
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Filename cannot be empty".to_string(),
            ));
        }

        if trimmed == "." || trimmed == ".." {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Filename cannot be '.' or '..'".to_string(),
            ));
        }

        if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Filename cannot contain path separators or NUL characters".to_string(),
            ));
        }

        if trimmed.len() > 255 {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Filename cannot exceed 255 bytes (NAME_MAX limit)".to_string(),
            ));
        }

        if trimmed.chars().any(|c| (c as u32) < 32 || c == '\x7f') {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Filename cannot contain control characters".to_string(),
            ));
        }

        Ok(trimmed.to_string())
    }
}

#[derive(Debug, Clone)]
pub(super) struct RenamePaths {
    old_path: PathBuf,
    new_path: PathBuf,
    validated_name: String,
}

#[inline]
pub fn is_same_file_path(p1: &Path, p2: &Path) -> bool {
    if p1 == p2 {
        return true;
    }
    match (p1.canonicalize(), p2.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl FileOperations<'_> {
    #[inline]
    pub fn rename_media(&self, media_id: MediaId, new_filename: &str) -> Result<MediaRecord, LibraryError> {
        self.rename_media_with_overwrite(media_id, new_filename, false)
    }

    pub fn rename_media_with_overwrite(
        &self,
        media_id: MediaId,
        new_filename: &str,
        overwrite_existing: bool,
    ) -> Result<MediaRecord, LibraryError> {
        let t0 = Instant::now();
        let validated_name = FilenameValidator::validate(new_filename)?;

        let mut wtxn = self.db.env.write_txn()?;

        let mut record = self
            .db
            .media
            .get(&wtxn, &media_id)?
            .ok_or(LibraryError::MediaNotFound(media_id))?;

        let folder = self
            .db
            .folders
            .get(&wtxn, &record.folder_id)?
            .ok_or(LibraryError::FolderNotFound(record.folder_id))?;

        let paths = Self::resolve_rename_paths(&folder, &record, validated_name.clone());

        if is_same_file_path(&paths.old_path, &paths.new_path) {
            return Ok(record);
        }

        if paths.new_path.exists() && !overwrite_existing {
            return Err(LibraryError::FileOperationAlreadyExists(paths.validated_name));
        }

        if overwrite_existing && paths.new_path.exists() {
            let mut target_media_ids = self.db.media_by_folder.get(&wtxn, &folder.id)?.unwrap_or_default();

            let mut overwritten_id_opt = None;
            for &t_id in &target_media_ids {
                if t_id != media_id
                    && let Some(t_rec) = self.db.media.get(&wtxn, &t_id)?
                    && t_rec.filename.eq_ignore_ascii_case(&validated_name)
                {
                    overwritten_id_opt = Some((t_id, t_rec.file_hash));
                    break;
                }
            }

            if let Some((old_dst_id, old_hash)) = overwritten_id_opt {
                self.db
                    .update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
                self.db.media.delete(&mut wtxn, &old_dst_id)?;
                self.db.thumbnails.delete(&mut wtxn, &old_dst_id)?;
                self.db.video_previews.delete(&mut wtxn, &old_dst_id)?;
                self.db.tags_by_media.delete(&mut wtxn, &old_dst_id)?;

                target_media_ids.retain(|&id| id != old_dst_id);
                self.db.media_by_folder.put(&mut wtxn, &folder.id, &target_media_ids)?;

                // Deduct overwritten media count from folder
                FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, folder.id, -1)?;
            }
        }

        let old_file_physically_exists = paths.old_path.exists();

        if old_file_physically_exists {
            robust_move_file(&paths.old_path, &paths.new_path)?;

            if let Ok(fs_meta) = std::fs::metadata(&paths.new_path) {
                record.modification_time = fs_meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(record.modification_time, |d| d.as_secs().saturating_as::<i64>());

                record.creation_time = fs_meta
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(record.creation_time, |d| d.as_secs().saturating_as::<i64>());

                record.file_size = fs_meta.len();
            }
        }

        // The new name decides the format (what the format switches of the Settings act on); image or video stays as the
        // content made it, so renaming a JPEG to ".mp4" does not turn it into a video.
        let format = MediaFormat::from_path(&paths.new_path);

        // A new extension can move the file into or out of a switched-off format: the folder's count follows what it lists.
        let mask = FormatPolicy::mask();
        if !record.is_deleted && mask.shows(record.format) != mask.shows(format) {
            let step = if mask.shows(format) { 1 } else { -1 };
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, record.folder_id, step)?;
        }

        record.filename.clone_from(&paths.validated_name);
        record.format = format;
        record.is_deleted = false;

        self.db.media.put(&mut wtxn, &media_id, &record)?;
        wtxn.commit()?;

        tracing::info!(
            media_id = media_id,
            new_filename = %LogObscurer::filename(&paths.validated_name),
            elapsed_us = t0.elapsed().as_micros(),
            "Committed file rename operation to LMDB storage successfully"
        );

        Ok(record)
    }

    pub fn check_rename_conflict(
        &self,
        media_id: MediaId,
        new_filename: &str,
    ) -> Result<Option<FileConflictReport>, LibraryError> {
        let validated_name = FilenameValidator::validate(new_filename)?;

        let rtxn = self.db.env.read_txn()?;
        let source_record = self
            .db
            .media
            .get(&rtxn, &media_id)?
            .ok_or(LibraryError::MediaNotFound(media_id))?;

        let folder = self
            .db
            .folders
            .get(&rtxn, &source_record.folder_id)?
            .ok_or(LibraryError::FolderNotFound(source_record.folder_id))?;

        let src_path = Path::new(&folder.path).join(&source_record.filename);
        let dst_path = Path::new(&folder.path).join(&validated_name);

        if is_same_file_path(&src_path, &dst_path) {
            return Ok(None);
        }

        let target_media_ids = self.db.media_by_folder.get(&rtxn, &folder.id)?.unwrap_or_default();

        let mut existing_target_media_id = None;
        for &t_id in &target_media_ids {
            if t_id != media_id
                && let Some(t_rec) = self.db.media.get(&rtxn, &t_id)?
                && t_rec.filename.eq_ignore_ascii_case(&validated_name)
            {
                existing_target_media_id = Some(t_id);
                break;
            }
        }
        drop(rtxn);

        if dst_path.exists() || existing_target_media_id.is_some() {
            let source_details = self.probe_file_details(&src_path, folder.id, Some(media_id))?;
            let dest_details = self.probe_file_details(&dst_path, folder.id, existing_target_media_id)?;

            Ok(Some(FileConflictReport {
                source: source_details,
                destination: dest_details,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn generate_unique_filename(target_folder_path: &Path, original_name: &str) -> String {
        let path = Path::new(original_name);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(original_name);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let mut counter = 1u32;
        loop {
            let candidate = if ext.is_empty() {
                format!("{stem} ({counter})")
            } else {
                format!("{stem} ({counter}).{ext}")
            };

            if !target_folder_path.join(&candidate).exists() {
                return candidate;
            }

            counter += 1;
            if counter > 100_000 {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                return if ext.is_empty() {
                    format!("{stem}_{timestamp}")
                } else {
                    format!("{stem}_{timestamp}.{ext}")
                };
            }
        }
    }

    pub(super) fn resolve_rename_paths(
        folder: &FolderRecord,
        record: &MediaRecord,
        validated_name: String,
    ) -> RenamePaths {
        let folder_path = Path::new(&folder.path);
        let old_path = folder_path.join(&record.filename);
        let new_path = folder_path.join(&validated_name);

        RenamePaths {
            old_path,
            new_path,
            validated_name,
        }
    }
}
