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
}
