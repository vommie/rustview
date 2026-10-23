use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{MediaId, MediaRecord};
use crate::media::{FileHash, MediaFormat, MediaHasher, MediaType, MetadataExtractor, RawMetadata};
use crate::settings::LogObscurer;
use std::path::{Path, PathBuf};

/// Extracted file inspection metrics from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshedFileAttributes {
    pub file_size: u64,
    pub modification_time: i64,
    pub creation_time: i64,
    pub file_hash: FileHash,
    pub raw_metadata: RawMetadata,
    pub is_corrupted: bool,
}

/// Subsystem responsible for re-inspecting media files on disk and synchronizing metadata and hashes with LMDB storage.
pub struct MediaItemRefresher<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> MediaItemRefresher<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Re-reads the media file from disk, recalculates its MD5 checksum and container metadata,
    /// and updates the database record while strictly preserving ratings, source, and tag associations.
    pub fn refresh_media(&self, media_id: MediaId) -> Result<MediaRecord, LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        let (record, file_path) = self.resolve_media_path(&wtxn, media_id)?;
        let attributes = self.inspect_physical_file(&file_path)?;

        let updated_record =
            self.commit_refreshed_record(&mut wtxn, media_id, record, attributes)?;

        wtxn.commit()?;

        tracing::info!(
            media = %LogObscurer::media(Some(media_id), &updated_record.filename),
            file_size = updated_record.file_size,
            width = updated_record.width,
            height = updated_record.height,
            duration_ms = updated_record.duration_ms,
            rating = updated_record.rating,
            source = ?updated_record.source,
            is_corrupted = updated_record.is_corrupted,
            "Committed media metadata refresh to LMDB storage successfully"
        );

        Ok(updated_record)
    }

    /// Resolves existing MediaRecord and its absolute filesystem path.
    fn resolve_media_path(
        &self,
        txn: &heed::RoTxn,
        media_id: MediaId,
    ) -> Result<(MediaRecord, PathBuf), LibraryError> {
        let record = self
            .db
            .media
            .get(txn, &media_id)?
            .ok_or(LibraryError::MediaNotFound(media_id))?;

        let folder = self
            .db
            .folders
            .get(txn, &record.folder_id)?
            .ok_or(LibraryError::FolderNotFound(record.folder_id))?;

        let file_path = Path::new(&folder.path).join(&record.filename);

        if !file_path.exists() {
            return Err(LibraryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Media file not found on disk: {:?}", file_path),
            )));
        }

        Ok((record, file_path))
    }

    /// Inspects physical file on disk: reads metadata, hashes file content, and extracts container dimensions & duration.
    pub fn inspect_physical_file(
        &self,
        file_path: &Path,
    ) -> Result<RefreshedFileAttributes, LibraryError> {
        tracing::debug!(
            file = %LogObscurer::path(file_path),
            "[MediaItemRefresher] Inspecting physical file on disk..."
        );

        let fs_meta = std::fs::metadata(file_path)?;
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

        let is_zero_byte = file_size == 0;
        let format = MediaFormat::from_path(file_path);
        let media_type = format.media_type().unwrap_or(MediaType::Image);

        let (file_hash, hash_ok) = if is_zero_byte {
            (MediaHasher::hash_bytes(b""), true)
        } else {
            match MediaHasher::hash_file(file_path) {
                Ok(h) => (h, true),
                Err(_) => (MediaHasher::hash_bytes(file_path.to_string_lossy().as_bytes()), false),
            }
        };

        let (raw_metadata, meta_ok) = if is_zero_byte {
            (
                RawMetadata {
                    file_size: 0,
                    modification_time,
                    creation_time,
                    duration_ms: 0,
                    width: 0,
                    height: 0,
                    media_type,
                    format,
                },
                false,
            )
        } else {
            match MetadataExtractor::extract(file_path) {
                Ok(m) => (m, true),
                Err(_) => (
                    RawMetadata {
                        file_size,
                        modification_time,
                        creation_time,
                        duration_ms: 0,
                        width: 0,
                        height: 0,
                        media_type,
                        format,
                    },
                    false,
                ),
            }
        };

        let is_corrupted = is_zero_byte || !hash_ok || !meta_ok || (raw_metadata.width == 0 && raw_metadata.height == 0);

        Ok(RefreshedFileAttributes {
            file_size,
            modification_time,
            creation_time,
            file_hash,
            raw_metadata,
            is_corrupted,
        })
    }

    /// Updates LMDB database record and secondary hash indices atomically.
    fn commit_refreshed_record(
        &self,
        wtxn: &mut heed::RwTxn,
        media_id: MediaId,
        mut record: MediaRecord,
        attributes: RefreshedFileAttributes,
    ) -> Result<MediaRecord, LibraryError> {
        if attributes.file_hash != record.file_hash {
            self.db.update_media_hash_index(
                wtxn,
                media_id,
                Some(&record.file_hash),
                Some(&attributes.file_hash),
            )?;
        }

        record.file_hash = attributes.file_hash;
        record.file_size = attributes.file_size;
        record.modification_time = attributes.modification_time;
        record.creation_time = attributes.creation_time;
        record.duration_ms = attributes.raw_metadata.duration_ms;
        record.media_type = attributes.raw_metadata.media_type;
        record.format = attributes.raw_metadata.format;
        record.width = attributes.raw_metadata.width;
        record.height = attributes.raw_metadata.height;
        record.is_deleted = false;
        record.is_corrupted = attributes.is_corrupted;

        self.db.media.put(wtxn, &media_id, &record)?;

        Ok(record)
    }
}
