use super::filters::IndexedMediaRecord;
use crate::library::models::MediaRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Filename,
    FileSize,
    Dimensions,
    ModificationTime,
    Rating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

pub struct MediaSorter;

impl MediaSorter {
    pub fn sort(records: &mut [MediaRecord], column: SortColumn, order: SortOrder) {
        records.sort_by(|a, b| {
            let cmp = match column {
                SortColumn::Filename => a.filename.to_lowercase().cmp(&b.filename.to_lowercase()),
                SortColumn::FileSize => a.file_size.cmp(&b.file_size),
                SortColumn::Dimensions => (a.width * a.height).cmp(&(b.width * b.height)),
                SortColumn::ModificationTime => a.modification_time.cmp(&b.modification_time),
                SortColumn::Rating => a.rating.cmp(&b.rating),
            };

            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }

    pub fn sort_indexed(records: &mut [IndexedMediaRecord], column: SortColumn, order: SortOrder) {
        records.sort_by(|a, b| {
            let cmp = match column {
                SortColumn::Filename => a.filename_lower.cmp(&b.filename_lower),
                SortColumn::FileSize => a.record.file_size.cmp(&b.record.file_size),
                SortColumn::Dimensions => {
                    (a.record.width * a.record.height).cmp(&(b.record.width * b.record.height))
                }
                SortColumn::ModificationTime => {
                    a.record.modification_time.cmp(&b.record.modification_time)
                }
                SortColumn::Rating => a.record.rating.cmp(&b.record.rating),
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
        records.sort_by(|a, b| {
            let cmp = match column {
                SortColumn::Filename => a.filename_lower.cmp(&b.filename_lower),
                SortColumn::FileSize => a.record.file_size.cmp(&b.record.file_size),
                SortColumn::Dimensions => {
                    (a.record.width * a.record.height).cmp(&(b.record.width * b.record.height))
                }
                SortColumn::ModificationTime => {
                    a.record.modification_time.cmp(&b.record.modification_time)
                }
                SortColumn::Rating => a.record.rating.cmp(&b.record.rating),
            };

            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }
}
