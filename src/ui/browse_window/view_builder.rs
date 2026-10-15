//! The one place that turns an indexed record into the row the grid and the table show.

use crate::media::MediaType;
use crate::query::filters::IndexedMediaRecord;
use crate::ui::MediaItemView;
use slint::Image;

/// A complete row for the item at list `position`.
pub fn build_view(item: &IndexedMediaRecord, position: usize, is_selected: bool, thumbnail: Image) -> MediaItemView {
    MediaItemView {
        id: item.record.id as i32,
        index: position as i32,
        filename: item.filename_slint.clone(),
        path_str: item.folder_path_str.clone(),
        modified_date_str: item.modified_date_str.clone(),
        created_date_str: item.created_date_str.clone(),
        filesize_str: item.filesize_mb_str.clone(),
        rating: i32::from(item.record.rating),
        rating_str: item.rating_str.clone(),
        tags_count: item.tags_count as i32,
        tags_str: item.tags_count_str.clone(),
        width_str: item.width_str.clone(),
        height_str: item.height_str.clone(),
        ar_str: item.ar_str.clone(),
        duration_str: item.duration_str.clone(),
        duration_badge_str: item.duration_badge_str.clone(),
        format_str: item.format_str.clone(),
        flags_str: item.flags_str.clone(),
        has_tags: item.has_tags,
        thumbnail,
        is_selected,
        is_video: matches!(item.record.media_type, MediaType::Video),
        is_corrupted: item.is_corrupted,
        is_deleted: item.is_deleted,
        is_pending: item.is_pending,
    }
}

/// Copies the metadata of the record into an existing row; the thumbnail, the selection and the position stay as they are.
pub fn apply_record(view: &mut MediaItemView, item: &IndexedMediaRecord) {
    view.width_str = item.width_str.clone();
    view.height_str = item.height_str.clone();
    view.ar_str = item.ar_str.clone();
    view.duration_str = item.duration_str.clone();
    view.duration_badge_str = item.duration_badge_str.clone();
    view.filesize_str = item.filesize_mb_str.clone();
    view.modified_date_str = item.modified_date_str.clone();
    view.created_date_str = item.created_date_str.clone();
    view.format_str = item.format_str.clone();
    view.flags_str = item.flags_str.clone();
    view.is_corrupted = item.is_corrupted;
    view.is_deleted = item.is_deleted;
    view.is_pending = item.is_pending;
    view.tags_count = item.tags_count as i32;
    view.tags_str = item.tags_count_str.clone();
    view.has_tags = item.has_tags;
}
