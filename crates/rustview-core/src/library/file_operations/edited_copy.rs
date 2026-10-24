//! A picture that was edited and saved as a new file ("Save As") joins the library at once, without a scan of its folder: one
//! record from the file itself (size, dates, hash, pixel size), with the rating and the tags of the picture it was made from
//! when the user keeps them (like a copy). A file of that name that the library knew already (the export replaced it) keeps
//! its record and is refreshed; kept assignments replace its own then.

use super::FileOperations;
use crate::library::db::LibraryDatabase;
use crate::library::error::LibraryError;
use crate::library::folders::FolderManager;
use crate::library::models::{FolderId, MediaId, MediaRecord};
use crate::library::refresh_media_files::MediaItemRefresher;
use crate::library::schema::SEQ_MEDIA_ID;
use crate::settings::LogObscurer;
use std::path::Path;

impl FileOperations<'_> {
    /// Adds the file `filename` of folder `folder_id` (written just now) as a medium made from `source_id`; with
    /// `keep_assignments` it gets the rating and the tags of `source_id`. Returns its record.
    pub fn index_edited_copy(
        &self,
        source_id: MediaId,
        folder_id: FolderId,
        filename: &str,
        keep_assignments: bool,
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
                keep_assignments,
                "[FileOperations: EditedCopy] The new file replaced a known one; its record is refreshed"
            );
            let mut record = refresher.refresh_media(id)?;
            if keep_assignments && id != source_id {
                let mut wtxn = db.env.write_txn()?;
                if let Some(updated) = copy_assignments(db, &mut wtxn, source_id, id)? {
                    record = updated;
                }
                wtxn.commit()?;
            }
            return Ok(record);
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
            rating: 0,
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
        let record = match source.filter(|_| keep_assignments) {
            Some(_) => copy_assignments(db, &mut wtxn, source_id, id)?.unwrap_or(record),
            None => record,
        };
        FolderManager::adjust_folder_media_count_txn(db, &mut wtxn, folder_id, 1)?;
        wtxn.commit()?;
        tracing::info!(
            source_media_id = source_id,
            media = %LogObscurer::media(Some(id), filename),
            width = record.width,
            height = record.height,
            keep_assignments,
            "[FileOperations: EditedCopy] The new file joined the library"
        );
        Ok(record)
    }
}

/// Gives `target` the rating and exactly the tags of `source` (its own tags go). Returns the updated record of `target`.
fn copy_assignments(
    db: &LibraryDatabase,
    wtxn: &mut heed::RwTxn,
    source: MediaId,
    target: MediaId,
) -> Result<Option<MediaRecord>, LibraryError> {
    let rating = db.media.get(wtxn, &source)?.map_or(0, |s| s.rating);
    let tags = db.tags_by_media.get(wtxn, &source)?.unwrap_or_default();
    for old in db.tags_by_media.get(wtxn, &target)?.unwrap_or_default() {
        if !tags.contains(&old) {
            let mut tagged = db.media_by_tag.get(wtxn, &old)?.unwrap_or_default();
            tagged.retain(|id| *id != target);
            if tagged.is_empty() {
                db.media_by_tag.delete(wtxn, &old)?;
            } else {
                db.media_by_tag.put(wtxn, &old, &tagged)?;
            }
        }
    }
    if tags.is_empty() {
        db.tags_by_media.delete(wtxn, &target)?;
    } else {
        db.tags_by_media.put(wtxn, &target, &tags)?;
    }
    for tag in &tags {
        let mut tagged = db.media_by_tag.get(wtxn, tag)?.unwrap_or_default();
        if !tagged.contains(&target) {
            tagged.push(target);
            db.media_by_tag.put(wtxn, tag, &tagged)?;
        }
    }
    let Some(mut record) = db.media.get(wtxn, &target)? else {
        return Ok(None);
    };
    record.rating = rating;
    db.media.put(wtxn, &target, &record)?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use crate::library::Library;
    use crate::library::models::{FolderId, MediaRecord};
    use std::path::Path;

    fn picture(path: &Path) {
        image::RgbImage::from_pixel(4, 3, image::Rgb([200, 30, 30]))
            .save(path)
            .expect("write picture");
    }

    /// A library with one folder holding `source.png`, rated 4 with one tag.
    fn library_with_source(store: &Path, folder: &Path) -> (Library, MediaRecord, FolderId) {
        let library = Library::open(store).expect("library");
        let folder_record = library.folders().add_folder(folder).expect("folder");
        picture(&folder.join("source.png"));
        let source = library
            .file_operations()
            .index_edited_copy(0, folder_record.id, "source.png", false)
            .expect("index source");
        library.media().set_rating(source.id, 4).expect("rate");
        let tag = library.tags().create_tag("holiday", None).expect("tag");
        library.tags().assign_tag(source.id, tag.id).expect("assign");
        (library, source, folder_record.id)
    }

    #[test]
    fn a_copy_takes_the_rating_and_tags_only_when_they_are_kept() {
        let (store, disk) = (tempfile::tempdir().expect("store"), tempfile::tempdir().expect("disk"));
        let (library, source, folder) = library_with_source(store.path(), disk.path());
        picture(&disk.path().join("kept.png"));
        picture(&disk.path().join("plain.png"));
        let ops = library.file_operations();

        let kept = ops
            .index_edited_copy(source.id, folder, "kept.png", true)
            .expect("kept");
        assert_eq!(kept.rating, 4);
        assert_eq!(library.tags().get_tags_for_media(kept.id).expect("tags").len(), 1);

        let plain = ops
            .index_edited_copy(source.id, folder, "plain.png", false)
            .expect("plain");
        assert_eq!(plain.rating, 0);
        assert!(library.tags().get_tags_for_media(plain.id).expect("tags").is_empty());
    }

    #[test]
    fn a_replaced_known_file_gets_the_kept_assignments_instead_of_its_own() {
        let (store, disk) = (tempfile::tempdir().expect("store"), tempfile::tempdir().expect("disk"));
        let (library, source, folder) = library_with_source(store.path(), disk.path());
        picture(&disk.path().join("other.png"));
        let ops = library.file_operations();
        let other = ops.index_edited_copy(0, folder, "other.png", false).expect("other");
        let own = library.tags().create_tag("own", None).expect("tag");
        library.tags().assign_tag(other.id, own.id).expect("assign");

        let replaced = ops
            .index_edited_copy(source.id, folder, "other.png", true)
            .expect("replace");
        assert_eq!(replaced.id, other.id, "the record stays");
        assert_eq!(replaced.rating, 4);
        let names: Vec<String> = library
            .tags()
            .get_tags_for_media(other.id)
            .expect("tags")
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["holiday"]);
        assert!(
            library.tags().get_media_for_tag(own.id).expect("tagged").is_empty(),
            "the tag index forgets the replaced assignment"
        );
    }
}
