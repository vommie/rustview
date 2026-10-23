//! Keeps the media counts of the folder tree in step with the formats the user has switched on.
//!
//! A folder's count is the number of media files it shows. Switching a format off hides its records, so every folder
//! loses as many files as it holds records of that format; switching it on gives them back. The counts are changed by
//! exactly that difference (never recomputed from the library): a folder whose files were never indexed keeps the count of
//! its tree scan, and switching a format off and on again returns every count to where it was.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folders::FolderManager;
use super::models::FolderId;
use crate::media::{FormatMask, MediaFormat};
use std::collections::HashMap;

/// What a change of the policy did to the folder counts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FormatCountReport {
    pub folders_changed: usize,
    /// Records that became hidden.
    pub hidden: u64,
    /// Records that are shown again.
    pub restored: u64,
}

pub struct FolderFormatCounts<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderFormatCounts<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Moves the folder counts from what `old` shows to what `new` shows. One pass over the media table plus one write
    /// transaction; call it from a worker thread.
    pub fn apply_change(&self, old: FormatMask, new: FormatMask) -> Result<FormatCountReport, LibraryError> {
        let changed: HashMap<MediaFormat, i64> = MediaFormat::all()
            .filter(|f| old.shows(*f) != new.shows(*f))
            .map(|f| (f, if new.shows(f) { 1 } else { -1 }))
            .collect();
        let mut report = FormatCountReport::default();
        if changed.is_empty() {
            return Ok(report);
        }

        let mut per_folder: HashMap<FolderId, i64> = HashMap::new();
        {
            let rtxn = self.db.env.read_txn()?;
            for entry in self.db.media.iter(&rtxn)? {
                let (_, record) = entry?;
                if record.is_deleted {
                    continue;
                }
                if let Some(step) = changed.get(&record.format) {
                    *per_folder.entry(record.folder_id).or_default() += step;
                    if *step > 0 {
                        report.restored += 1;
                    } else {
                        report.hidden += 1;
                    }
                }
            }
        }

        let mut wtxn = self.db.env.write_txn()?;
        for (folder_id, delta) in per_folder.into_iter().filter(|(_, d)| *d != 0) {
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, folder_id, delta)?;
            report.folders_changed += 1;
        }
        wtxn.commit()?;
        Ok(report)
    }
}
