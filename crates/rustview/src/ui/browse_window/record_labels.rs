//! The strings the grid and the table show for a record, derived when a row is built.
//!
//! Records keep numbers only (see `query::IndexedMediaRecord`); the formatted dates, sizes, dimensions and labels cost
//! microseconds per row and are produced for the rows that are alive as tiles, not for every record of a folder.

use super::super::models::format_timestamp;
use rustview_core::media::MediaType;
use rustview_core::media::formats::{approximate_aspect_ratio, format_duration_compact, format_duration_hms_ms, format_filesize_mb};
use rustview_core::query::IndexedMediaRecord;
use slint::SharedString;

pub struct RecordLabels {
    pub filename: SharedString,
    pub modified_date: SharedString,
    pub created_date: SharedString,
    pub filesize: SharedString,
    pub rating: SharedString,
    pub tags: SharedString,
    pub width: SharedString,
    pub height: SharedString,
    pub aspect_ratio: SharedString,
    pub duration: SharedString,
    pub duration_badge: SharedString,
    pub format: SharedString,
    pub flags: SharedString,
}

impl RecordLabels {
    pub fn of(item: &IndexedMediaRecord) -> Self {
        let r = &item.record;
        let is_video = matches!(r.media_type, MediaType::Video);
        Self {
            filename: SharedString::from(r.filename.as_str()),
            modified_date: format_timestamp(r.modification_time).into(),
            created_date: format_timestamp(r.creation_time).into(),
            filesize: format_filesize_mb(r.file_size).into(),
            rating: r.rating.to_string().into(),
            tags: item.tags_count.to_string().into(),
            width: dimension(r.width),
            height: dimension(r.height),
            aspect_ratio: approximate_aspect_ratio(r.width, r.height).into(),
            duration: format_duration_hms_ms(r.duration_ms, is_video).into(),
            duration_badge: format_duration_compact(r.duration_ms, is_video).into(),
            format: item.format_label().into(),
            flags: item.flags_label().into(),
        }
    }
}

fn dimension(pixels: u32) -> SharedString {
    if pixels > 0 { pixels.to_string().into() } else { "-".into() }
}
