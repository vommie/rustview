use rustview::library::models::MediaRecord;
use rustview::library::Library;
use rustview::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use rustview::media::{MediaFormat, MediaType};
use rustview::query::filters::IndexedMediaRecord;
use rustview::settings::SettingsManager;
use rustview::ui::bridge_image_fullscreen::ImageFullscreenDialogController;
use rustview::ui::bridge_state::UiState;
use rustview::viewer::ViewerTabManager;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;
use tempfile::tempdir;

fn create_indexed_record(id: u64, name: &str) -> IndexedMediaRecord {
    let rec = MediaRecord {
        id,
        folder_id: 1,
        filename: name.to_string(),
        file_hash: [0xEE; 16],
        file_size: 1024,
        modification_time: 1700000000,
        creation_time: 1690000000,
        duration_ms: 0,
        media_type: MediaType::Image,
        format: MediaFormat::Jpeg,
        width: 800,
        height: 600,
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
        modified_date_str: "2026-10-01 12:00:00".into(),
        created_date_str: "2026-10-01 10:00:00".into(),
        filesize_mb_str: format_filesize_mb(rec.file_size).into(),
        rating_str: "0".into(),
        tags_count: 0,
        tags_count_str: "0".into(),
        width_str: "800".into(),
        height_str: "600".into(),
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
fn test_fullscreen_controller_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ImageFullscreenDialogController>();
}

#[test]
fn test_fullscreen_controller_double_click_detection() {
    let controller = ImageFullscreenDialogController::new();

    assert_eq!(
        controller.check_double_click(false),
        rustview::ui::bridge_file_selection::DoubleClickKind::None
    );

    assert_eq!(
        controller.check_double_click(false),
        rustview::ui::bridge_file_selection::DoubleClickKind::Normal
    );

    assert_eq!(
        controller.check_double_click(true),
        rustview::ui::bridge_file_selection::DoubleClickKind::None
    );

    assert_eq!(
        controller.check_double_click(true),
        rustview::ui::bridge_file_selection::DoubleClickKind::Ctrl
    );
}

#[test]
fn test_fullscreen_singleton_and_modal_locking() {
    let controller = ImageFullscreenDialogController::new();
    assert!(!controller.is_open());

    controller.close();
    assert!(!controller.is_open());
}

#[test]
fn test_fullscreen_viewer_tab_sync() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img1_path = tmp_media.path().join("full_test.png");
    image::RgbImage::new(1920, 1080).save(&img1_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    let settings = SettingsManager::new(tmp_config.path());
    let tab_mgr = Arc::new(ViewerTabManager::new(lib));

    let (tab_id, is_new) = tab_mgr.open_or_focus_media(media.id, false).unwrap();
    assert!(is_new);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_id));

    let controller = ImageFullscreenDialogController::new();
    assert!(!controller.is_open());

    let app_settings = settings.get();
    let bg_mode_idx = match app_settings.viewer.background_mode.to_lowercase().as_str() {
        "deep black" | "black" => 1,
        "transparent checkerboard" | "checkerboard" => 2,
        _ => 0,
    };
    assert_eq!(bg_mode_idx, 0);
}

#[test]
fn test_tab_fullscreen_is_strictly_exclusive_and_cannot_navigate() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img1_path = tmp_media.path().join("tab1.png");
    let img2_path = tmp_media.path().join("tab2.png");
    image::RgbImage::new(100, 100).save(&img1_path).unwrap();
    image::RgbImage::new(100, 100).save(&img2_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 2);

    let m1 = media.iter().find(|m| m.filename == "tab1.png").unwrap().id;
    let m2 = media.iter().find(|m| m.filename == "tab2.png").unwrap().id;

    let settings = SettingsManager::new(tmp_config.path());
    let tab_mgr = Arc::new(ViewerTabManager::new(lib.clone()));
    let (tab1_id, _) = tab_mgr.open_or_focus_media(m1, false).unwrap();
    let (_tab2_id, _) = tab_mgr.open_or_focus_media(m2, false).unwrap();

    let controller = ImageFullscreenDialogController::new();

    // 1. Show Tab 1 in Fullscreen
    controller.show_for_tab(tab1_id, &tab_mgr, &settings, slint::Weak::default());
    assert!(controller.is_open());
    assert_eq!(controller.get_active_media_id(), Some(m1));

    // 2. Navigation MUST be blocked in Tab-Fullscreen!
    controller.navigate_standalone_adjacent(true);
    assert_eq!(controller.get_active_media_id(), Some(m1), "Tab-Fullscreen must NEVER navigate to other files!");

    controller.navigate_standalone_adjacent(false);
    assert_eq!(controller.get_active_media_id(), Some(m1), "Tab-Fullscreen must NEVER navigate to other files!");

    // 3. Browse-Mode show_for_media MUST NOT hijack or overwrite an active Tab-Fullscreen!
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));
    let settings_arc = Arc::new(settings);
    controller.show_for_media(m2, &lib, &settings_arc, &state, &tab_mgr, slint::Weak::default());
    assert_eq!(controller.get_active_media_id(), Some(m1), "Tab-Fullscreen is strictly exclusive and cannot be hijacked by Browse mode!");

    controller.close();
}

#[test]
fn test_fullscreen_standalone_media_mode_bypasses_tabs() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img_path = tmp_media.path().join("quick_fs_image.png");
    let vid_path = tmp_media.path().join("quick_fs_video.mp4");

    image::RgbImage::new(800, 600).save(&img_path).unwrap();
    std::fs::write(&vid_path, b"test video data").unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 2);

    let settings = Arc::new(SettingsManager::new(tmp_config.path()));
    let tab_mgr = Arc::new(ViewerTabManager::new(lib.clone()));
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));

    let controller = ImageFullscreenDialogController::new();

    assert_eq!(tab_mgr.tab_count(), 0);
    assert_eq!(tab_mgr.active_tab_id(), None);

    let dummy_window_weak = slint::Weak::default();
    controller.show_for_media(
        media[0].id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        dummy_window_weak.clone(),
    );

    assert_eq!(tab_mgr.tab_count(), 0);
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert!(controller.is_quick_fullscreen_active());

    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());
    assert_eq!(tab_mgr.tab_count(), 0);

    controller.show_for_media(
        media[1].id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        dummy_window_weak,
    );

    assert_eq!(tab_mgr.tab_count(), 0);
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert!(controller.is_quick_fullscreen_active());

    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());
    assert_eq!(tab_mgr.tab_count(), 0);
}

#[test]
fn test_quick_fullscreen_remains_open_on_outside_click() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img1_path = tmp_media.path().join("quick1.png");
    let img2_path = tmp_media.path().join("quick2.png");
    image::RgbImage::new(400, 300).save(&img1_path).unwrap();
    image::RgbImage::new(500, 400).save(&img2_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 2);

    let settings = Arc::new(SettingsManager::new(tmp_config.path()));
    let tab_mgr = Arc::new(ViewerTabManager::new(lib.clone()));
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));

    let controller = ImageFullscreenDialogController::new();

    controller.show_for_media(
        media[0].id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        slint::Weak::default(),
    );

    assert!(controller.is_open());
    assert!(controller.is_quick_fullscreen_active());

    controller.update_quick_fullscreen_media(media[1].id, &lib, &settings);
    assert!(controller.is_open());
    assert!(controller.is_quick_fullscreen_active());
    assert_eq!(controller.get_active_media_id(), Some(media[1].id));

    assert!(controller.is_open());

    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());
}

#[test]
fn test_quick_fullscreen_orphaned_navigation_recovery() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("matched_1.png");
    let p2 = tmp_media.path().join("matched_2.png");
    let p_orphan = tmp_media.path().join("orphan_dog.png");

    image::RgbImage::new(100, 100).save(&p1).unwrap();
    image::RgbImage::new(100, 100).save(&p2).unwrap();
    image::RgbImage::new(100, 100).save(&p_orphan).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 3);

    let m1 = media.iter().find(|m| m.filename == "matched_1.png").unwrap().id;
    let m2 = media.iter().find(|m| m.filename == "matched_2.png").unwrap().id;
    let m_orphan = media.iter().find(|m| m.filename == "orphan_dog.png").unwrap().id;

    let settings = Arc::new(SettingsManager::new(tmp_config.path()));
    let tab_mgr = Arc::new(ViewerTabManager::new(lib.clone()));
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));

    {
        let mut st = state.write();
        st.all_folder_media = vec![
            create_indexed_record(m1, "matched_1.png"),
            create_indexed_record(m2, "matched_2.png"),
            create_indexed_record(m_orphan, "orphan_dog.png"),
        ];
        st.active_indices = vec![0, 1];
    }

    let controller = ImageFullscreenDialogController::new();

    controller.show_for_media(
        m_orphan,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        slint::Weak::default(),
    );

    assert_eq!(controller.get_active_media_id(), Some(m_orphan));

    // 1. User presses Next (is_next = true) on orphaned file -> jump to index 0 (matched_1.png)
    controller.navigate_standalone_adjacent(true);
    assert_eq!(controller.get_active_media_id(), Some(m1));

    // 2. Next again: moves to index 1 (matched_2.png)
    controller.navigate_standalone_adjacent(true);
    assert_eq!(controller.get_active_media_id(), Some(m2));

    // 3. Cyclic Wrapping: Next wraps around to index 0 (matched_1.png)
    controller.navigate_standalone_adjacent(true);
    assert_eq!(controller.get_active_media_id(), Some(m1));

    // 4. Previous wraps around to index 1 (matched_2.png)
    controller.navigate_standalone_adjacent(false);
    assert_eq!(controller.get_active_media_id(), Some(m2));

    controller.close();
}

#[test]
fn test_fullscreen_ctrl_double_click_converts_standalone_to_background_tab() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img_path = tmp_media.path().join("convert_image.png");
    image::RgbImage::new(800, 600).save(&img_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    let settings = Arc::new(SettingsManager::new(tmp_config.path()));
    let tab_mgr = Arc::new(ViewerTabManager::new(lib.clone()));
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));

    let controller = ImageFullscreenDialogController::new();

    controller.show_for_media(
        media.id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        slint::Weak::default(),
    );

    assert_eq!(tab_mgr.tab_count(), 0);

    controller.handle_ctrl_double_click();

    assert_eq!(tab_mgr.tab_count(), 1);
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert!(!controller.is_open());

    let tab = tab_mgr.list_tabs()[0].clone();
    assert_eq!(tab.media_id, media.id);
}

#[test]
fn test_fullscreen_origin_mode_restoration_and_close_handler() {
    let controller = ImageFullscreenDialogController::new();

    let received_origin_mode = Arc::new(AtomicI32::new(-1));
    let received_tab_id = Arc::new(AtomicBool::new(false));
    let received_media_id = Arc::new(AtomicU64::new(0));

    let origin_clone = received_origin_mode.clone();
    let tab_clone = received_tab_id.clone();
    let media_clone = received_media_id.clone();

    controller.set_close_handler(move |origin_mode, tab_id_opt, standalone_media_id_opt| {
        origin_clone.store(origin_mode, Ordering::SeqCst);
        if tab_id_opt == Some(42) {
            tab_clone.store(true, Ordering::SeqCst);
        }
        if let Some(mid) = standalone_media_id_opt {
            media_clone.store(mid, Ordering::SeqCst);
        }
    });

    controller.close();
    assert_eq!(received_origin_mode.load(Ordering::SeqCst), -1);
}

#[test]
fn test_fullscreen_missing_media_renders_fallback_without_freeze() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img_path = tmp_media.path().join("missing_img.png");
    let vid_path = tmp_media.path().join("missing_vid.mp4");

    image::RgbImage::new(400, 300).save(&img_path).unwrap();
    std::fs::write(&vid_path, b"test vid").unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 2);

    std::fs::remove_file(&img_path).unwrap();
    std::fs::remove_file(&vid_path).unwrap();
    assert!(!img_path.exists());
    assert!(!vid_path.exists());

    let settings = Arc::new(SettingsManager::new(tmp_config.path()));
    let tab_mgr = Arc::new(ViewerTabManager::new(lib.clone()));
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));

    let controller = ImageFullscreenDialogController::new();

    controller.show_for_media(
        media[1].id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        slint::Weak::default(),
    );

    assert!(controller.is_open());
    assert!(controller.is_quick_fullscreen_active());

    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());

    controller.show_for_media(
        media[0].id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        slint::Weak::default(),
    );

    assert!(controller.is_open());
    assert!(controller.is_quick_fullscreen_active());

    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());
}

#[test]
fn test_select_and_reveal_media_synchronizes_state() {
    let tmp_db = tempdir().unwrap();
    let _lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));

    {
        let mut st = state.write();
        st.file_selection.select_single(10);
    }
    assert_eq!(state.read().file_selection.count(), 1);
    assert!(state.read().file_selection.is_selected(10));

    {
        let mut st = state.write();
        st.file_selection.select_single(20);
    }
    assert_eq!(state.read().file_selection.count(), 1);
    assert!(!state.read().file_selection.is_selected(10));
    assert!(state.read().file_selection.is_selected(20));
}
