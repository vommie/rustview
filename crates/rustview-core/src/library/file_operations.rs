use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folders::FolderManager;
use super::models::{FileConflictItemDetails, FolderId, MediaId, MediaRecord};
use super::refresh_media_files::MediaItemRefresher;
use crate::media::{MediaFormat, MediaHasher, MetadataExtractor};
use crate::util::cast::SaturatingAs;
use std::path::Path;

mod edited_copy;
mod move_observer;
mod rename;
mod transfer;
pub use move_observer::set_move_observer;
pub use rename::FilenameValidator;
pub use rename::is_same_file_path;
pub use transfer::robust_move_file;

pub struct FileOperations<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FileOperations<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
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
                .map_or(0, |d| d.as_secs().saturating_as::<i64>());
            let ctime = fs_meta
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(mtime, |d| d.as_secs().saturating_as::<i64>());
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

    /// Purges media records completely from LMDB storage and adjusts folder count rollups.
    pub fn purge_media_from_db(&self, media_ids: &[MediaId]) -> Result<usize, LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;
        let mut purged_count = 0;
        let mut folder_deltas: std::collections::HashMap<FolderId, i64> = std::collections::HashMap::new();

        for &mid in media_ids {
            if let Some(rec) = self.db.media.get(&wtxn, &mid)? {
                self.db
                    .update_media_hash_index(&mut wtxn, mid, Some(&rec.file_hash), None)?;
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
        tracing::info!(
            purged_count,
            "[FileOperations: PurgeMedia] Purged media records and adjusted folder rollups in LMDB"
        );
        Ok(purged_count)
    }

    pub fn refresh_media(&self, media_id: MediaId) -> Result<MediaRecord, LibraryError> {
        let refresher = MediaItemRefresher::new(self.db);
        refresher.refresh_media(media_id)
    }
}
