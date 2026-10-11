use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folders::FolderManager;
use super::models::{
    FileConflictItemDetails, FileConflictReport, FileTransferKind, FolderId, FolderRecord,
    MediaId, MediaRecord,
};
use super::refresh_media_files::MediaItemRefresher;
use super::schema::SEQ_MEDIA_ID;
use crate::media::{MediaFormat, MediaHasher, MetadataExtractor};
use crate::settings::LogObscurer;
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
struct RenamePaths {
    old_path: PathBuf,
    new_path: PathBuf,
    validated_name: String,
}

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
                "[FileOperations: FS Move] Physical rename failed (EXDEV). Executing copy + remove fallback..."
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

pub struct FileOperations<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FileOperations<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    #[inline]
    pub fn rename_media(
        &self,
        media_id: MediaId,
        new_filename: &str,
    ) -> Result<MediaRecord, LibraryError> {
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

        let paths = self.resolve_rename_paths(&folder, &record, validated_name.clone())?;

        if is_same_file_path(&paths.old_path, &paths.new_path) {
            return Ok(record);
        }

        if paths.new_path.exists() && !overwrite_existing {
            return Err(LibraryError::FileOperationAlreadyExists(paths.validated_name));
        }

        if overwrite_existing && paths.new_path.exists() {
            let mut target_media_ids = self
                .db
                .media_by_folder
                .get(&wtxn, &folder.id)?
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
                self.db.update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
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
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(record.modification_time);

                record.creation_time = fs_meta
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(record.creation_time);

                record.file_size = fs_meta.len();
            }
        }

        let format = MediaFormat::from_path(&paths.new_path);
        let media_type = format.media_type().unwrap_or(record.media_type);

        record.filename = paths.validated_name.clone();
        record.format = format;
        record.media_type = media_type;
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

        let target_media_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &folder.id)?
            .unwrap_or_default();

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
                return candidate;
            }

            counter += 1;
            if counter > 100_000 {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                return if ext.is_empty() {
                    format!("{}_{}", stem, timestamp)
                } else {
                    format!("{}_{}.{}", stem, timestamp, ext)
                };
            }
        }
    }

    pub fn probe_file_details(
        &self,
        path: &Path,
        folder_id: FolderId,
        media_id: Option<MediaId>,
    ) -> Result<FileConflictItemDetails, LibraryError> {
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

        let db_record_opt = if let Some(m_id) = media_id {
            self.db.media.get(&rtxn, &m_id).ok().flatten()
        } else {
            None
        };

        let (file_size, modification_time, creation_time) = if let Ok(fs_meta) = std::fs::metadata(path) {
            let size = fs_meta.len();
            let mtime = fs_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let ctime = fs_meta
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(mtime);
            (size, mtime, ctime)
        } else if let Some(ref rec) = db_record_opt {
            (rec.file_size, rec.modification_time, rec.creation_time)
        } else {
            (0, 0, 0)
        };

        let format = MediaFormat::from_path(path);

        let (width, height, duration_ms, rating, file_hash) = if let Some(ref rec) = db_record_opt {
            (rec.width, rec.height, rec.duration_ms, rec.rating, rec.file_hash)
        } else if path.exists() {
            let (w, h, dur) = if let Ok(meta) = MetadataExtractor::extract(path) {
                (meta.width, meta.height, meta.duration_ms)
            } else {
                (0, 0, 0)
            };
            let hash = MediaHasher::hash_file(path).unwrap_or_default();
            (w, h, dur, 0, hash)
        } else {
            (0, 0, 0, 0, [0u8; 16])
        };

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

    pub fn check_transfer_conflict(
        &self,
        media_id: MediaId,
        target_folder_id: FolderId,
        transfer_kind: FileTransferKind,
    ) -> Result<Option<FileConflictReport>, LibraryError> {
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
            Ok(None)
        }
    }

    /// Moves a media file physically to a target folder and rolls delta adjustments up through LMDB ancestor chains.
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

        let is_same_file = is_same_file_path(&src_path, &dst_path);
        if is_same_file && old_folder_id == target_folder_id {
            return Ok(source_record);
        }

        if dst_path.exists() && !overwrite_existing {
            return Err(LibraryError::FileOperationAlreadyExists(validated_name));
        }

        if src_path.exists() {
            robust_move_file(&src_path, &dst_path)?;

            if let Ok(fs_meta) = std::fs::metadata(&dst_path) {
                source_record.modification_time = fs_meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(source_record.modification_time);

                source_record.creation_time = fs_meta
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(source_record.creation_time);

                source_record.file_size = fs_meta.len();
            }
        }

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
                self.db.update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
                self.db.media.delete(&mut wtxn, &old_dst_id)?;
                self.db.thumbnails.delete(&mut wtxn, &old_dst_id)?;
                self.db.video_previews.delete(&mut wtxn, &old_dst_id)?;
                self.db.tags_by_media.delete(&mut wtxn, &old_dst_id)?;

                target_media_ids.retain(|&id| id != old_dst_id);
                self.db.media_by_folder.put(&mut wtxn, &target_folder_id, &target_media_ids)?;

                // Overwritten file in target folder is removed
                FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, target_folder_id, -1)?;
            }
        }

        source_record.folder_id = target_folder_id;
        source_record.filename = validated_name.clone();
        source_record.is_deleted = false;
        self.db.media.put(&mut wtxn, &media_id, &source_record)?;

        if old_folder_id != target_folder_id {
            if let Some(mut old_list) = self.db.media_by_folder.get(&wtxn, &old_folder_id)? {
                old_list.retain(|&id| id != media_id);
                self.db.media_by_folder.put(&mut wtxn, &old_folder_id, &old_list)?;
            }

            let mut new_list = self
                .db
                .media_by_folder
                .get(&wtxn, &target_folder_id)?
                .unwrap_or_default();
            if !new_list.contains(&media_id) {
                new_list.push(media_id);
                self.db.media_by_folder.put(&mut wtxn, &target_folder_id, &new_list)?;
            }

            // Atomic delta adjustments: -1 for source folder chain, +1 for target folder chain
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, old_folder_id, -1)?;
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, target_folder_id, 1)?;
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

    /// Copies a media file physically to a target folder and rolls delta adjustments up through LMDB ancestor chains.
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

        let is_same_file = is_same_file_path(&src_path, &dst_path);
        if is_same_file {
            if overwrite_existing {
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

        let mut file_size = source_record.file_size;
        let mut mtime = source_record.modification_time;
        let mut ctime = source_record.creation_time;

        if src_path.exists() {
            std::fs::copy(&src_path, &dst_path)?;

            if let Ok(fs_meta) = std::fs::metadata(&dst_path) {
                file_size = fs_meta.len();
                mtime = fs_meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(source_record.modification_time);

                ctime = fs_meta
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(source_record.creation_time);
            }
        }

        let mut target_media_ids = self
            .db
            .media_by_folder
            .get(&wtxn, &target_folder_id)?
            .unwrap_or_default();

        let mut overwritten_existing_file = false;
        if overwrite_existing && dst_path.exists() {
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
                self.db.update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
                self.db.media.delete(&mut wtxn, &old_dst_id)?;
                self.db.thumbnails.delete(&mut wtxn, &old_dst_id)?;
                self.db.video_previews.delete(&mut wtxn, &old_dst_id)?;
                self.db.tags_by_media.delete(&mut wtxn, &old_dst_id)?;

                target_media_ids.retain(|&id| id != old_dst_id);
                overwritten_existing_file = true;
            }
        }

        let new_media_id = self.db.next_id(&mut wtxn, SEQ_MEDIA_ID)?;
        let new_record = MediaRecord {
            id: new_media_id,
            folder_id: target_folder_id,
            filename: validated_name.clone(),
            file_hash: source_record.file_hash,
            file_size,
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
            is_corrupted: source_record.is_corrupted,
            source: source_record.source.clone(),
        };

        self.db.media.put(&mut wtxn, &new_media_id, &new_record)?;
        self.db.update_media_hash_index(&mut wtxn, new_media_id, None, Some(&new_record.file_hash))?;

        if !target_media_ids.contains(&new_media_id) {
            target_media_ids.push(new_media_id);
            self.db.media_by_folder.put(&mut wtxn, &target_folder_id, &target_media_ids)?;
        }

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

        if let Some(thumb_loc) = self.db.thumbnails.get(&wtxn, &media_id)? {
            self.db.thumbnails.put(&mut wtxn, &new_media_id, &thumb_loc)?;
        }
        if let Some(prev_rec) = self.db.video_previews.get(&wtxn, &media_id)? {
            self.db.video_previews.put(&mut wtxn, &new_media_id, &prev_rec)?;
        }

        // If not overwriting, target folder gains +1 media file rolled up through ancestors
        if !overwritten_existing_file {
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, target_folder_id, 1)?;
        }

        wtxn.commit()?;

        tracing::info!(
            source_media_id = media_id,
            new_media_id = new_media_id,
            from_folder = %LogObscurer::folder(Some(source_record.folder_id), &source_folder.path),
            to_folder = %LogObscurer::folder(Some(target_folder_id), &target_folder.path),
            filename = %LogObscurer::filename(&validated_name),
            elapsed_ms = t0.elapsed().as_millis(),
            "[FileOperations: Copy] Cloned media instance committed to LMDB"
        );

        Ok(new_record)
    }

    /// Purges media records completely from LMDB storage and adjusts folder count rollups.
    pub fn purge_media_from_db(&self, media_ids: &[MediaId]) -> Result<usize, LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;
        let mut purged_count = 0;
        let mut folder_deltas: std::collections::HashMap<FolderId, i64> = std::collections::HashMap::new();

        for &mid in media_ids {
            if let Some(rec) = self.db.media.get(&wtxn, &mid)? {
                self.db.update_media_hash_index(&mut wtxn, mid, Some(&rec.file_hash), None)?;
                self.db.media.delete(&mut wtxn, &mid)?;
                self.db.thumbnails.delete(&mut wtxn, &mid)?;
                self.db.video_previews.delete(&mut wtxn, &mid)?;

                if let Some(tags) = self.db.tags_by_media.get(&wtxn, &mid)? {
                    for tag_id in tags {
                        if let Some(mut t_media) = self.db.media_by_tag.get(&wtxn, &tag_id)? {
                            t_media.retain(|&id| id != mid);
                            if t_media.is_empty() {
                                self.db.media_by_tag.delete(&mut wtxn, &tag_id)?;
                            } else {
                                self.db.media_by_tag.put(&mut wtxn, &tag_id, &t_media)?;
                            }
                        }
                    }
                    self.db.tags_by_media.delete(&mut wtxn, &mid)?;
                }

                if let Some(mut folder_media) = self.db.media_by_folder.get(&wtxn, &rec.folder_id)? {
                    folder_media.retain(|&id| id != mid);
                    self.db.media_by_folder.put(&mut wtxn, &rec.folder_id, &folder_media)?;
                }

                *folder_deltas.entry(rec.folder_id).or_insert(0) -= 1;
                purged_count += 1;
            }
        }

        // Adjust counts and roll up through ancestors in the same transaction
        for (fid, delta) in folder_deltas {
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, fid, delta)?;
        }

        wtxn.commit()?;
        tracing::info!(purged_count, "[FileOperations: PurgeMedia] Purged media records and adjusted folder rollups in LMDB");
        Ok(purged_count)
    }

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
}
