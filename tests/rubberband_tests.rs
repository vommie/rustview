use rustview::library::models::{MediaId, MediaRecord};
use rustview::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use rustview::media::{MediaFormat, MediaType};
use rustview::query::filters::IndexedMediaRecord;
use rustview::ui::rubberband_selection::{
    GridLayoutConfig, RectF32, RubberbandGeometry, RubberbandSession, RubberbandStartParams,
    TableLayoutConfig,
};
use std::collections::HashSet;
use std::path::PathBuf;

fn create_indexed_record(id: u64, name: &str) -> IndexedMediaRecord {
    let rec = MediaRecord {
        id,
        folder_id: 1,
        filename: name.to_string(),
        file_hash: [0xBB; 16],
        file_size: 1024,
        modification_time: 1700000000,
        creation_time: 1690000000,
        duration_ms: 0,
        media_type: MediaType::Image,
        format: MediaFormat::Jpeg,
        width: 1920,
        height: 1080,
        rating: 0,
        last_seen: 1,
        is_deleted: false,
        is_corrupted: false,
    };

    let is_video = matches!(rec.media_type, MediaType::Video);
    let ar_str = approximate_aspect_ratio(rec.width, rec.height);
    let ar_value = if rec.height > 0 { rec.width as f32 / rec.height as f32 } else { 0.0 };

    IndexedMediaRecord {
        index: id as usize,
        full_path: PathBuf::from(format!("/media/{}", name)),
        folder_path_str: "/media".into(),
        folder_path_lower: "/media".to_string(),
        filename_lower: name.to_lowercase(),
        filename_slint: name.into(),
        modified_date_str: "2026-08-20 12:00:00".into(),
        created_date_str: "2026-08-20 10:00:00".into(),
        filesize_mb_str: format_filesize_mb(rec.file_size).into(),
        rating_str: "0".into(),
        tags_count: 0,
        tags_count_str: "0".into(),
        width_str: "1920".into(),
        height_str: "1080".into(),
        ar_str: ar_str.into(),
        ar_value,
        duration_str: format_duration_hms_ms(rec.duration_ms, is_video).into(),
        format_str: "jpg".into(),
        flags_str: "OK".into(),
        is_corrupted: false,
        is_deleted: false,
        has_tags: false,
        record: rec,
    }
}

#[test]
fn test_rect_f32_intersection_and_points() {
    let r1 = RectF32::from_points(100.0, 100.0, 200.0, 200.0);
    assert_eq!(r1.x, 100.0);
    assert_eq!(r1.y, 100.0);
    assert_eq!(r1.width, 100.0);
    assert_eq!(r1.height, 100.0);

    // Negative drag (bottom-right to top-left)
    let r2 = RectF32::from_points(200.0, 200.0, 100.0, 100.0);
    assert_eq!(r2, r1);

    // Intersecting rectangle
    let r3 = RectF32::new(150.0, 150.0, 100.0, 100.0);
    assert!(r1.intersects(&r3));
    assert!(r3.intersects(&r1));

    // Disjoint rectangle
    let r4 = RectF32::new(300.0, 300.0, 50.0, 50.0);
    assert!(!r1.intersects(&r4));
}

#[test]
fn test_grid_view_geometric_intersections() {
    let config = GridLayoutConfig {
        columns: 3,
        item_tile_width: 100.0,
        item_tile_height: 100.0,
        grid_spacing: 10.0,
        grid_padding: 10.0,
    };

    let dataset: Vec<IndexedMediaRecord> = (0..9)
        .map(|i| create_indexed_record(i + 1, &format!("img_{}.jpg", i + 1)))
        .collect();

    let active_indices: Vec<usize> = (0..9).collect();

    // 1. Box selecting item 1 only (Row 0, Col 0)
    let rect1 = RectF32::from_points(0.0, 0.0, 50.0, 50.0);
    let inter1 = RubberbandGeometry::compute_grid_intersections(rect1, &dataset, &active_indices, config);
    assert_eq!(inter1, HashSet::from([1]));

    // 2. Box selecting items 1, 2, 4, 5 (Top 2x2 grid)
    let rect2 = RectF32::from_points(50.0, 50.0, 160.0, 160.0);
    let inter2 = RubberbandGeometry::compute_grid_intersections(rect2, &dataset, &active_indices, config);
    assert_eq!(inter2, HashSet::from([1, 2, 4, 5]));

    // 3. Box in empty spacing (x in [112, 118]) -> 0 items
    let rect3 = RectF32::from_points(112.0, 20.0, 118.0, 80.0);
    let inter3 = RubberbandGeometry::compute_grid_intersections(rect3, &dataset, &active_indices, config);
    assert_eq!(inter3, HashSet::new());
}

#[test]
fn test_table_view_geometric_intersections() {
    let config = TableLayoutConfig {
        row_height: 28.0,
        content_width: 800.0,
    };

    let dataset: Vec<IndexedMediaRecord> = (0..10)
        .map(|i| create_indexed_record(i + 1, &format!("doc_{}.png", i + 1)))
        .collect();

    let active_indices: Vec<usize> = (0..10).collect();

    // 1. Box selecting rows 1 and 2 (y in [10, 45]) -> Row 0 (0..28) and Row 1 (28..56)
    let rect1 = RectF32::from_points(50.0, 10.0, 200.0, 45.0);
    let inter1 = RubberbandGeometry::compute_table_intersections(rect1, &dataset, &active_indices, config);
    assert_eq!(inter1, HashSet::from([1, 2]));

    // 2. Box entirely beyond content width -> 0 items
    let rect2 = RectF32::from_points(850.0, 10.0, 950.0, 100.0);
    let inter2 = RubberbandGeometry::compute_table_intersections(rect2, &dataset, &active_indices, config);
    assert_eq!(inter2, HashSet::new());
}

#[test]
fn test_rubberband_empty_active_indices_never_selects_hidden_items() {
    let dataset: Vec<IndexedMediaRecord> = (0..10)
        .map(|i| create_indexed_record(i + 1, &format!("hidden_img_{}.jpg", i + 1)))
        .collect();

    // Filter is active but matched 0 items (e.g. Media Type filter is Videos, but folder only has images)
    let empty_active_indices: Vec<usize> = Vec::new();

    let grid_config = GridLayoutConfig {
        columns: 3,
        item_tile_width: 100.0,
        item_tile_height: 100.0,
        grid_spacing: 10.0,
        grid_padding: 10.0,
    };

    let table_config = TableLayoutConfig {
        row_height: 28.0,
        content_width: 800.0,
    };

    // Huge bounding box covering the whole canvas
    let huge_rect = RectF32::from_points(0.0, 0.0, 2000.0, 2000.0);

    // Grid View: MUST return empty set!
    let grid_inter = RubberbandGeometry::compute_grid_intersections(
        huge_rect,
        &dataset,
        &empty_active_indices,
        grid_config,
    );
    assert!(grid_inter.is_empty(), "Rubberband must never select filtered-out/hidden items in Grid View");

    // Table View: MUST return empty set!
    let table_inter = RubberbandGeometry::compute_table_intersections(
        huge_rect,
        &dataset,
        &empty_active_indices,
        table_config,
    );
    assert!(table_inter.is_empty(), "Rubberband must never select filtered-out/hidden items in Table View");
}

#[test]
fn test_rubberband_modifier_algebra() {
    let baseline: HashSet<MediaId> = HashSet::from([10, 20, 30]);
    let intersected: HashSet<MediaId> = HashSet::from([20, 40]);

    // 1. Normal (No modifier): Replace baseline with intersected
    let normal_sel = RubberbandGeometry::evaluate_modifier_selection(&baseline, &intersected, false, false);
    assert_eq!(normal_sel, HashSet::from([20, 40]));

    // 2. Shift: Union with baseline (10, 20, 30, 40)
    let shift_sel = RubberbandGeometry::evaluate_modifier_selection(&baseline, &intersected, false, true);
    assert_eq!(shift_sel, HashSet::from([10, 20, 30, 40]));

    // 3. Ctrl: Invert / Symmetric Difference against baseline
    let ctrl_sel = RubberbandGeometry::evaluate_modifier_selection(&baseline, &intersected, true, false);
    assert_eq!(ctrl_sel, HashSet::from([10, 30, 40]));
}

#[test]
fn test_rubberband_session_lifecycle() {
    let mut session = RubberbandSession::default();
    assert!(!session.is_active);

    let baseline = HashSet::from([1, 2]);
    session.start(RubberbandStartParams {
        start_content_x: 100.0,
        start_content_y: 150.0,
        view_x: 100.0,
        view_y: 150.0,
        is_ctrl: true,
        is_shift: false,
        baseline: baseline.clone(),
    });
    assert!(session.is_active);
    assert!(session.is_ctrl);
    assert!(!session.is_shift);
    assert_eq!(session.baseline_selection, baseline);

    session.update_content_point(250.0, 300.0, 250.0, 300.0);
    let rect = session.content_rect();
    assert_eq!(rect.x, 100.0);
    assert_eq!(rect.y, 150.0);
    assert_eq!(rect.width, 150.0);
    assert_eq!(rect.height, 150.0);

    session.reset();
    assert!(!session.is_active);
    assert_eq!(session.content_rect().width, 0.0);
}
