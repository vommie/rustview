use rustview::library::models::MediaRecord;
use rustview::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use rustview::media::{MediaFormat, MediaType};
use rustview::query::filters::IndexedMediaRecord;
use rustview::ui::bridge_state::UiState;
use rustview::ui::keyboard_navigation::{
    calculate_target_index, NavigationDirection,
};
use std::path::PathBuf;

#[test]
fn test_calculate_target_index_grid_arrows() {
    // 12 items in 4 columns (3 rows: 0..4, 4..8, 8..12)
    let total_items = 12;
    let columns = 4;
    let page_rows = 2;

    // Current at index 5 (Row 1, Col 1)
    let cur = 5;

    // Up -> Row 0, Col 1 = 1
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Up, total_items, columns, page_rows),
        Some(1)
    );

    // Down -> Row 2, Col 1 = 9
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Down, total_items, columns, page_rows),
        Some(9)
    );

    // Left -> 4
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Left, total_items, columns, page_rows),
        Some(4)
    );

    // Right -> 6
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Right, total_items, columns, page_rows),
        Some(6)
    );

    // Boundary: Up from Row 0 (item 1) -> None
    assert_eq!(
        calculate_target_index(1, NavigationDirection::Up, total_items, columns, page_rows),
        None
    );

    // Boundary: Left from item 0 -> None
    assert_eq!(
        calculate_target_index(0, NavigationDirection::Left, total_items, columns, page_rows),
        None
    );

    // Boundary: Right from item 11 -> None
    assert_eq!(
        calculate_target_index(11, NavigationDirection::Right, total_items, columns, page_rows),
        None
    );

    // Boundary: Down from Row 2 (item 9) -> None
    assert_eq!(
        calculate_target_index(9, NavigationDirection::Down, total_items, columns, page_rows),
        None
    );
}

#[test]
fn test_calculate_target_index_grid_incomplete_last_row() {
    // 10 items in 4 columns:
    // Row 0: 0, 1, 2, 3
    // Row 1: 4, 5, 6, 7
    // Row 2: 8, 9 (Cols 2 and 3 do not exist!)
    let total_items = 10;
    let columns = 4;
    let page_rows = 2;

    // Moving down from item 7 (Row 1, Col 3) -> Candidate is 1 * 4 + 3 = 11 >= 10.
    // Must land on last item of incomplete row (index 9)!
    assert_eq!(
        calculate_target_index(7, NavigationDirection::Down, total_items, columns, page_rows),
        Some(9)
    );

    // Moving down from item 6 (Row 1, Col 2) -> Candidate is 10 >= 10 -> lands on 9!
    assert_eq!(
        calculate_target_index(6, NavigationDirection::Down, total_items, columns, page_rows),
        Some(9)
    );

    // Moving down from item 5 (Row 1, Col 1) -> Candidate is 9 < 10 -> lands on 9
    assert_eq!(
        calculate_target_index(5, NavigationDirection::Down, total_items, columns, page_rows),
        Some(9)
    );

    // Moving down from item 4 (Row 1, Col 0) -> Candidate is 8 < 10 -> lands on 8
    assert_eq!(
        calculate_target_index(4, NavigationDirection::Down, total_items, columns, page_rows),
        Some(8)
    );
}

#[test]
fn test_calculate_target_index_table_arrows() {
    // 10 items in single-column TableView (columns == 1)
    let total_items = 10;
    let columns = 1;
    let page_rows = 3;

    // Current at row 5
    let cur = 5;

    // Up -> 4
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Up, total_items, columns, page_rows),
        Some(4)
    );

    // Down -> 6
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Down, total_items, columns, page_rows),
        Some(6)
    );

    // Left in TableView -> None (does not jump rows)
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Left, total_items, columns, page_rows),
        None
    );

    // Right in TableView -> None
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::Right, total_items, columns, page_rows),
        None
    );

    // Boundary: Up from 0 -> None
    assert_eq!(
        calculate_target_index(0, NavigationDirection::Up, total_items, columns, page_rows),
        None
    );

    // Boundary: Down from 9 -> None
    assert_eq!(
        calculate_target_index(9, NavigationDirection::Down, total_items, columns, page_rows),
        None
    );
}

#[test]
fn test_calculate_target_index_page_up_down_grid_column_preservation() {
    // 30 items in 3 columns (10 rows: 0..=9).
    let total_items = 30;
    let columns = 3;
    let page_rows = 4;

    // Current at index 4 (Row 1, Column 1)
    let cur = 4;

    // PageDown: Row 1 + 4 = Row 5, Column 1 -> 5 * 3 + 1 = 16
    assert_eq!(
        calculate_target_index(cur, NavigationDirection::PageDown, total_items, columns, page_rows),
        Some(16)
    );

    // From 16, PageDown again: Row 5 + 4 = Row 9, Column 1 -> 9 * 3 + 1 = 28
    assert_eq!(
        calculate_target_index(16, NavigationDirection::PageDown, total_items, columns, page_rows),
        Some(28)
    );

    // From 28, PageUp: Row 9 - 4 = Row 5, Column 1 -> 5 * 3 + 1 = 16
    assert_eq!(
        calculate_target_index(28, NavigationDirection::PageUp, total_items, columns, page_rows),
        Some(16)
    );

    // From 16, PageUp: Row 5 - 4 = Row 1, Column 1 -> 4
    assert_eq!(
        calculate_target_index(16, NavigationDirection::PageUp, total_items, columns, page_rows),
        Some(4)
    );

    // From 4, PageUp: Row 1 - 4 saturates to Row 0, Column 1 -> 0 * 3 + 1 = 1
    assert_eq!(
        calculate_target_index(4, NavigationDirection::PageUp, total_items, columns, page_rows),
        Some(1)
    );
}

#[test]
fn test_calculate_target_index_home_end() {
    let total_items = 25;
    let columns = 5;
    let page_rows = 3;

    assert_eq!(
        calculate_target_index(12, NavigationDirection::Home, total_items, columns, page_rows),
        Some(0)
    );

    assert_eq!(
        calculate_target_index(12, NavigationDirection::End, total_items, columns, page_rows),
        Some(24)
    );

    // Already at Home
    assert_eq!(
        calculate_target_index(0, NavigationDirection::Home, total_items, columns, page_rows),
        None
    );

    // Already at End
    assert_eq!(
        calculate_target_index(24, NavigationDirection::End, total_items, columns, page_rows),
        None
    );
}

fn create_dummy_record(id: u64, name: &str) -> IndexedMediaRecord {
    let rec = MediaRecord {
        id,
        folder_id: 1,
        filename: name.to_string(),
        file_hash: [0xAA; 16],
        file_size: 1024,
        modification_time: 1700000000,
        creation_time: 1690000000,
        duration_ms: 0,
        media_type: MediaType::Image,
        format: MediaFormat::Jpeg,
        width: 100,
        height: 100,
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
        width_str: "100".into(),
        height_str: "100".into(),
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
fn test_shift_selection_expansion_and_reduction_lifecycle() {
    let mut state = UiState::default();
    let total = 10;
    for i in 0..total {
        let item = create_dummy_record(i as u64, &format!("item_{}.jpg", i));
        state.all_folder_media.push(item);
        state.active_indices.push(i);
        state.media_row_map.insert(i as u64, i);
    }

    // 1. Initial selection at item 4 (without Shift)
    state.file_selection.select_single(4);
    assert_eq!(state.file_selection.count(), 1);
    assert_eq!(state.file_selection.anchor_id(), Some(4));
    assert_eq!(state.file_selection.last_clicked_id(), Some(4));

    let visible_ids: Vec<u64> = (0..total).map(|i| i as u64).collect();

    // 2. Hold Shift and navigate to item 7 -> Range expands [4, 5, 6, 7]
    state.file_selection.select_range(7, &visible_ids, false);
    assert_eq!(state.file_selection.count(), 4);
    assert_eq!(state.file_selection.anchor_id(), Some(4), "Anchor must remain 4");
    assert_eq!(state.file_selection.last_clicked_id(), Some(7));
    assert!(state.file_selection.is_selected(4));
    assert!(state.file_selection.is_selected(5));
    assert!(state.file_selection.is_selected(6));
    assert!(state.file_selection.is_selected(7));
    assert!(!state.file_selection.is_selected(8));

    // 3. Hold Shift and navigate to item 8 -> Range expands to [4, 5, 6, 7, 8]
    state.file_selection.select_range(8, &visible_ids, false);
    assert_eq!(state.file_selection.count(), 5);
    assert!(state.file_selection.is_selected(8));

    // 4. Hold Shift and navigate back to item 6 -> Range shrinks to [4, 5, 6]
    state.file_selection.select_range(6, &visible_ids, false);
    assert_eq!(state.file_selection.count(), 3);
    assert!(state.file_selection.is_selected(4));
    assert!(state.file_selection.is_selected(5));
    assert!(state.file_selection.is_selected(6));
    assert!(!state.file_selection.is_selected(7), "Item 7 must be unselected as range shrinks");
    assert!(!state.file_selection.is_selected(8), "Item 8 must be unselected as range shrinks");

    // 5. Hold Shift and navigate back to anchor (item 4) -> Range shrinks to [4]
    state.file_selection.select_range(4, &visible_ids, false);
    assert_eq!(state.file_selection.count(), 1);
    assert!(state.file_selection.is_selected(4));

    // 6. Hold Shift and navigate past anchor to item 2 -> Range expands backwards [2, 3, 4]
    state.file_selection.select_range(2, &visible_ids, false);
    assert_eq!(state.file_selection.count(), 3);
    assert_eq!(state.file_selection.anchor_id(), Some(4));
    assert!(state.file_selection.is_selected(2));
    assert!(state.file_selection.is_selected(3));
    assert!(state.file_selection.is_selected(4));

    // 7. Release Shift, navigate to item 1 -> Single select item 1, clears previous selection, anchor becomes 1
    state.file_selection.select_single(1);
    assert_eq!(state.file_selection.count(), 1);
    assert!(state.file_selection.is_selected(1));
    assert!(!state.file_selection.is_selected(2));
    assert!(!state.file_selection.is_selected(4));
    assert_eq!(state.file_selection.anchor_id(), Some(1));
    assert_eq!(state.file_selection.last_clicked_id(), Some(1));
}

#[test]
fn test_empty_selection_navigation_starts_at_zero() {
    let total_items = 10;
    let columns = 3;
    let page_rows = 2;

    // When nothing is selected, starting at index 0 and moving Down lands on index 3
    let target = calculate_target_index(0, NavigationDirection::Down, total_items, columns, page_rows);
    assert_eq!(target, Some(3));

    // Moving Right from 0 lands on index 1
    let target_right = calculate_target_index(0, NavigationDirection::Right, total_items, columns, page_rows);
    assert_eq!(target_right, Some(1));
}
