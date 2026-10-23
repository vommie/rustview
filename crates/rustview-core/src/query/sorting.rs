use super::filters::IndexedMediaRecord;
use crate::library::models::MediaRecord;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortColumn {
    Filename = 0,
    Path = 1,
    ModifiedDate = 2,
    CreatedDate = 3,
    FileSize = 4,
    Rating = 5,
    Tags = 6,
    Width = 7,
    Height = 8,
    AspectRatio = 9,
    Duration = 10,
    Format = 11,
    Flags = 12,
}

impl SortColumn {
    pub fn as_str(&self) -> &'static str {
        match self {
            SortColumn::Filename => "Filename",
            SortColumn::Path => "Path",
            SortColumn::ModifiedDate => "Modified Date",
            SortColumn::CreatedDate => "Created Date",
            SortColumn::FileSize => "Filesize (MB)",
            SortColumn::Rating => "Rating",
            SortColumn::Tags => "Tags",
            SortColumn::Width => "Width",
            SortColumn::Height => "Height",
            SortColumn::AspectRatio => "AR",
            SortColumn::Duration => "Duration",
            SortColumn::Format => "Format",
            SortColumn::Flags => "Flags",
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.trim() {
            "Path" | "Folder" | "FolderPath" | "Directory" => SortColumn::Path,
            "Modified Date" | "ModifiedDate" | "ModificationTime" | "Date" => SortColumn::ModifiedDate,
            "Created Date" | "CreatedDate" | "CreationDate" => SortColumn::CreatedDate,
            "Filesize (MB)" | "Filesize" | "FileSize" | "Size" => SortColumn::FileSize,
            "Rating" => SortColumn::Rating,
            "Tags" | "TagsCount" => SortColumn::Tags,
            "Width" => SortColumn::Width,
            "Height" => SortColumn::Height,
            "AR" | "AspectRatio" | "Dimensions" => SortColumn::AspectRatio,
            "Duration" => SortColumn::Duration,
            "Format" => SortColumn::Format,
            "Flags" | "Flag" | "Status" | "State" => SortColumn::Flags,
            _ => SortColumn::Filename,
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => SortColumn::Path,
            2 => SortColumn::ModifiedDate,
            3 => SortColumn::CreatedDate,
            4 => SortColumn::FileSize,
            5 => SortColumn::Rating,
            6 => SortColumn::Tags,
            7 => SortColumn::Width,
            8 => SortColumn::Height,
            9 => SortColumn::AspectRatio,
            10 => SortColumn::Duration,
            11 => SortColumn::Format,
            12 => SortColumn::Flags,
            _ => SortColumn::Filename,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending = 0,
    Descending = 1,
}

impl SortOrder {
    pub fn as_str(&self) -> &'static str {
        match self {
            SortOrder::Ascending => "Ascending",
            SortOrder::Descending => "Descending",
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "Descending" | "Desc" => SortOrder::Descending,
            _ => SortOrder::Ascending,
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_index(idx: i32) -> Self {
        if idx == 1 {
            SortOrder::Descending
        } else {
            SortOrder::Ascending
        }
    }

    pub fn toggle(&self) -> Self {
        match self {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        }
    }
}

pub struct MediaSorter;

impl MediaSorter {
    pub fn sort(records: &mut [MediaRecord], column: SortColumn, order: SortOrder) {
        records.par_sort_unstable_by(|a, b| {
            let cmp = match column {
                SortColumn::Filename => a.filename.to_lowercase().cmp(&b.filename.to_lowercase()),
                SortColumn::Path => a
                    .folder_id
                    .cmp(&b.folder_id)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::ModifiedDate => a
                    .modification_time
                    .cmp(&b.modification_time)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::CreatedDate => a
                    .creation_time
                    .cmp(&b.creation_time)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::FileSize => a
                    .file_size
                    .cmp(&b.file_size)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::Rating => a
                    .rating
                    .cmp(&b.rating)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::Tags => a.id.cmp(&b.id),
                SortColumn::Width => a
                    .width
                    .cmp(&b.width)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::Height => a
                    .height
                    .cmp(&b.height)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::AspectRatio => {
                    let ar_a = if a.height > 0 {
                        a.width as f32 / a.height as f32
                    } else {
                        0.0
                    };
                    let ar_b = if b.height > 0 {
                        b.width as f32 / b.height as f32
                    } else {
                        0.0
                    };
                    ar_a.total_cmp(&ar_b)
                        .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()))
                }
                SortColumn::Duration => a
                    .duration_ms
                    .cmp(&b.duration_ms)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::Format => {
                    let fa = a.format.default_extension();
                    let fb = b.format.default_extension();
                    fa.cmp(fb)
                        .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()))
                }
                SortColumn::Flags => {
                    let flag_val = |r: &MediaRecord| -> u8 { if r.is_deleted { 2 } else { u8::from(r.is_corrupted) } };
                    flag_val(a)
                        .cmp(&flag_val(b))
                        .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()))
                }
            };

            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }

    pub fn sort_indexed_refs(records: &mut [&IndexedMediaRecord], column: SortColumn, order: SortOrder) {
        records.par_sort_unstable_by(|a, b| {
            let cmp = Self::compare_indexed(a, b, column);
            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }

    /// The ascending order of two records for `column`; ties are broken by file name.
    fn compare_indexed(a: &IndexedMediaRecord, b: &IndexedMediaRecord, column: SortColumn) -> Ordering {
        let by_name = || a.filename_lower.cmp(&b.filename_lower);
        let (ra, rb) = (&a.record, &b.record);
        match column {
            SortColumn::Filename => return by_name(),
            SortColumn::Path => {
                let by_dir = if Arc::ptr_eq(&a.folder, &b.folder) {
                    Ordering::Equal
                } else {
                    a.folder.path_lower().cmp(b.folder.path_lower())
                };
                return by_dir.then_with(by_name);
            }
            SortColumn::ModifiedDate => ra.modification_time.cmp(&rb.modification_time),
            SortColumn::CreatedDate => ra.creation_time.cmp(&rb.creation_time),
            SortColumn::FileSize => ra.file_size.cmp(&rb.file_size),
            SortColumn::Rating => ra.rating.cmp(&rb.rating),
            SortColumn::Tags => a.tags_count.cmp(&b.tags_count),
            SortColumn::Width => ra.width.cmp(&rb.width),
            SortColumn::Height => ra.height.cmp(&rb.height),
            SortColumn::AspectRatio => a
                .ar_value
                .total_cmp(&b.ar_value)
                .then_with(|| (ra.width * ra.height).cmp(&(rb.width * rb.height))),
            SortColumn::Duration => ra.duration_ms.cmp(&rb.duration_ms),
            SortColumn::Format => a.format_label().cmp(b.format_label()),
            SortColumn::Flags => a.flags_label().cmp(b.flags_label()),
        }
        .then_with(by_name)
    }
}
