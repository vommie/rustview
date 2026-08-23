use rustview::library::models::MediaRecord;
use rustview::library::Library;
use rustview::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use rustview::media::{MediaFormat, MediaType};
use rustview::query::filters::{IndexedMediaRecord, MediaTypeFilter, QueryFilter};
use rustview::ui::bridge_state::UiState;
use rustview::ui::file_selection::{FileSelectionModel, FileSelectionSummary};
use std::collections::HashSet;
use std::path::PathBuf;
use tempfile::tempdir;

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
        is_corrupted: false,
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
fn test_file_selection_filter_pruning_lifecycle() {
    let tmp_db = tempdir().unwrap();
    let lib = Library::open(tmp_db.path()).unwrap();

    let r_vid1 = create_dummy_record(1, "clip_alpha.mp4", 1000, 1920, 1080, MediaFormat::Mp4, 5);
    let r_vid2 = create_dummy_record(2, "clip_beta.mp4", 2000, 1920, 1080, MediaFormat::Mp4, 4);
    let r_img1 = create_dummy_record(3, "photo_gamma.jpg", 500, 800, 600, MediaFormat::Jpeg, 5);
    let r_img2 = create_dummy_record(4, "photo_delta.png", 600, 1024, 768, MediaFormat::Png, 3);

    let indexed: Vec<IndexedMediaRecord> = vec![r_vid1, r_vid2, r_img1, r_img2]
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
                flags_str: "OK".into(),
                is_corrupted: false,
                is_deleted: false,
                has_tags: false,
                record: r,
            }
        })
        .collect();

    let mut sel = FileSelectionModel::new();
    sel.select_single(1); // clip_alpha.mp4 (Video)
    sel.toggle(2);        // clip_beta.mp4 (Video)
    sel.toggle(3);        // photo_gamma.jpg (Image)
    assert_eq!(sel.count(), 3);

    // 1. Filter: "Images" only -> Items 3 & 4 match. Item 1 & 2 must be pruned. Item 3 must stay selected!
    let filter_images = QueryFilter {
        media_type_filter: MediaTypeFilter::Images,
        ..Default::default()
    };
    let matched_images = filter_images.execute_indexed_refs(&lib, &indexed, None);
    let matched_ids_images: HashSet<u64> = matched_images.iter().map(|m| m.record.id).collect();
    assert_eq!(matched_ids_images, HashSet::from([3, 4]));

    sel.prune_missing(&matched_ids_images);
    assert_eq!(sel.count(), 1);
    assert!(sel.is_selected(3));
    assert!(!sel.is_selected(1));
    assert!(!sel.is_selected(2));

    // 2. Select item 4 as well (both images selected)
    sel.toggle(4);
    assert_eq!(sel.count(), 2);

    // 3. Search query: "gamma" -> Only photo_gamma.jpg (ID 3) matches. Item 4 must be pruned!
    let filter_search = QueryFilter {
        media_type_filter: MediaTypeFilter::Images,
        search_query: "gamma".to_string(),
        ..Default::default()
    };
    let matched_search = filter_search.execute_indexed_refs(&lib, &indexed, None);
    let matched_ids_search: HashSet<u64> = matched_search.iter().map(|m| m.record.id).collect();
    assert_eq!(matched_ids_search, HashSet::from([3]));

    sel.prune_missing(&matched_ids_search);
    assert_eq!(sel.count(), 1);
    assert!(sel.is_selected(3));
    assert!(!sel.is_selected(4));
}

#[test]
fn test_file_selection_visual_sort_order_preservation() {
    let r1 = create_dummy_record(100, "C_File.mp4", 1000, 1920, 1080, MediaFormat::Mp4, 1);
    let r2 = create_dummy_record(200, "A_File.mp4", 2000, 1920, 1080, MediaFormat::Mp4, 2);
    let r3 = create_dummy_record(300, "B_File.mp4", 3000, 1920, 1080, MediaFormat::Mp4, 3);

    let idx1 = IndexedMediaRecord {
        index: 0,
        full_path: PathBuf::from("/media/C_File.mp4"),
        filename_lower: "c_file.mp4".into(),
        filename_slint: "C_File.mp4".into(),
        modified_date_str: "-".into(),
        created_date_str: "-".into(),
        filesize_mb_str: "1.00".into(),
        rating_str: "1".into(),
        tags_count: 0,
        tags_count_str: "0".into(),
        width_str: "1920".into(),
        height_str: "1080".into(),
        ar_str: "16:9".into(),
        ar_value: 1.77,
        duration_str: "00:00:10.500".into(),
        format_str: "mp4".into(),
        flags_str: "OK".into(),
        is_corrupted: false,
        is_deleted: false,
        has_tags: false,
        record: r1,
    };

    let idx2 = IndexedMediaRecord {
        index: 1,
        full_path: PathBuf::from("/media/A_File.mp4"),
        filename_lower: "a_file.mp4".into(),
        filename_slint: "A_File.mp4".into(),
        modified_date_str: "-".into(),
        created_date_str: "-".into(),
        filesize_mb_str: "2.00".into(),
        rating_str: "2".into(),
        tags_count: 0,
        tags_count_str: "0".into(),
        width_str: "1920".into(),
        height_str: "1080".into(),
        ar_str: "16:9".into(),
        ar_value: 1.77,
        duration_str: "00:00:10.500".into(),
        format_str: "mp4".into(),
        flags_str: "OK".into(),
        is_corrupted: false,
        is_deleted: false,
        has_tags: false,
        record: r2,
    };

    let idx3 = IndexedMediaRecord {
        index: 2,
        full_path: PathBuf::from("/media/B_File.mp4"),
        filename_lower: "b_file.mp4".into(),
        filename_slint: "B_File.mp4".into(),
        modified_date_str: "-".into(),
        created_date_str: "-".into(),
        filesize_mb_str: "3.00".into(),
        rating_str: "3".into(),
        tags_count: 0,
        tags_count_str: "0".into(),
        width_str: "1920".into(),
        height_str: "1080".into(),
        ar_str: "16:9".into(),
        ar_value: 1.77,
        duration_str: "00:00:10.500".into(),
        format_str: "mp4".into(),
        flags_str: "OK".into(),
        is_corrupted: false,
        is_deleted: false,
        has_tags: false,
        record: r3,
    };

    let mut state = UiState {
        all_folder_media: vec![idx1, idx2, idx3],
        active_indices: vec![1, 2, 0],
        ..Default::default()
    };

    state.file_selection.select_single(100);
    state.file_selection.toggle(200);
    state.file_selection.toggle(300);

    let ordered_media = state.get_selected_media_ordered();
    assert_eq!(ordered_media.len(), 3);
    assert_eq!(ordered_media[0].record.filename, "A_File.mp4");
    assert_eq!(ordered_media[1].record.filename, "B_File.mp4");
    assert_eq!(ordered_media[2].record.filename, "C_File.mp4");

    let ordered_paths = state.get_selected_paths_ordered();
    assert_eq!(ordered_paths[0], PathBuf::from("/media/A_File.mp4"));
    assert_eq!(ordered_paths[1], PathBuf::from("/media/B_File.mp4"));
    assert_eq!(ordered_paths[2], PathBuf::from("/media/C_File.mp4"));

    let ordered_ids = state.get_selected_ids_ordered();
    assert_eq!(ordered_ids, vec![200, 300, 100]);
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
                flags_str: "OK".into(),
                is_corrupted: false,
                is_deleted: false,
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
