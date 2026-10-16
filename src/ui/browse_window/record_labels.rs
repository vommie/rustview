//! The strings the grid and the table show for a record, derived when a row is built.
//!
//! Records keep numbers only (see `query::IndexedMediaRecord`); the formatted dates, sizes, dimensions and labels cost
//! microseconds per row and are produced for the rows that are alive as tiles, not for every record of a folder.

use super::super::models::format_timestamp;
use crate::media::MediaType;
use crate::media::formats::{approximate_aspect_ratio, format_duration_compact, format_duration_hms_ms, format_filesize_mb};
use crate::query::IndexedMediaRecord;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::models::MediaRecord;
    use crate::media::MediaFormat;
    use crate::query::FolderDir;

    fn video(width: u32, duration_ms: u64) -> IndexedMediaRecord {
        let record = MediaRecord {
            id: 1,
            folder_id: 1,
            filename: "Clip.MP4".to_string(),
            file_hash: [0; 16],
            file_size: 3 * 1024 * 1024,
            modification_time: 0,
            media_type: MediaType::Video,
            format: MediaFormat::Mp4,
            width,
            height: 1080,
            rating: 4,
            last_seen: 0,
            is_deleted: false,
            creation_time: 0,
            duration_ms,
            is_corrupted: false,
            source: None,
        };
        IndexedMediaRecord::new(0, record, FolderDir::new("/v"), 2)
    }

    #[test]
    fn labels_follow_the_record() {
        let labels = RecordLabels::of(&video(1920, 65_000));
        assert_eq!(labels.filename.as_str(), "Clip.MP4");
        assert_eq!(labels.width.as_str(), "1920");
        assert_eq!(labels.height.as_str(), "1080");
        assert_eq!(labels.rating.as_str(), "4");
        assert_eq!(labels.tags.as_str(), "2");
        assert_eq!(labels.format.as_str(), "mp4");
        assert_eq!(labels.flags.as_str(), "OK");
        assert_eq!(labels.modified_date.as_str(), "-");
        assert_eq!(labels.duration_badge.as_str(), "01:05");
        assert_eq!(labels.filesize.as_str(), "3.00");
    }

    #[test]
    fn unknown_dimensions_show_a_dash() {
        let labels = RecordLabels::of(&video(0, 0));
        assert_eq!(labels.width.as_str(), "-");
        assert!(labels.duration_badge.is_empty());
        assert_eq!(labels.duration.as_str(), "-");
    }
}
