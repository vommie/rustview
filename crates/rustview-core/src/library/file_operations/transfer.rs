//! Moving and copying media between folders, including cross-device moves and conflict checks.

use super::{FileOperations, FilenameValidator, is_same_file_path};
use crate::library::error::LibraryError;
use crate::library::folders::FolderManager;
use crate::library::models::{FileConflictReport, FileTransferKind, FolderId, MediaId, MediaRecord};
use crate::library::schema::SEQ_MEDIA_ID;
use crate::settings::LogObscurer;
use crate::util::cast::SaturatingAs;
use std::path::Path;
use std::time::Instant;

/// Moves a symbolic link to `dst` so that it keeps pointing at the same file: a relative target is made absolute first (a plain rename
/// would leave it dangling in the new folder). The file the link points to is never touched.
pub(super) fn move_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    let target = std::fs::read_link(src)?;
    let absolute = if target.is_absolute() {
        target
    } else {
        src.parent().unwrap_or(Path::new("/")).join(target)
    };
    std::os::unix::fs::symlink(&absolute, dst)?;
    if let Err(e) = std::fs::remove_file(src) {
        let _ = std::fs::remove_file(dst);
        return Err(e);
    }
    Ok(())
}

pub fn robust_move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(src).is_ok_and(|m| m.file_type().is_symlink()) {
        if dst.exists() || std::fs::symlink_metadata(dst).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "the destination exists already",
            ));
        }
        return move_symlink(src, dst);
    }
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
        // Only a move across file systems is carried out by copying; any other failure (no permission, a missing folder, ...)
        // is reported as it is and leaves both sides untouched.
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            tracing::info!(
                src = %LogObscurer::path(src),
                dst = %LogObscurer::path(dst),
                error = %err,
                "[FileOperations: FS Move] Physical rename failed (EXDEV). Executing copy + remove fallback..."
            );
            let t_copy = Instant::now();
            let bytes = std::fs::copy(src, dst)?;
            let copy_dur = t_copy.elapsed();
            if let Err(remove_err) = std::fs::remove_file(src) {
                // The original cannot be removed (read-only folder): do not leave a second copy behind.
                let _ = std::fs::remove_file(dst);
                return Err(remove_err);
            }
            tracing::info!(
                bytes_copied = bytes,
                copy_time_ms = copy_dur.as_millis(),
                total_time_ms = t0.elapsed().as_millis(),
                "[FileOperations: FS Move] Cross-filesystem move completed successfully"
            );
            Ok(())
        }
        Err(err) => Err(err),
    }
}

impl FileOperations<'_> {
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

        if transfer_kind == FileTransferKind::Move
            && (source_record.folder_id == target_folder_id || is_same_file_path(&src_path, &dst_path))
        {
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
                    .map_or(source_record.modification_time, |d| d.as_secs().saturating_as::<i64>());

                source_record.creation_time = fs_meta
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(source_record.creation_time, |d| d.as_secs().saturating_as::<i64>());

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
                self.db
                    .update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
                self.db.media.delete(&mut wtxn, &old_dst_id)?;
                self.db.thumbnails.delete(&mut wtxn, &old_dst_id)?;
                self.db.video_previews.delete(&mut wtxn, &old_dst_id)?;
                self.db.tags_by_media.delete(&mut wtxn, &old_dst_id)?;

                target_media_ids.retain(|&id| id != old_dst_id);
                self.db
                    .media_by_folder
                    .put(&mut wtxn, &target_folder_id, &target_media_ids)?;

                // Overwritten file in target folder is removed
                FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, target_folder_id, -1)?;
            }
        }

        source_record.folder_id = target_folder_id;
        source_record.filename.clone_from(&validated_name);
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
            }
            return Err(LibraryError::FileOperationAlreadyExists(format!(
                "Cannot copy file onto itself in the same directory without renaming: {validated_name}"
            )));
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
                    .map_or(source_record.modification_time, |d| d.as_secs().saturating_as::<i64>());

                ctime = fs_meta
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(source_record.creation_time, |d| d.as_secs().saturating_as::<i64>());
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
                self.db
                    .update_media_hash_index(&mut wtxn, old_dst_id, Some(&old_hash), None)?;
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
        self.db
            .update_media_hash_index(&mut wtxn, new_media_id, None, Some(&new_record.file_hash))?;

        if !target_media_ids.contains(&new_media_id) {
            target_media_ids.push(new_media_id);
            self.db
                .media_by_folder
                .put(&mut wtxn, &target_folder_id, &target_media_ids)?;
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
}
