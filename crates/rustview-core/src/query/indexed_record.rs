//! A library record as the Browse view keeps it in memory: the stored record plus the few values that sorting, filtering and
//! navigation read in tight loops.
//!
//! Everything the grid and the table *display* (formatted dates, sizes, dimensions, labels) is derived from the record when a
//! row is built (`ui::browse_window::RecordLabels`), so a folder of a million items does not hold ~13 formatted strings per
//! record. `new`, `set_record` and `refresh_derived` are the only places that compute the derived flags.

use super::folder_dir::FolderDir;
use crate::library::models::MediaRecord;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct IndexedMediaRecord {
    /// Position in the folder's record list (stable sequence number).
    pub index: usize,
    pub record: MediaRecord,
    pub folder: Arc<FolderDir>,
    /// Lower-cased file name, the key of the default sort order and of the text filter.
    pub filename_lower: String,
    pub tags_count: usize,
    pub ar_value: f32,
    pub is_corrupted: bool,
    pub is_deleted: bool,
    pub is_pending: bool,
    pub has_tags: bool,
    /// Hover preview frames of an animated picture are stored in the pack store. Not derived from the record: the folder load
    /// and the thumbnail pipeline set it.
    pub is_animated: bool,
}

impl IndexedMediaRecord {
    pub fn new(index: usize, record: MediaRecord, folder: Arc<FolderDir>, tags_count: usize) -> Self {
        let mut item = Self {
            index,
            filename_lower: record.filename.to_lowercase(),
            record,
            folder,
            tags_count,
            ar_value: 0.0,
            is_corrupted: false,
            is_deleted: false,
            is_pending: false,
            has_tags: false,
            is_animated: false,
        };
        item.refresh_derived();
        item
    }

    /// Recomputes the values derived from `record` and `tags_count`; call after changing `record` in place.
    pub fn refresh_derived(&mut self) {
        let r = &self.record;
        self.ar_value = if r.height > 0 {
            r.width as f32 / r.height as f32
        } else {
            0.0
        };
        self.is_pending = r.source.is_some() && (r.file_size == 0 || r.width == 0 || r.height == 0 || r.is_corrupted);
        self.is_corrupted = !self.is_pending && r.is_corrupted;
        self.is_deleted = r.is_deleted;
        self.has_tags = self.tags_count > 0;
    }

    /// Replaces the stored record (refresh, rename, completed external media) and recomputes everything derived from it.
    pub fn set_record(&mut self, record: MediaRecord) {
        if record.filename != self.record.filename {
            self.filename_lower = record.filename.to_lowercase();
        }
        self.record = record;
        self.refresh_derived();
    }

    pub fn set_tags_count(&mut self, count: usize) {
        self.tags_count = count;
        self.has_tags = count > 0;
    }

    pub fn mark_deleted(&mut self) {
        self.record.is_deleted = true;
        self.refresh_derived();
    }

    pub fn set_folder(&mut self, folder: Arc<FolderDir>) {
        self.folder = folder;
    }

    /// The path of the file. Built on demand; hot paths must not call it per record.
    pub fn full_path(&self) -> PathBuf {
        self.folder.path().join(&self.record.filename)
    }

    /// Whether the record is the file at `path` (compares directory and name without building a path).
    pub fn is_at(&self, path: &Path) -> bool {
        path.parent() == Some(self.folder.path())
            && path
                .file_name()
                .is_some_and(|name| name == self.record.filename.as_str())
    }

    /// The status shown in the "Flags" column; also the key of that sort order.
    pub fn flags_label(&self) -> &'static str {
        if self.is_pending {
            "Pending"
        } else if self.is_deleted {
            "Deleted"
        } else if self.is_corrupted {
            "Corrupted"
        } else {
            "OK"
        }
    }

    /// The lower-case file extension of the format, shown in the "Format" column and the key of that sort order.
    pub fn format_label(&self) -> &'static str {
        self.record.format.default_extension()
    }
}
