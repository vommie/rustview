use rustview::library::models::MediaRecord;
use rustview::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use rustview::media::{MediaFormat, MediaType};
use rustview::query::filters::IndexedMediaRecord;
use rustview::ui::file_selection::{FileSelectionModel, FileSelectionSummary};
use std::collections::HashSet;
use std::path::PathBuf;

fn create_dummy_record(
    id: u64,
    name: &str,
    size: u64,
    w: u32,
    h: u32,
    fmt: MediaFormat,
    rating: u8,
) -> MediaRecord {
    MediaRecord {
        id,
        folder_id: 1,
        filename: name.to_string(),
        file_hash: [0xAA; 16],
        file_size: size,
        modification_time: 1700000000,
        creation_time: 1690000000,
        duration_ms: if matches!(fmt, MediaFormat::Mp4) { 10500 } else { 0 },
        media_type: if matches!(fmt, MediaFormat::Mp4) {
            MediaType::Video
        } else {
            MediaType::Image
        },
        format: fmt,
        width: w,
        height: h,
        rating,
        last_seen: 1,
        is_deleted: false,
    }
}

#[test]
fn test_file_selection_single_click_clears_prior() {
    let mut sel = FileSelectionModel::new();
    sel.select_single(101);
    assert_eq!(sel.count(), 1);
    assert!(sel.is_selected(101));

    sel.select_single(102);
    assert_eq!(sel.count(), 1);
    assert!(!sel.is_selected(101));
    assert!(sel.is_selected(102));
    assert_eq!(sel.anchor_id(), Some(102));
}

#[test]
fn test_file_selection_clear_all() {
    let mut sel = FileSelectionModel::new();
    sel.select_single(101);
    sel.toggle(102);
    sel.toggle(103);
    assert_eq!(sel.count(), 3);

    sel.clear();
    assert_eq!(sel.count(), 0);
    assert!(!sel.is_selected(101));
    assert!(!sel.is_selected(102));
    assert!(!sel.is_selected(103));
    assert_eq!(sel.anchor_id(), None);
    assert_eq!(sel.last_clicked_id(), None);
}

#[test]
fn test_file_selection_multi_select_drag_preservation() {
    let mut sel = FileSelectionModel::new();
    sel.select_single(10);
    sel.toggle(20);
    sel.toggle(30);
    assert_eq!(sel.count(), 3);

    assert!(sel.is_selected(20));
    let dragged_ids = sel.selected_ids().clone();
    assert_eq!(dragged_ids.len(), 3);
    assert!(dragged_ids.contains(&10));
    assert!(dragged_ids.contains(&20));
    assert!(dragged_ids.contains(&30));
}

#[test]
fn test_file_selection_diff_computation() {
    let mut prev = HashSet::new();
    prev.insert(1);
    prev.insert(2);

    let mut current = HashSet::new();
    current.insert(2);
    current.insert(3);

    let mut diff = FileSelectionModel::compute_selection_diff(&prev, &current);
    diff.sort();
    assert_eq!(diff, vec![1, 3]);
}

#[test]
fn test_file_selection_ctrl_toggle() {
    let mut sel = FileSelectionModel::new();
    sel.select_single(1);
    sel.toggle(2);
    sel.toggle(3);

    assert_eq!(sel.count(), 3);
    assert!(sel.is_selected(1));
    assert!(sel.is_selected(2));
    assert!(sel.is_selected(3));

    sel.toggle(2);
    assert_eq!(sel.count(), 2);
    assert!(!sel.is_selected(2));
    assert_eq!(sel.anchor_id(), Some(2));
}

#[test]
fn test_file_selection_shift_range_contiguous() {
    let mut sel = FileSelectionModel::new();
    let visible_order = vec![10, 20, 30, 40, 50, 60];

    sel.select_single(20);
    assert_eq!(sel.anchor_id(), Some(20));

    sel.select_range(50, &visible_order, false);
    assert_eq!(sel.count(), 4);
    assert!(sel.is_selected(20));
    assert!(sel.is_selected(30));
    assert!(sel.is_selected(40));
    assert!(sel.is_selected(50));
    assert!(!sel.is_selected(10));
    assert!(!sel.is_selected(60));
    assert_eq!(sel.anchor_id(), Some(20));

    sel.select_range(30, &visible_order, false);
    assert_eq!(sel.count(), 2);
    assert!(sel.is_selected(20));
    assert!(sel.is_selected(30));
    assert!(!sel.is_selected(40));
    assert!(!sel.is_selected(50));
}

#[test]
fn test_file_selection_ctrl_shift_additive_range() {
    let mut sel = FileSelectionModel::new();
    let visible_order = vec![1, 2, 3, 4, 5, 6, 7, 8];

    sel.select_single(1);
    sel.toggle(4);
    assert_eq!(sel.count(), 2);
    assert_eq!(sel.anchor_id(), Some(4));

    sel.select_range(6, &visible_order, true);
    assert_eq!(sel.count(), 4);
    assert!(sel.is_selected(1));
    assert!(sel.is_selected(4));
    assert!(sel.is_selected(5));
    assert!(sel.is_selected(6));
}

#[test]
fn test_file_selection_summary_calculations() {
    let recs = vec![
        create_dummy_record(1, "img1.jpg", 1024 * 1024, 1920, 1080, MediaFormat::Jpeg, 4),
        create_dummy_record(
            2,
            "img2.jpg",
            2 * 1024 * 1024,
            1920,
            1080,
            MediaFormat::Jpeg,
            5,
        ),
        create_dummy_record(
            3,
            "video.mp4",
            50 * 1024 * 1024,
            3840,
            2160,
            MediaFormat::Mp4,
            0,
        ),
    ];

    let summary = FileSelectionSummary::from_records(&recs);
    assert_eq!(summary.count, 3);
    assert_eq!(summary.total_size_bytes, 53 * 1024 * 1024);
    assert_eq!(summary.min_dimensions, Some((1920, 1080)));
    assert_eq!(summary.max_dimensions, Some((3840, 2160)));
    assert!(!summary.is_uniform_dimensions);
    assert_eq!(summary.rated_count, 2);
    assert_eq!(summary.avg_rating, 4.5);
    assert_eq!(summary.format_breakdown[0], (MediaFormat::Jpeg, 2));
    assert_eq!(summary.format_breakdown[1], (MediaFormat::Mp4, 1));

    // Test zero-copy summary aggregation
    let indexed = recs
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let is_video = matches!(r.media_type, MediaType::Video);
            let ar_str = approximate_aspect_ratio(r.width, r.height);
            let ar_value = if r.height > 0 { r.width as f32 / r.height as f32 } else { 0.0 };
            IndexedMediaRecord {
                index: i,
                full_path: PathBuf::from(&r.filename),
                filename_lower: r.filename.to_lowercase(),
                filename_slint: r.filename.clone().into(),
                modified_date_str: "2026-08-20 12:00:00".into(),
                created_date_str: "2026-08-20 10:00:00".into(),
                filesize_mb_str: format_filesize_mb(r.file_size).into(),
                rating_str: r.rating.to_string().into(),
                tags_count: 0,
                tags_count_str: "0".into(),
                width_str: r.width.to_string().into(),
                height_str: r.height.to_string().into(),
                ar_str: ar_str.into(),
                ar_value,
                duration_str: format_duration_hms_ms(r.duration_ms, is_video).into(),
                format_str: r.format.default_extension().into(),
                has_tags: false,
                record: r,
            }
        })
        .collect::<Vec<_>>();

    let summary_indexed = FileSelectionSummary::from_indexed_records(&indexed);
    assert_eq!(summary_indexed.count, 3);
    assert_eq!(summary_indexed.total_size_bytes, 53 * 1024 * 1024);
    assert_eq!(summary_indexed.avg_rating, 4.5);
}
