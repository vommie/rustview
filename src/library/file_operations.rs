use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{
    FileConflictItemDetails, FileConflictReport, FileTransferKind, FolderId, FolderRecord, MediaId,
    MediaRecord,
};
use super::refresh_media_files::MediaItemRefresher;
use super::schema::SEQ_MEDIA_ID;
use crate::media::{MediaFormat, MediaHasher, MetadataExtractor};
use crate::settings::LogObscurer;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// POSIX Linux media filename validator ensuring safety and filesystem compliance.
pub struct FilenameValidator;

impl FilenameValidator {
    /// Validates and trims a filename according to Linux POSIX naming rules.
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

/// Resolved filesystem paths for a file rename operation.
#[derive(Debug, Clone)]
struct RenamePaths {
    old_path: PathBuf,
    new_path: PathBuf,
    validated_name: String,
}

/// Robust filesystem mover supporting both standard renames and cross-filesystem copy+remove fallbacks.
pub fn robust_move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if is_same_file_path(src, dst) {
        return Ok(());
    }
    let t0 = Instant::now();
    match std::fs::rename(src, dst) {
        Ok(()) => {
            tracing::debug!(
                src = %LogObscurer::path(src),
                dst = %LogObscurer::path(dst),
                elapsed_us = t0.elapsed().as_micros(),
                "[FileOperations: FS Move] Atomic rename succeeded on same filesystem"
            );
            Ok(())
        }
        Err(err) => {
            tracing::info!(
                src = %LogObscurer::path(src),
                dst = %LogObscurer::path(dst),
                error = %err,
                "[FileOperations: FS Move] Physical rename failed (cross-device/EXDEV detected). Executing copy + remove fallback..."
            );
            let t_copy = Instant::now();
            let bytes = std::fs::copy(src, dst)?;
            let copy_dur = t_copy.elapsed();
            std::fs::remove_file(src)?;
            tracing::info!(
                bytes_copied = bytes,
                copy_time_ms = copy_dur.as_millis(),
                total_time_ms = t0.elapsed().as_millis(),
                "[FileOperations: FS Move] Cross-filesystem move completed successfully"
            );
            Ok(())
        }
    }
}

/// Checks if two path references point to the exact same physical filesystem location.
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

/// High-level file management operations subsystem for the media library.
pub struct FileOperations<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FileOperations<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Atomically renames a media file on the physical filesystem and updates its LMDB record.
    pub fn rename_media(
        &self,
        media_id: MediaId,
        new_filename: &str,
    ) -> Result<MediaRecord, LibraryError> {
        let t0 = Instant::now();
        let validated_name = FilenameValidator::validate(new_filename)?;

        let mut wtxn = self.db.env.write_txn()?;

        let record = self
            .db
            .media
            .get(&wtxn, &media_id)?
            .ok_or(LibraryError::MediaNotFound(media_id))?;

        let folder = self
            .db
            .folders
            .get(&wtxn, &record.folder_id)?
            .ok_or(LibraryError::FolderNotFound(record.folder_id))?;

        let paths = self.resolve_rename_paths(&folder, &record, validated_name)?;

        if is_same_file_path(&paths.old_path, &paths.new_path) {
            return Ok(record);
        }

        self.verify_destination_available(&paths)?;

        tracing::info!(
            media = %LogObscurer::media(Some(media_id), &record.filename),
            old_path = %LogObscurer::path(&paths.old_path),
            new_path = %LogObscurer::path(&paths.new_path),
            "Executing physical file rename operation..."
        );

        let mtime = self.perform_fs_rename(&paths, record.modification_time)?;
        let updated_record = self.commit_rename_to_db(&mut wtxn, media_id, record, &paths, mtime)?;

        wtxn.commit()?;

        tracing::info!(
            media_id = media_id,
            new_filename = %LogObscurer::filename(&paths.validated_name),
            elapsed_us = t0.elapsed().as_micros(),
            "Committed file rename operation to LMDB storage successfully"
        );

        Ok(updated_record)
    }

    /// Generates a unique, collision-free filename in the target folder by appending sequential counter suffixes like `(1)`, `(2)`.
    pub fn generate_unique_filename(target_folder_path: &Path, original_name: &str) -> String {
        let t0 = Instant::now();
        let path = Path::new(original_name);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(original_name);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let mut counter = 1u32;
        loop {
            let candidate = if ext.is_empty() {
                format!("{} ({})", stem, counter)
            } else {
                format!("{} ({}).{}", stem, counter, ext)
            };

            if !target_folder_path.join(&candidate).exists() {
                tracing::debug!(
                    original = %LogObscurer::filename(original_name),
                    resolved_unique = %LogObscurer::filename(&candidate),
                    iterations = counter,
                    target_dir = %LogObscurer::path(target_folder_path),
                    elapsed_us = t0.elapsed().as_micros(),
                    "[FileOperations: AutoRename] Found available unique candidate filename"
                );
                return candidate;
            }

            counter += 1;
            if counter > 100_000 {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let fallback = if ext.is_empty() {
                    format!("{}_{}", stem, timestamp)
                } else {
                    format!("{}_{}.{}", stem, timestamp, ext)
                };
                tracing::warn!(
                    original = %LogObscurer::filename(original_name),
                    fallback = %LogObscurer::filename(&fallback),
                    "[FileOperations: AutoRename] Exceeded 100,000 iterations limit. Using timestamp fallback"
                );
                return fallback;
            }
        }
    }

    /// Probes full file metadata, checksum, dimensions, duration, and rating for conflict comparison.
    pub fn probe_file_details(
        &self,
        path: &Path,
        folder_id: FolderId,
        media_id: Option<MediaId>,
    ) -> Result<FileConflictItemDetails, LibraryError> {
        let t0 = Instant::now();
        let rtxn = self.db.env.read_txn()?;
        let folder = self
            .db
            .folders
            .get(&rtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;

        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let fs_meta = std::fs::metadata(path)?;
        let file_size = fs_meta.len();
        let modification_time = fs_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let creation_time = fs_meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(modification_time);

        let format = MediaFormat::from_path(path);

        let (width, height, duration_ms, rating, file_hash) = if let Some(m_id) = media_id
            && let Ok(Some(rec)) = self.db.media.get(&rtxn, &m_id)
        {
            (rec.width, rec.height, rec.duration_ms, rec.rating, rec.file_hash)
        } else {
            let (w, h, dur) = if let Ok(meta) = MetadataExtractor::extract(path) {
                (meta.width, meta.height, meta.duration_ms)
            } else {
                (0, 0, 0)
            };
            let hash = MediaHasher::hash_file(path).unwrap_or_default();
            (w, h, dur, 0, hash)
        };

        tracing::debug!(
            file = %LogObscurer::path(path),
            size_bytes = file_size,
            dimensions = format!("{}x{}", width, height),
            duration_ms = duration_ms,
            hash = %MediaHasher::to_hex(&file_hash),
            elapsed_us = t0.elapsed().as_micros(),
            "[FileOperations: Probe] File attributes inspected for conflict check"
        );

        Ok(FileConflictItemDetails {
            media_id,
            folder_id,
            folder_path: folder.path,
            filename,
            full_path: path.to_path_buf(),
            file_size,
            modification_time,
            creation_time,
            duration_ms,
            file_hash,
            width,
            height,
            format,
            rating,
        })
    }

    /// Checks whether copying or moving a media file into `target_folder_id` causes a filename collision or self-collision.
    pub fn check_transfer_conflict(
        &self,
        media_id: MediaId,
        target_folder_id: FolderId,
        transfer_kind: FileTransferKind,
    ) -> Result<Option<FileConflictReport>, LibraryError> {
        let t0 = Instant::now();
        let rtxn = self.db.env.read_txn()?;

        let source_record = self
            .db
            .media
            .get(&rtxn, &media_id)?
            .ok_or(LibraryError::MediaNotFound(media_id))?;

        let source_folder = self
            .db
            .folders
            .get(&rtxn, &source_record.folder_id)?
            .ok_or(LibraryError::FolderNotFound(source_record.folder_id))?;

        let target_folder = self
            .db
            .folders
            .get(&rtxn, &target_folder_id)?
            .ok_or(LibraryError::FolderNotFound(target_folder_id))?;

        let src_path = Path::new(&source_folder.path).join(&source_record.filename);
        let dst_path = Path::new(&target_folder.path).join(&source_record.filename);

        // Self-Move: Moving a file to the exact same directory is a safe no-op
        if transfer_kind == FileTransferKind::Move && (source_record.folder_id == target_folder_id || is_same_file_path(&src_path, &dst_path)) {
            return Ok(None);
        }

        let target_media_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &target_folder_id)?
            .unwrap_or_default();

        let mut existing_target_media_id = None;
        for &t_id in &target_media_ids {
            if let Some(t_rec) = self.db.media.get(&rtxn, &t_id)?
                && t_rec.filename.eq_ignore_ascii_case(&source_record.filename)
            {
                existing_target_media_id = Some(t_id);
                break;
            }
        }
        drop(rtxn);

        if dst_path.exists() || existing_target_media_id.is_some() {
            let is_self_collision = is_same_file_path(&src_path, &dst_path);
            tracing::info!(
                media_id = media_id,
                transfer_kind = ?transfer_kind,
                is_self_collision = is_self_collision,
                filename = %LogObscurer::filename(&source_record.filename),
                target_folder = %LogObscurer::folder(Some(target_folder_id), &target_folder.path),
                destination_exists_on_disk = dst_path.exists(),
                destination_indexed_in_db = existing_target_media_id.is_some(),
                elapsed_us = t0.elapsed().as_micros(),
                "[FileOperations: ConflictCheck] Collision detected in destination folder"
            );

            let source_details = self.probe_file_details(&src_path, source_folder.id, Some(media_id))?;
            let dest_details = if is_self_collision {
                source_details.clone()
            } else {
                self.probe_file_details(&dst_path, target_folder_id, existing_target_media_id)?
            };

            Ok(Some(FileConflictReport {
                source: source_details,
                destination: dest_details,
            }))
        } else {
            tracing::debug!(
                media_id = media_id,
                transfer_kind = ?transfer_kind,
                filename = %LogObscurer::filename(&source_record.filename),
                target_folder = %LogObscurer::folder(Some(target_folder_id), &target_folder.path),
                elapsed_us = t0.elapsed().as_micros(),
                "[FileOperations: ConflictCheck] Target is clean, no conflict"
            );
            Ok(None)
        }
    }

    /// Moves a media file physically to a target folder, updating LMDB indices, with support for custom renamed filenames and overwrite replacement.
    pub fn move_single_media(
        &self,
        media_id: MediaId,
        target_folder_id: FolderId,
        custom_filename: Option<&str>,
        overwrite_existing: bool,
    ) -> Result<MediaRecord, LibraryError> {
        let t0 = Instant::now();
        let mut wtxn = self.db.env.write_txn()?;

        let mut source_record = self
            .db
            .media
            .get(&wtxn, &media_id)?
            .ok_or(LibraryError::MediaNotFound(media_id))?;

        let source_folder = self
            .db
            .folders
            .get(&wtxn, &source_record.folder_id)?
            .ok_or(LibraryError::FolderNotFound(source_record.folder_id))?;

        let target_folder = self
            .db
            .folders
            .get(&wtxn, &target_folder_id)?
            .ok_or(LibraryError::FolderNotFound(target_folder_id))?;

        let old_folder_id = source_record.folder_id;
        let target_name_raw = custom_filename.unwrap_or(&source_record.filename);
        let validated_name = FilenameValidator::validate(target_name_raw)?;

        let src_path = Path::new(&source_folder.path).join(&source_record.filename);
        let dst_path = Path::new(&target_folder.path).join(&validated_name);

        if !src_path.exists() {
            return Err(LibraryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Source media file not found on disk: {:?}", src_path),
            )));
        }

        let is_same_file = is_same_file_path(&src_path, &dst_path);

        // Self-Move: If moving to the exact same file path, safely no-op
        if is_same_file {
            tracing::debug!(
                media_id = media_id,
                file = %LogObscurer::path(&src_path),
                "[FileOperations: Move Self-Check] File is already at destination path. Operation bypassed."
            );
            return Ok(source_record);
        }

        if dst_path.exists() && !overwrite_existing {
            return Err(LibraryError::FileOperationAlreadyExists(validated_name));
        }

        // 1. Physical move on storage
        tracing::info!(
            media = %LogObscurer::media(Some(media_id), &source_record.filename),
            src = %LogObscurer::path(&src_path),
            dst = %LogObscurer::path(&dst_path),
            overwrite = overwrite_existing,
            "Moving media file physically on disk..."
        );
        robust_move_file(&src_path, &dst_path)?;

        let fs_meta = std::fs::metadata(&dst_path)?;
        let mtime = fs_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(source_record.modification_time);

        let ctime = fs_meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(source_record.creation_time);

        // 2. Overwrite handling in LMDB: delete overwritten target record if one existed in destination
        if overwrite_existing {
            let mut target_media_ids = self
                .db
                .media_by_folder
                .get(&wtxn, &target_folder_id)?
                .unwrap_or_default();

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
                tracing::info!(
                    overwritten_media_id = old_dst_id,
                    target_folder_id = target_folder_id,
                    "[FileOperations: Overwrite] Removing overwritten media record from LMDB"
                );
                self.db.update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
                self.db.media.delete(&mut wtxn, &old_dst_id)?;
                self.db.thumbnails.delete(&mut wtxn, &old_dst_id)?;
                self.db.video_previews.delete(&mut wtxn, &old_dst_id)?;
                self.db.tags_by_media.delete(&mut wtxn, &old_dst_id)?;

                target_media_ids.retain(|&id| id != old_dst_id);
                self.db.media_by_folder.put(&mut wtxn, &target_folder_id, &target_media_ids)?;
            }
        }

        // 3. Update source media record in LMDB
        source_record.folder_id = target_folder_id;
        source_record.filename = validated_name.clone();
        source_record.modification_time = mtime;
        source_record.creation_time = ctime;
        source_record.file_size = fs_meta.len();
        source_record.is_deleted = false;
        self.db.media.put(&mut wtxn, &media_id, &source_record)?;

        // 4. Update folder membership indices in LMDB
        if old_folder_id != target_folder_id {
            // Remove from old folder
            if let Some(mut old_list) = self.db.media_by_folder.get(&wtxn, &old_folder_id)? {
                old_list.retain(|&id| id != media_id);
                self.db.media_by_folder.put(&mut wtxn, &old_folder_id, &old_list)?;
            }

            // Add to new folder
            let mut new_list = self
                .db
                .media_by_folder
                .get(&wtxn, &target_folder_id)?
                .unwrap_or_default();
            if !new_list.contains(&media_id) {
                new_list.push(media_id);
                self.db.media_by_folder.put(&mut wtxn, &target_folder_id, &new_list)?;
            }
        }

        wtxn.commit()?;

        tracing::info!(
            media_id = media_id,
            from_folder = %LogObscurer::folder(Some(old_folder_id), &source_folder.path),
            to_folder = %LogObscurer::folder(Some(target_folder_id), &target_folder.path),
            filename = %LogObscurer::filename(&validated_name),
            elapsed_ms = t0.elapsed().as_millis(),
            "[FileOperations: Move] Media move transaction committed to LMDB storage successfully"
        );

        Ok(source_record)
    }

    /// Copies a media file physically to a target folder, creating a new LMDB media instance with cloned tags, ratings, and shared packfile thumbnail pointers.
    pub fn copy_single_media(
        &self,
        media_id: MediaId,
        target_folder_id: FolderId,
        custom_filename: Option<&str>,
        overwrite_existing: bool,
    ) -> Result<MediaRecord, LibraryError> {
        let t0 = Instant::now();
        let mut wtxn = self.db.env.write_txn()?;

        let source_record = self
            .db
            .media
            .get(&wtxn, &media_id)?
            .ok_or(LibraryError::MediaNotFound(media_id))?;

        let source_folder = self
            .db
            .folders
            .get(&wtxn, &source_record.folder_id)?
            .ok_or(LibraryError::FolderNotFound(source_record.folder_id))?;

        let target_folder = self
            .db
            .folders
            .get(&wtxn, &target_folder_id)?
            .ok_or(LibraryError::FolderNotFound(target_folder_id))?;

        let target_name_raw = custom_filename.unwrap_or(&source_record.filename);
        let validated_name = FilenameValidator::validate(target_name_raw)?;

        let src_path = Path::new(&source_folder.path).join(&source_record.filename);
        let dst_path = Path::new(&target_folder.path).join(&validated_name);

        if !src_path.exists() {
            return Err(LibraryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Source media file not found on disk: {:?}", src_path),
            )));
        }

        let is_same_file = is_same_file_path(&src_path, &dst_path);

        // Self-Overwrite Guard: Copying a file onto itself with overwrite must NOT truncate/re-copy the file
        if is_same_file {
            if overwrite_existing {
                tracing::warn!(
                    media_id = media_id,
                    file = %LogObscurer::path(&src_path),
                    "[FileOperations: SelfTransferGuard] Copying file onto itself with overwrite is a safe identity no-op. Truncation prevented."
                );
                return Ok(source_record);
            } else {
                return Err(LibraryError::FileOperationAlreadyExists(format!(
                    "Cannot copy file onto itself in the same directory without renaming: {}",
                    validated_name
                )));
            }
        }

        if dst_path.exists() && !overwrite_existing {
            return Err(LibraryError::FileOperationAlreadyExists(validated_name));
        }

        // 1. Physical copy on storage
        tracing::info!(
            media = %LogObscurer::media(Some(media_id), &source_record.filename),
            src = %LogObscurer::path(&src_path),
            dst = %LogObscurer::path(&dst_path),
            overwrite = overwrite_existing,
            "Copying media file physically on disk..."
        );
        std::fs::copy(&src_path, &dst_path)?;

        let fs_meta = std::fs::metadata(&dst_path)?;
        let mtime = fs_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(source_record.modification_time);

        let ctime = fs_meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(source_record.creation_time);

        // 2. Overwrite cleanup in target if existing destination record is replaced
        let mut target_media_ids = self
            .db
            .media_by_folder
            .get(&wtxn, &target_folder_id)?
            .unwrap_or_default();

        if overwrite_existing && dst_path.exists() {
            let mut overwritten_id_opt = None;
            for &t_id in &target_media_ids {
                if let Some(t_rec) = self.db.media.get(&wtxn, &t_id)?
                    && t_rec.filename.eq_ignore_ascii_case(&validated_name)
                {
                    overwritten_id_opt = Some((t_id, t_rec.file_hash));
                    break;
                }
            }

            if let Some((old_dst_id, old_hash)) = overwritten_id_opt {
                tracing::info!(
                    overwritten_media_id = old_dst_id,
                    target_folder_id = target_folder_id,
                    "[FileOperations: Copy Overwrite] Removing overwritten media record from LMDB"
                );
                self.db.update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
                self.db.media.delete(&mut wtxn, &old_dst_id)?;
                self.db.thumbnails.delete(&mut wtxn, &old_dst_id)?;
                self.db.video_previews.delete(&mut wtxn, &old_dst_id)?;
                self.db.tags_by_media.delete(&mut wtxn, &old_dst_id)?;

                target_media_ids.retain(|&id| id != old_dst_id);
            }
        }

        // 3. Allocate new Media ID and create new MediaRecord
        let new_media_id = self.db.next_id(&mut wtxn, SEQ_MEDIA_ID)?;
        let new_record = MediaRecord {
            id: new_media_id,
            folder_id: target_folder_id,
            filename: validated_name.clone(),
            file_hash: source_record.file_hash,
            file_size: fs_meta.len(),
            modification_time: mtime,
            creation_time: ctime,
            duration_ms: source_record.duration_ms,
            media_type: source_record.media_type,
            format: source_record.format,
            width: source_record.width,
            height: source_record.height,
            rating: source_record.rating,
            last_seen: 1,
            is_deleted: false,
        };

        self.db.media.put(&mut wtxn, &new_media_id, &new_record)?;
        self.db.update_media_hash_index(&mut wtxn, new_media_id, None, Some(&new_record.file_hash))?;

        // 4. Update folder media list
        if !target_media_ids.contains(&new_media_id) {
            target_media_ids.push(new_media_id);
            self.db.media_by_folder.put(&mut wtxn, &target_folder_id, &target_media_ids)?;
        }

        // 5. Clone Tag Associations
        if let Some(source_tags) = self.db.tags_by_media.get(&wtxn, &media_id)? {
            self.db.tags_by_media.put(&mut wtxn, &new_media_id, &source_tags)?;
            for &t_id in &source_tags {
                let mut tag_media_list = self.db.media_by_tag.get(&wtxn, &t_id)?.unwrap_or_default();
                if !tag_media_list.contains(&new_media_id) {
                    tag_media_list.push(new_media_id);
                    self.db.media_by_tag.put(&mut wtxn, &t_id, &tag_media_list)?;
                }
            }
        }

        // 6. Reuse Packfile Thumbnail & Preview locations (Zero-copy in PackStore)
        if let Some(thumb_loc) = self.db.thumbnails.get(&wtxn, &media_id)? {
            self.db.thumbnails.put(&mut wtxn, &new_media_id, &thumb_loc)?;
        }
        if let Some(prev_rec) = self.db.video_previews.get(&wtxn, &media_id)? {
            self.db.video_previews.put(&mut wtxn, &new_media_id, &prev_rec)?;
        }

        wtxn.commit()?;

        tracing::info!(
            source_media_id = media_id,
            new_media_id = new_media_id,
            from_folder = %LogObscurer::folder(Some(source_record.folder_id), &source_folder.path),
            to_folder = %LogObscurer::folder(Some(target_folder_id), &target_folder.path),
            filename = %LogObscurer::filename(&validated_name),
            elapsed_ms = t0.elapsed().as_millis(),
            "[FileOperations: Copy] Cloned media instance with tags, ratings, and shared packfile thumbnails committed to LMDB"
        );

        Ok(new_record)
    }

    /// Delegates media refresh to the dedicated `MediaItemRefresher` subsystem.
    pub fn refresh_media(&self, media_id: MediaId) -> Result<MediaRecord, LibraryError> {
        let refresher = MediaItemRefresher::new(self.db);
        refresher.refresh_media(media_id)
    }

    fn resolve_rename_paths(
        &self,
        folder: &FolderRecord,
        record: &MediaRecord,
        validated_name: String,
    ) -> Result<RenamePaths, LibraryError> {
        let folder_path = Path::new(&folder.path);
        let old_path = folder_path.join(&record.filename);
        let new_path = folder_path.join(&validated_name);

        Ok(RenamePaths {
            old_path,
            new_path,
            validated_name,
        })
    }

    fn verify_destination_available(&self, paths: &RenamePaths) -> Result<(), LibraryError> {
        if paths.new_path.exists() {
            return Err(LibraryError::FileOperationAlreadyExists(
                paths.validated_name.clone(),
            ));
        }
        Ok(())
    }

    fn perform_fs_rename(
        &self,
        paths: &RenamePaths,
        fallback_mtime: i64,
    ) -> Result<i64, LibraryError> {
        std::fs::rename(&paths.old_path, &paths.new_path)?;

        let fs_meta = std::fs::metadata(&paths.new_path)?;
        let mtime = fs_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(fallback_mtime);

        Ok(mtime)
    }

    fn commit_rename_to_db(
        &self,
        wtxn: &mut heed::RwTxn,
        media_id: MediaId,
        mut record: MediaRecord,
        paths: &RenamePaths,
        mtime: i64,
    ) -> Result<MediaRecord, LibraryError> {
        record.filename = paths.validated_name.clone();
        record.modification_time = mtime;
        self.db.media.put(wtxn, &media_id, &record)?;
        Ok(record)
    }
}
