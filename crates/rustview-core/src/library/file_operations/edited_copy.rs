//! A picture that was edited and saved as a new file ("Save As") joins the library at once, without a scan of its folder: one
//! record from the file itself (size, dates, hash, pixel size), with the rating and the tags of the picture it was made from,
//! like a copy. A file of that name that the library knew already (the export replaced it) keeps its record and is refreshed.

use super::FileOperations;
use crate::library::error::LibraryError;
use crate::library::folders::FolderManager;
use crate::library::models::{FolderId, MediaId, MediaRecord};
use crate::library::refresh_media_files::MediaItemRefresher;
use crate::library::schema::SEQ_MEDIA_ID;
use crate::settings::LogObscurer;
use std::path::Path;

impl FileOperations<'_> {
    /// Adds the file `filename` of folder `folder_id` (written just now) as a medium made from `source_id`. Returns its record.
    pub fn index_edited_copy(
        &self,
        source_id: MediaId,
        folder_id: FolderId,
        filename: &str,
    ) -> Result<MediaRecord, LibraryError> {
        let db = self.db;
        let (folder, existing, source) = {
            let rtxn = db.env.read_txn()?;
            let folder = db
                .folders
                .get(&rtxn, &folder_id)?
                .ok_or(LibraryError::FolderNotFound(folder_id))?;
            let mut existing = None;
            for id in db.media_by_folder.get(&rtxn, &folder_id)?.unwrap_or_default() {
                if let Some(record) = db.media.get(&rtxn, &id)?
                    && record.filename == filename
                {
                    existing = Some(id);
                    break;
                }
            }
            (folder, existing, db.media.get(&rtxn, &source_id)?)
        };
        let refresher = MediaItemRefresher::new(db);
        if let Some(id) = existing {
            tracing::info!(
                media_id = id,
                "[FileOperations: EditedCopy] The new file replaced a known one; its record is refreshed"
            );
            return refresher.refresh_media(id);
        }

        let path = Path::new(&folder.path).join(filename);
        let attributes = refresher.inspect_physical_file(&path)?;
        let mut wtxn = db.env.write_txn()?;
        let id = db.next_id(&mut wtxn, SEQ_MEDIA_ID)?;
        let record = MediaRecord {
            id,
            folder_id,
            filename: filename.to_string(),
            file_hash: attributes.file_hash,
            file_size: attributes.file_size,
            modification_time: attributes.modification_time,
            creation_time: attributes.creation_time,
            duration_ms: attributes.raw_metadata.duration_ms,
            media_type: attributes.raw_metadata.media_type,
            format: attributes.raw_metadata.format,
            width: attributes.raw_metadata.width,
            height: attributes.raw_metadata.height,
            rating: source.as_ref().map_or(0, |s| s.rating),
            last_seen: 1,
            is_deleted: false,
            is_corrupted: attributes.is_corrupted,
            source: None,
        };
        db.media.put(&mut wtxn, &id, &record)?;
        db.update_media_hash_index(&mut wtxn, id, None, Some(&record.file_hash))?;
        let mut in_folder = db.media_by_folder.get(&wtxn, &folder_id)?.unwrap_or_default();
        in_folder.push(id);
        db.media_by_folder.put(&mut wtxn, &folder_id, &in_folder)?;
        if let Some(tags) = db.tags_by_media.get(&wtxn, &source_id)? {
            db.tags_by_media.put(&mut wtxn, &id, &tags)?;
            for tag in &tags {
                let mut tagged = db.media_by_tag.get(&wtxn, tag)?.unwrap_or_default();
                if !tagged.contains(&id) {
                    tagged.push(id);
                    db.media_by_tag.put(&mut wtxn, tag, &tagged)?;
                }
            }
        }
        FolderManager::adjust_folder_media_count_txn(db, &mut wtxn, folder_id, 1)?;
        wtxn.commit()?;
        tracing::info!(
            source_media_id = source_id,
            media = %LogObscurer::media(Some(id), filename),
            width = record.width,
            height = record.height,
            "[FileOperations: EditedCopy] The new file joined the library"
        );
        Ok(record)
    }
}
