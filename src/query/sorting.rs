use super::filters::IndexedMediaRecord;
use crate::library::models::MediaRecord;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortColumn {
    Filename = 0,
    ModifiedDate = 1,
    CreatedDate = 2,
    FileSize = 3,
    Rating = 4,
    Tags = 5,
    Width = 6,
    Height = 7,
    AspectRatio = 8,
    Duration = 9,
    Format = 10,
}

impl SortColumn {
    pub fn as_str(&self) -> &'static str {
        match self {
            SortColumn::Filename => "Filename",
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
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.trim() {
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
            _ => SortColumn::Filename,
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => SortColumn::ModifiedDate,
            2 => SortColumn::CreatedDate,
            3 => SortColumn::FileSize,
            4 => SortColumn::Rating,
            5 => SortColumn::Tags,
            6 => SortColumn::Width,
            7 => SortColumn::Height,
            8 => SortColumn::AspectRatio,
            9 => SortColumn::Duration,
            10 => SortColumn::Format,
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
                    let ar_a = if a.height > 0 { a.width as f32 / a.height as f32 } else { 0.0 };
                    let ar_b = if b.height > 0 { b.width as f32 / b.height as f32 } else { 0.0 };
                    ar_a.total_cmp(&ar_b).then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()))
                }
                SortColumn::Duration => a
                    .duration_ms
                    .cmp(&b.duration_ms)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::Format => {
                    let fa = a.format.default_extension();
                    let fb = b.format.default_extension();
                    fa.cmp(fb).then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()))
                }
            };

            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }

    pub fn sort_indexed(records: &mut [IndexedMediaRecord], column: SortColumn, order: SortOrder) {
        records.par_sort_unstable_by(|a, b| {
            let cmp = match column {
                SortColumn::Filename => a.filename_lower.cmp(&b.filename_lower),
                SortColumn::ModifiedDate => a
                    .record
                    .modification_time
                    .cmp(&b.record.modification_time)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::CreatedDate => a
                    .record
                    .creation_time
                    .cmp(&b.record.creation_time)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::FileSize => a
                    .record
                    .file_size
                    .cmp(&b.record.file_size)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Rating => a
                    .record
                    .rating
                    .cmp(&b.record.rating)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Tags => a
                    .tags_count
                    .cmp(&b.tags_count)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Width => a
                    .record
                    .width
                    .cmp(&b.record.width)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Height => a
                    .record
                    .height
                    .cmp(&b.record.height)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::AspectRatio => a
                    .ar_value
                    .total_cmp(&b.ar_value)
                    .then_with(|| (a.record.width * a.record.height).cmp(&(b.record.width * b.record.height)))
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Duration => a
                    .record
                    .duration_ms
                    .cmp(&b.record.duration_ms)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Format => a
                    .format_str
                    .cmp(&b.format_str)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
            };

            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }

    pub fn sort_indexed_refs(
        records: &mut [&IndexedMediaRecord],
        column: SortColumn,
        order: SortOrder,
    ) {
        records.par_sort_unstable_by(|a, b| {
            let cmp = match column {
                SortColumn::Filename => a.filename_lower.cmp(&b.filename_lower),
                SortColumn::ModifiedDate => a
                    .record
                    .modification_time
                    .cmp(&b.record.modification_time)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::CreatedDate => a
                    .record
                    .creation_time
                    .cmp(&b.record.creation_time)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::FileSize => a
                    .record
                    .file_size
                    .cmp(&b.record.file_size)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Rating => a
                    .record
                    .rating
                    .cmp(&b.record.rating)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Tags => a
                    .tags_count
                    .cmp(&b.tags_count)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Width => a
                    .record
                    .width
                    .cmp(&b.record.width)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Height => a
                    .record
                    .height
                    .cmp(&b.record.height)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::AspectRatio => a
                    .ar_value
                    .total_cmp(&b.ar_value)
                    .then_with(|| (a.record.width * a.record.height).cmp(&(b.record.width * b.record.height)))
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Duration => a
                    .record
                    .duration_ms
                    .cmp(&b.record.duration_ms)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Format => a
                    .format_str
                    .cmp(&b.format_str)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
            };

            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }
}
