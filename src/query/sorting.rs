use super::filters::IndexedMediaRecord;
use crate::library::models::MediaRecord;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortColumn {
    Filename = 0,
    Dimensions = 1,
    FileSize = 2,
    Rating = 3,
    ModificationTime = 4,
}

impl SortColumn {
    pub fn as_str(&self) -> &'static str {
        match self {
            SortColumn::Filename => "Filename",
            SortColumn::Dimensions => "Dimensions",
            SortColumn::FileSize => "FileSize",
            SortColumn::Rating => "Rating",
            SortColumn::ModificationTime => "ModificationTime",
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "Dimensions" => SortColumn::Dimensions,
            "FileSize" | "Size" => SortColumn::FileSize,
            "Rating" => SortColumn::Rating,
            "ModificationTime" | "Date" => SortColumn::ModificationTime,
            _ => SortColumn::Filename,
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => SortColumn::Dimensions,
            2 => SortColumn::FileSize,
            3 => SortColumn::Rating,
            4 => SortColumn::ModificationTime,
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
                SortColumn::FileSize => a
                    .file_size
                    .cmp(&b.file_size)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::Dimensions => (a.width * a.height)
                    .cmp(&(b.width * b.height))
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::ModificationTime => a
                    .modification_time
                    .cmp(&b.modification_time)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
                SortColumn::Rating => a
                    .rating
                    .cmp(&b.rating)
                    .then_with(|| a.filename.to_lowercase().cmp(&b.filename.to_lowercase())),
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
                SortColumn::FileSize => a
                    .record
                    .file_size
                    .cmp(&b.record.file_size)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Dimensions => (a.record.width * a.record.height)
                    .cmp(&(b.record.width * b.record.height))
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::ModificationTime => a
                    .record
                    .modification_time
                    .cmp(&b.record.modification_time)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Rating => a
                    .record
                    .rating
                    .cmp(&b.record.rating)
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
                SortColumn::FileSize => a
                    .record
                    .file_size
                    .cmp(&b.record.file_size)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Dimensions => (a.record.width * a.record.height)
                    .cmp(&(b.record.width * b.record.height))
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::ModificationTime => a
                    .record
                    .modification_time
                    .cmp(&b.record.modification_time)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
                SortColumn::Rating => a
                    .record
                    .rating
                    .cmp(&b.record.rating)
                    .then_with(|| a.filename_lower.cmp(&b.filename_lower)),
            };

            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }
}
