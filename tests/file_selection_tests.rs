use rustview::library::models::MediaRecord;
use rustview::media::{MediaFormat, MediaType};
use rustview::ui::file_selection::{FileSelectionModel, FileSelectionSummary};

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
        file_hash: [0xAA; 32],
        file_size: size,
        modification_time: 1700000000,
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
fn test_file_selection_ctrl_toggle() {
    let mut sel = FileSelectionModel::new();
    sel.select_single(1);
    sel.toggle(2);
    sel.toggle(3);

    assert_eq!(sel.count(), 3);
    assert!(sel.is_selected(1));
    assert!(sel.is_selected(2));
    assert!(sel.is_selected(3));

    // Deselect item 2
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

    // Shift-click down to 50 -> selects [20, 30, 40, 50]
    sel.select_range(50, &visible_order, false);
    assert_eq!(sel.count(), 4);
    assert!(sel.is_selected(20));
    assert!(sel.is_selected(30));
    assert!(sel.is_selected(40));
    assert!(sel.is_selected(50));
    assert!(!sel.is_selected(10));
    assert!(!sel.is_selected(60));
    assert_eq!(sel.anchor_id(), Some(20));

    // Shift-click back to 30 -> shrinks range to [20, 30]
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

    // Ctrl+Shift click 6 -> adds [4..=6] while retaining 1
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
}
