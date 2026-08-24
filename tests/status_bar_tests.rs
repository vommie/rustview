use rustview::library::models::MediaRecord;
use rustview::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use rustview::media::{MediaFormat, MediaType};
use rustview::query::filters::IndexedMediaRecord;
use rustview::thumbnails::ThumbnailProgressSnapshot;
use rustview::ui::bridge_state::UiState;
use rustview::ui::bridge_status_bar::StatusMessageLevel;
use std::path::PathBuf;

fn create_indexed_record(id: u64, name: &str, size_bytes: u64) -> IndexedMediaRecord {
    let rec = MediaRecord {
        id,
        folder_id: 1,
        filename: name.to_string(),
        file_hash: [0xDD; 16],
        file_size: size_bytes,
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
fn test_status_message_level_enumeration() {
    assert_eq!(StatusMessageLevel::Info as i32, 0);
    assert_eq!(StatusMessageLevel::Success as i32, 1);
    assert_eq!(StatusMessageLevel::Warning as i32, 2);
    assert_eq!(StatusMessageLevel::Error as i32, 3);
    assert_eq!(StatusMessageLevel::default(), StatusMessageLevel::Info);
}

#[test]
fn test_status_bar_metrics_calculation_logic() {
    let mut state = UiState {
        all_folder_media: vec![
            create_indexed_record(1, "img1.jpg", 10 * 1024 * 1024),
            create_indexed_record(2, "img2.jpg", 20 * 1024 * 1024),
            create_indexed_record(3, "img3.jpg", 30 * 1024 * 1024),
        ],
        active_indices: vec![0, 1, 2],
        ..Default::default()
    };

    // Case 1: No selection -> 0 selected items, total items = 3, total size = 60 MB
    let total_count = state.all_folder_media.len();
    let visible_count = state.active_indices.len();
    assert_eq!(total_count, 3);
    assert_eq!(visible_count, 3);
    assert_eq!(state.file_selection.count(), 0);

    let total_bytes: u64 = state.all_folder_media.iter().map(|m| m.record.file_size).sum();
    assert_eq!(total_bytes, 60 * 1024 * 1024);

    // Case 2: Select 2 items (img1 and img3 -> 10 MB + 30 MB = 40 MB)
    state.file_selection.select_single(1);
    state.file_selection.toggle(3);

    assert_eq!(state.file_selection.count(), 2);
    let selected_ids = state.file_selection.selected_ids();

    let selected_bytes: u64 = state
        .all_folder_media
        .iter()
        .filter(|m| selected_ids.contains(&m.record.id))
        .map(|m| m.record.file_size)
        .sum();

    assert_eq!(selected_bytes, 40 * 1024 * 1024);
}

#[test]
fn test_status_bar_thumbnail_progress_snapshot() {
    let snapshot = ThumbnailProgressSnapshot {
        phase: "Generating".to_string(),
        is_preloading: false,
        preload_current: 50,
        preload_total: 50,
        is_generating: true,
        generate_current: 25,
        generate_total: 100,
        active_workers: 4,
    };

    assert!(!snapshot.is_preloading);
    assert!(snapshot.is_generating);
    assert_eq!(snapshot.generate_current, 25);
    assert_eq!(snapshot.generate_total, 100);
    assert_eq!(snapshot.active_workers, 4);
}

#[test]
fn test_status_bar_virtual_folder_state() {
    let mut state = UiState {
        is_virtual_folder_mode: true,
        virtual_folder_count: 5,
        ..Default::default()
    };

    assert!(state.is_virtual_folder_mode);
    assert_eq!(state.virtual_folder_count, 5);

    state.is_virtual_folder_mode = false;
    state.virtual_folder_count = 0;
    assert!(!state.is_virtual_folder_mode);
    assert_eq!(state.virtual_folder_count, 0);
}
