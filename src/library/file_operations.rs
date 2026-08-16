use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderRecord, MediaId, MediaRecord};
use super::refresh_media_files::MediaItemRefresher;
use std::path::{Path, PathBuf};

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

        if paths.old_path == paths.new_path {
            return Ok(record);
        }

        self.verify_destination_available(&paths)?;

        tracing::info!(
            media_id = media_id,
            old_path = ?paths.old_path,
            new_path = ?paths.new_path,
            "Executing physical file rename operation..."
        );

        let mtime = self.perform_fs_rename(&paths, record.modification_time)?;
        let updated_record = self.commit_rename_to_db(&mut wtxn, media_id, record, &paths, mtime)?;

        wtxn.commit()?;

        tracing::info!(
            media_id = media_id,
            new_filename = %paths.validated_name,
            "Committed file rename operation to LMDB storage successfully"
        );

        Ok(updated_record)
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
