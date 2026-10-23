//! The one place that turns an indexed record into the row the grid and the table show.

use super::record_labels::RecordLabels;
use crate::ui::MediaItemView;
use rustview_core::media::MediaType;
use rustview_core::query::IndexedMediaRecord;
use rustview_core::util::cast::{SaturatingAs, WrappingAs};
use slint::Image;

/// A complete row for the item at list `position`.
pub fn build_view(item: &IndexedMediaRecord, position: usize, is_selected: bool, thumbnail: Image) -> MediaItemView {
    let mut view = MediaItemView {
        id: item.record.id.wrapping_as::<i32>(),
        index: position.saturating_as::<i32>(),
        thumbnail,
        is_selected,
        is_video: matches!(item.record.media_type, MediaType::Video),
        ..Default::default()
    };
    apply_record(&mut view, item);
    view
}

/// Copies everything the record determines into an existing row; the thumbnail, the selection and the position stay as they are.
pub fn apply_record(view: &mut MediaItemView, item: &IndexedMediaRecord) {
    let labels = RecordLabels::of(item);
    view.filename = labels.filename;
    view.path_str = item.folder.path_str().clone();
    view.modified_date_str = labels.modified_date;
    view.created_date_str = labels.created_date;
    view.filesize_str = labels.filesize;
    view.rating = i32::from(item.record.rating);
    view.rating_str = labels.rating;
    view.tags_count = item.tags_count.saturating_as::<i32>();
    view.tags_str = labels.tags;
    view.width_str = labels.width;
    view.height_str = labels.height;
    view.ar_str = labels.aspect_ratio;
    view.duration_str = labels.duration;
    view.duration_badge_str = labels.duration_badge;
    view.format_str = labels.format;
    view.flags_str = labels.flags;
    view.has_tags = item.has_tags;
    view.is_corrupted = item.is_corrupted;
    view.is_deleted = item.is_deleted;
    view.is_pending = item.is_pending;
    view.is_animated = item.is_animated;
}
