use super::filters::IndexedMediaRecord;
use crate::library::models::MediaRecord;
use crate::util::cast::Az;
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

/// The most levels a stacked sort has.
pub const MAX_SORT_KEYS: usize = 4;

/// One level of the list's sort order: the first key decides, the next one orders the records the keys before it leave
/// equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    pub column: SortColumn,
    pub order: SortOrder,
}

impl SortKey {
    pub fn new(column: SortColumn, order: SortOrder) -> Self {
        Self { column, order }
    }

    /// A click on a column header. A plain click sorts by that column alone (ascending) and reverses it
    /// when it already is the only key. A stacked click (Shift) adds the column as the next level, or reverses the level
    /// it already has; a full list replaces its last level.
    pub fn click(keys: &mut Vec<SortKey>, column: SortColumn, stacked: bool) {
        let place = keys.iter().position(|key| key.column == column);
        match (stacked, place) {
            (true, Some(place)) => keys[place].order = keys[place].order.toggle(),
            (true, None) => {
                if keys.len() >= MAX_SORT_KEYS {
                    keys.pop();
                }
                keys.push(SortKey::new(column, SortOrder::Ascending));
            }
            (false, Some(0)) if keys.len() == 1 => keys[0].order = keys[0].order.toggle(),
            (false, _) => *keys = vec![SortKey::new(column, SortOrder::Ascending)],
        }
    }

    /// The keys with every column at most once and at most `MAX_SORT_KEYS` of them; an empty list becomes the default
    /// order (file name, ascending).
    pub fn sanitized(keys: &[SortKey]) -> Vec<SortKey> {
        let mut clean: Vec<SortKey> = Vec::with_capacity(MAX_SORT_KEYS);
        for key in keys {
            if clean.len() < MAX_SORT_KEYS && !clean.iter().any(|kept| kept.column == key.column) {
                clean.push(*key);
            }
        }
        if clean.is_empty() {
            clean.push(SortKey::new(SortColumn::Filename, SortOrder::Ascending));
        }
        clean
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
                        a.width.az::<f32>() / a.height.az::<f32>()
                    } else {
                        0.0
                    };
                    let ar_b = if b.height > 0 {
                        b.width.az::<f32>() / b.height.az::<f32>()
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

    /// Sorts by `keys`, the first one first. Records that are equal in every key are ordered by file name, in the
    /// direction of the last key (so a single key sorts exactly as it always did).
    pub fn sort_indexed_refs(records: &mut [&IndexedMediaRecord], keys: &[SortKey]) {
        let tie_order = keys.last().map_or(SortOrder::Ascending, |key| key.order);
        let directed = |cmp: Ordering, order: SortOrder| match order {
            SortOrder::Ascending => cmp,
            SortOrder::Descending => cmp.reverse(),
        };
        records.par_sort_unstable_by(|a, b| {
            for key in keys {
                let cmp = Self::compare_column(a, b, key.column);
                if cmp != Ordering::Equal {
                    return directed(cmp, key.order);
                }
            }
            directed(a.filename_lower.cmp(&b.filename_lower), tie_order)
        });
    }

    /// The ascending order of two records for `column` alone (no tie-break).
    fn compare_column(a: &IndexedMediaRecord, b: &IndexedMediaRecord, column: SortColumn) -> Ordering {
        let (ra, rb) = (&a.record, &b.record);
        match column {
            SortColumn::Filename => a.filename_lower.cmp(&b.filename_lower),
            SortColumn::Path => {
                if Arc::ptr_eq(&a.folder, &b.folder) {
                    Ordering::Equal
                } else {
                    a.folder.path_lower().cmp(b.folder.path_lower())
                }
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
    }
}

#[cfg(test)]
mod sort_key_tests {
    use super::*;

    fn key(column: SortColumn, order: SortOrder) -> SortKey {
        SortKey::new(column, order)
    }

    #[test]
    fn a_plain_click_sorts_by_one_column_and_reverses_it() {
        let mut keys = vec![key(SortColumn::Filename, SortOrder::Ascending)];
        SortKey::click(&mut keys, SortColumn::Filename, false);
        assert_eq!(keys, [key(SortColumn::Filename, SortOrder::Descending)]);
        SortKey::click(&mut keys, SortColumn::Rating, false);
        assert_eq!(
            keys,
            [key(SortColumn::Rating, SortOrder::Ascending)],
            "another column starts ascending"
        );
    }

    #[test]
    fn a_stacked_click_adds_a_level_or_reverses_it() {
        let mut keys = vec![key(SortColumn::Rating, SortOrder::Descending)];
        SortKey::click(&mut keys, SortColumn::Filename, true);
        assert_eq!(
            keys,
            [
                key(SortColumn::Rating, SortOrder::Descending),
                key(SortColumn::Filename, SortOrder::Ascending)
            ]
        );
        SortKey::click(&mut keys, SortColumn::Rating, true);
        assert_eq!(
            keys[0],
            key(SortColumn::Rating, SortOrder::Ascending),
            "the level it has is reversed"
        );
        assert_eq!(keys.len(), 2);
        // A plain click on the first of several keys starts over with that column.
        SortKey::click(&mut keys, SortColumn::Rating, false);
        assert_eq!(keys, [key(SortColumn::Rating, SortOrder::Ascending)]);
    }

    #[test]
    fn a_full_list_replaces_its_last_level() {
        let mut keys = Vec::new();
        for column in [
            SortColumn::Rating,
            SortColumn::Tags,
            SortColumn::Format,
            SortColumn::FileSize,
        ] {
            SortKey::click(&mut keys, column, true);
        }
        assert_eq!(keys.len(), MAX_SORT_KEYS);
        SortKey::click(&mut keys, SortColumn::Width, true);
        assert_eq!(keys.len(), MAX_SORT_KEYS);
        assert_eq!(keys[3].column, SortColumn::Width);
        assert_eq!(keys[2].column, SortColumn::Format);
    }

    #[test]
    fn stored_keys_are_cleaned() {
        let filename = key(SortColumn::Filename, SortOrder::Ascending);
        assert_eq!(SortKey::sanitized(&[]), [filename], "no keys: the default order");
        let rating = key(SortColumn::Rating, SortOrder::Descending);
        let repeated = [rating, key(SortColumn::Rating, SortOrder::Ascending), filename];
        assert_eq!(
            SortKey::sanitized(&repeated),
            [rating, filename],
            "a column counts once"
        );
        let many: Vec<SortKey> = (0..7)
            .map(|index| key(SortColumn::from_index(index), SortOrder::Ascending))
            .collect();
        assert_eq!(SortKey::sanitized(&many).len(), MAX_SORT_KEYS);
    }
}
