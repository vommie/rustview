//! `move-media`: a media record follows its file to another path. The caller has moved the file already (`RustCut` moves its
//! rendered files itself, with progress and verification); only the record changes its folder and name. It keeps its id and
//! with it the tags, the rating, the hash, the source, the thumbnail and the video preview.

use super::error::ApiError;
use super::handlers::{ApiHandler, purge_record_txn, split_folder_and_filename};
use super::models::ApiMoveMediaResponse;
use crate::library::Library;
use crate::library::folders::FolderManager;
use crate::library::models::{FolderId, MediaId, MediaRecord};
use crate::media::MediaHasher;
use crate::util::cast::SaturatingAs;
use std::path::Path;

/// What a `move-media` call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The record now lives at the new path.
    Moved,
    /// There is no record at the old path, but one at the new path: an earlier call moved it.
    AlreadyMoved,
    /// Both paths name the same record.
    Unchanged,
    /// There is a record at neither path.
    NotFound,
}

impl Outcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Moved => "moved",
            Self::AlreadyMoved => "already_moved",
            Self::Unchanged => "unchanged",
            Self::NotFound => "not_found",
        }
    }
}

/// The record named `filename` in the folder `folder_path`, if the folder and the record exist (names compare without case, as
/// everywhere in the API).
fn find_record(lib: &Library, folder_path: &Path, filename: &str) -> Result<Option<(FolderId, MediaRecord)>, ApiError> {
    let Some(folder) = lib.folders().get_folder_by_path(folder_path)? else {
        return Ok(None);
    };
    let rtxn = lib.raw_db().env.read_txn()?;
    let media_ids = lib.raw_db().media_by_folder.get(&rtxn, &folder.id)?.unwrap_or_default();
    for mid in media_ids {
        if let Some(rec) = lib.raw_db().media.get(&rtxn, &mid)?
            && rec.filename.eq_ignore_ascii_case(filename)
        {
            return Ok(Some((folder.id, rec)));
        }
    }
    Ok(None)
}

fn unix_secs(time: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().saturating_as::<i64>())
}

impl ApiHandler {
    /// Moves the record of `from` to `to`. A different record at `to` is refused unless `replace` (the file there was overwritten:
    /// its record goes, with its tags). Answers exit code 0 when the record is at `to` afterward, 2 when there is none at either
    /// path; a refusal is an error (exit code 1).
    pub fn move_media(lib: &Library, from: &Path, to: &Path, replace: bool, json: bool) -> Result<i32, ApiError> {
        let (from_folder, from_name) = split_folder_and_filename(from)?;
        let (to_folder, to_name) = split_folder_and_filename(to)?;
        let source = find_record(lib, &from_folder, &from_name)?;
        let target = find_record(lib, &to_folder, &to_name)?;

        let (outcome, record, replaced) = match (source, target) {
            (None, None) => (Outcome::NotFound, None, None),
            (None, Some((_, rec))) => (Outcome::AlreadyMoved, Some(rec), None),
            (Some((_, rec)), Some((_, there))) if there.id == rec.id => (Outcome::Unchanged, Some(rec), None),
            (Some(_), Some((_, there))) if !replace => {
                return Err(ApiError::InvalidArgument(format!(
                    "Media #{} is registered at the target path already; pass --replace to remove its record.",
                    there.id
                )));
            }
            (Some((old_folder, rec)), target) => {
                let replaced = target.map(|(folder, there)| (folder, there.id, there.file_hash));
                let rec = Self::relink(lib, old_folder, rec, &to_folder, &to_name, to, replaced)?;
                (Outcome::Moved, Some(rec), replaced.map(|(_, id, _)| id))
            }
        };

        let tags = match &record {
            Some(rec) => lib
                .tags()
                .get_tags_for_media(rec.id)?
                .into_iter()
                .map(|t| t.id)
                .collect(),
            None => Vec::new(),
        };
        let message = match outcome {
            Outcome::Moved => format!("Media record moved to '{}'.", to.display()),
            Outcome::AlreadyMoved => format!("The media record is at '{}' already.", to.display()),
            Outcome::Unchanged => "Both paths name the same media record; nothing changed.".to_string(),
            Outcome::NotFound => "Neither path is registered in the database.".to_string(),
        };
        let response = ApiMoveMediaResponse {
            success: true,
            action: outcome.as_str().to_string(),
            media_id: record.as_ref().map(|rec| rec.id),
            from: from.to_string_lossy().to_string(),
            to: to.to_string_lossy().to_string(),
            replaced_media_id: replaced,
            rating: record.as_ref().map(|rec| rec.rating),
            tags,
            file_hash: record.as_ref().map(|rec| MediaHasher::to_hex(&rec.file_hash)),
            source: record.as_ref().and_then(|rec| rec.source.clone()),
            message,
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else {
            println!("{}", response.message);
            if let Some(id) = response.media_id {
                println!("  • Media     : #{id}");
            }
            if let Some(id) = response.replaced_media_id {
                println!("  • Replaced  : #{id}");
            }
        }

        Ok(if outcome == Outcome::NotFound { 2 } else { 0 })
    }

    /// Gives `rec` its new folder and name in one transaction; `replaced` (folder, id, hash) is the record at the target that goes.
    fn relink(
        lib: &Library,
        old_folder: FolderId,
        mut rec: MediaRecord,
        to_folder: &Path,
        to_name: &str,
        to: &Path,
        replaced: Option<(FolderId, MediaId, crate::media::FileHash)>,
    ) -> Result<MediaRecord, ApiError> {
        let new_folder = match lib.folders().get_folder_by_path(to_folder)? {
            Some(folder) => folder.id,
            None => lib.folders().add_folder(to_folder)?.id,
        };
        let db = lib.raw_db();
        let mut wtxn = db.env.write_txn()?;
        let generation = db.next_generation(&mut wtxn)?;

        if let Some((folder, id, hash)) = replaced {
            purge_record_txn(lib, &mut wtxn, id, &hash)?;
            let mut list = db.media_by_folder.get(&wtxn, &folder)?.unwrap_or_default();
            list.retain(|&mid| mid != id);
            db.media_by_folder.put(&mut wtxn, &folder, &list)?;
            FolderManager::adjust_folder_media_count_txn(db, &mut wtxn, folder, -1)?;
        }

        if let Ok(meta) = std::fs::metadata(to) {
            rec.file_size = meta.len();
            rec.modification_time = unix_secs(meta.modified()).unwrap_or(rec.modification_time);
            rec.creation_time = unix_secs(meta.created()).unwrap_or(rec.creation_time);
        }
        rec.folder_id = new_folder;
        rec.filename = to_name.to_string();
        rec.is_deleted = false;
        rec.last_seen = generation;
        db.media.put(&mut wtxn, &rec.id, &rec)?;

        if old_folder != new_folder {
            let mut old_list = db.media_by_folder.get(&wtxn, &old_folder)?.unwrap_or_default();
            old_list.retain(|&mid| mid != rec.id);
            db.media_by_folder.put(&mut wtxn, &old_folder, &old_list)?;
            let mut new_list = db.media_by_folder.get(&wtxn, &new_folder)?.unwrap_or_default();
            if !new_list.contains(&rec.id) {
                new_list.push(rec.id);
                db.media_by_folder.put(&mut wtxn, &new_folder, &new_list)?;
            }
            FolderManager::adjust_folder_media_count_txn(db, &mut wtxn, old_folder, -1)?;
            FolderManager::adjust_folder_media_count_txn(db, &mut wtxn, new_folder, 1)?;
        }
        wtxn.commit()?;

        tracing::info!(
            media_id = rec.id,
            replaced = replaced.map(|(_, id, _)| id),
            "[CliApi: MoveMedia] Media record moved to its file's new path"
        );
        Ok(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn library() -> (tempfile::TempDir, Arc<Library>) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Arc::new(Library::open(&dir.path().join("library.lmdb")).unwrap());
        (dir, lib)
    }

    fn record_at(lib: &Library, path: &Path) -> Option<MediaRecord> {
        let (folder, name) = split_folder_and_filename(path).unwrap();
        find_record(lib, &folder, &name).unwrap().map(|(_, rec)| rec)
    }

    fn tag(lib: &Library, name: &str) -> u32 {
        lib.tags().create_tag(name, None).unwrap().id
    }

    #[test]
    fn a_record_keeps_its_id_tags_rating_hash_and_source() {
        let (dir, lib) = library();
        let (from, to) = (
            dir.path().join("out/clip.mkv"),
            dir.path().join("videos/2026/Clip Beach.mkv"),
        );
        let beach = tag(&lib, "Beach");
        let hash = "8f618d1ebd606025257057ee18536e64";
        ApiHandler::set_media(
            &lib,
            &from,
            Some(5),
            Some(vec![beach]),
            Some(hash),
            Some("RustCut".into()),
            true,
        )
        .unwrap();
        let before = record_at(&lib, &from).unwrap();
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, b"moved file").unwrap();

        assert_eq!(ApiHandler::move_media(&lib, &from, &to, false, true).unwrap(), 0);
        assert!(record_at(&lib, &from).is_none(), "nothing stays at the old path");
        let after = record_at(&lib, &to).unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.filename, "Clip Beach.mkv");
        assert_eq!(
            (after.rating, after.file_hash, after.source.clone()),
            (5, before.file_hash, Some("RustCut".into()))
        );
        assert_eq!(after.file_size, 10, "the facts of the file at the new path are read");
        let tags: Vec<u32> = lib
            .tags()
            .get_tags_for_media(after.id)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(tags, vec![beach]);
    }

    #[test]
    fn a_second_call_and_a_missing_record_are_answers() {
        let (dir, lib) = library();
        let (from, to) = (dir.path().join("a/x.mkv"), dir.path().join("b/x.mkv"));
        assert_eq!(ApiHandler::move_media(&lib, &from, &to, false, true).unwrap(), 2);
        ApiHandler::set_media(&lib, &from, Some(1), None, None, None, true).unwrap();
        assert_eq!(ApiHandler::move_media(&lib, &from, &to, false, true).unwrap(), 0);
        assert_eq!(
            ApiHandler::move_media(&lib, &from, &to, false, true).unwrap(),
            0,
            "an earlier move is reported, not refused"
        );
        assert_eq!(ApiHandler::move_media(&lib, &to, &to, false, true).unwrap(), 0);
        assert!(record_at(&lib, &to).is_some());
    }

    #[test]
    fn a_record_at_the_target_is_replaced_only_when_asked() {
        let (dir, lib) = library();
        let (from, to) = (dir.path().join("a/new.mkv"), dir.path().join("b/old.mkv"));
        let old_tag = tag(&lib, "Old");
        ApiHandler::set_media(&lib, &from, Some(4), None, None, None, true).unwrap();
        ApiHandler::set_media(&lib, &to, Some(2), Some(vec![old_tag]), None, None, true).unwrap();
        let moved = record_at(&lib, &from).unwrap().id;
        let replaced = record_at(&lib, &to).unwrap().id;

        assert!(ApiHandler::move_media(&lib, &from, &to, false, true).is_err());
        assert_eq!(record_at(&lib, &from).unwrap().id, moved, "a refusal changes nothing");
        assert_eq!(ApiHandler::move_media(&lib, &from, &to, true, true).unwrap(), 0);
        let now = record_at(&lib, &to).unwrap();
        assert_eq!((now.id, now.rating), (moved, 4));
        assert!(
            lib.raw_db()
                .media
                .get(&lib.raw_db().env.read_txn().unwrap(), &replaced)
                .unwrap()
                .is_none()
        );
        assert!(
            lib.tags().get_tags_for_media(moved).unwrap().is_empty(),
            "the replaced record's tags went with it"
        );
    }

    #[test]
    fn a_rename_in_the_same_folder_and_a_new_folder_both_work() {
        let (dir, lib) = library();
        let (a, b, c) = (
            dir.path().join("f/a.mkv"),
            dir.path().join("f/b.mkv"),
            dir.path().join("g/h/b.mkv"),
        );
        ApiHandler::set_media(&lib, &a, Some(3), None, None, None, true).unwrap();
        let id = record_at(&lib, &a).unwrap().id;
        ApiHandler::move_media(&lib, &a, &b, false, true).unwrap();
        assert_eq!(record_at(&lib, &b).unwrap().id, id);
        ApiHandler::move_media(&lib, &b, &c, false, true).unwrap();
        assert_eq!(record_at(&lib, &c).unwrap().id, id);
        assert!(record_at(&lib, &b).is_none());
    }
}
