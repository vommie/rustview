use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{MediaId, MediaRecord};
use std::path::Path;

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
        let trimmed_name = new_filename.trim();

        if trimmed_name.is_empty()
            || trimmed_name.contains('/')
            || trimmed_name.contains('\\')
            || trimmed_name.contains('\0')
        {
            return Err(LibraryError::FileOperationInvalidFilename(
                trimmed_name.to_string(),
            ));
        }

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

        let folder_path = Path::new(&folder.path);
        let old_path = folder_path.join(&record.filename);
        let new_path = folder_path.join(trimmed_name);

        if old_path == new_path {
            return Ok(record);
        }

        if new_path.exists() {
            return Err(LibraryError::FileOperationAlreadyExists(
                trimmed_name.to_string(),
            ));
        }

        tracing::info!(
            media_id = media_id,
            old_path = ?old_path,
            new_path = ?new_path,
            "Executing physical file rename operation..."
        );

        // 1. Filesystem rename
        std::fs::rename(&old_path, &new_path)?;

        // 2. Refresh modification timestamp
        let fs_meta = std::fs::metadata(&new_path)?;
        let mtime = fs_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(record.modification_time);

        record.filename = trimmed_name.to_string();
        record.modification_time = mtime;

        // 3. Persist updated MediaRecord in LMDB
        self.db.media.put(&mut wtxn, &media_id, &record)?;

        wtxn.commit()?;

        tracing::info!(
            media_id = media_id,
            new_filename = %trimmed_name,
            "Committed file rename operation to LMDB storage successfully"
        );

        Ok(record)
    }
}
