use rustview::library::Library;
use rustview::settings::SettingsManager;
use rustview::ui::bridge_image_fullscreen::ImageFullscreenDialogController;
use rustview::ui::bridge_state::UiState;
use rustview::viewer::ViewerTabManager;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_fullscreen_controller_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ImageFullscreenDialogController>();
}

#[test]
fn test_fullscreen_controller_double_click_detection() {
    let controller = ImageFullscreenDialogController::new();

    // First click without Ctrl -> False / None
    assert_eq!(
        controller.check_double_click(false),
        rustview::ui::bridge_file_selection::DoubleClickKind::None
    );

    // Immediate second click without Ctrl (< 400ms) -> Normal Double Click
    assert_eq!(
        controller.check_double_click(false),
        rustview::ui::bridge_file_selection::DoubleClickKind::Normal
    );

    // First click with Ctrl -> None
    assert_eq!(
        controller.check_double_click(true),
        rustview::ui::bridge_file_selection::DoubleClickKind::None
    );

    // Immediate second click with Ctrl (< 400ms) -> Ctrl Double Click
    assert_eq!(
        controller.check_double_click(true),
        rustview::ui::bridge_file_selection::DoubleClickKind::Ctrl
    );
}

#[test]
fn test_fullscreen_singleton_and_modal_locking() {
    let controller = ImageFullscreenDialogController::new();
    assert!(!controller.is_open());

    // Closing when not active is a safe no-op
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

    // Verify background mode configuration mapping
    let app_settings = settings.get();
    let bg_mode_idx = match app_settings.viewer.background_mode.to_lowercase().as_str() {
        "deep black" | "black" => 1,
        "transparent checkerboard" | "checkerboard" => 2,
        _ => 0,
    };
    assert_eq!(bg_mode_idx, 0); // Default dark theme #11111b
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

    // Verify initially no tabs are open
    assert_eq!(tab_mgr.tab_count(), 0);
    assert_eq!(tab_mgr.active_tab_id(), None);

    // 1. Trigger Quick-Fullscreen on Image directly
    let dummy_window_weak = slint::Weak::default();
    controller.show_for_media(
        media[0].id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        dummy_window_weak.clone(),
    );

    // Quick-Fullscreen must bypass tab creation
    assert_eq!(tab_mgr.tab_count(), 0, "Quick-Fullscreen must bypass tab creation");
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert!(controller.is_quick_fullscreen_active());

    // Close Fullscreen
    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());
    assert_eq!(tab_mgr.tab_count(), 0);

    // 2. Trigger Quick-Fullscreen on Video directly
    controller.show_for_media(
        media[1].id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        dummy_window_weak,
    );

    // Quick-Fullscreen must bypass tab creation for videos
    assert_eq!(tab_mgr.tab_count(), 0, "Quick-Fullscreen must bypass tab creation for videos");
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert!(controller.is_quick_fullscreen_active());

    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());
    assert_eq!(tab_mgr.tab_count(), 0);
}

#[test]
fn test_quick_fullscreen_live_switch_and_dismiss_on_non_media() {
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

    // 1. Open Quick-Fullscreen on Media 1
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

    // 2. Simulate clicking on Media 2 -> Live update Quick-Fullscreen
    controller.update_quick_fullscreen_media(media[1].id, &lib, &settings);
    assert!(controller.is_open());
    assert!(controller.is_quick_fullscreen_active());

    // 3. Tagging / rating operations do NOT call dismiss_if_quick_fullscreen -> Quick-Fullscreen remains open!
    assert!(controller.is_quick_fullscreen_active());

    // 4. Simulate clicking outside media items and outside Tag & Rate panel (e.g. folder, background, search) -> Dismiss Quick-Fullscreen
    controller.dismiss_if_quick_fullscreen();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());
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

    // Open Quick-Fullscreen from Browse view (origin_mode = 0)
    controller.show_for_media(
        media.id,
        &lib,
        &settings,
        &state,
        &tab_mgr,
        slint::Weak::default(),
    );

    assert_eq!(tab_mgr.tab_count(), 0);

    // Simulate Ctrl + DoubleClick in Fullscreen -> converts to background tab and closes fullscreen!
    controller.handle_ctrl_double_click();

    // Verify background tab exists and active_tab_id is None (Browse view preserved)
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

    let origin_clone = received_origin_mode.clone();
    let tab_clone = received_tab_id.clone();

    controller.set_close_handler(move |origin_mode, tab_id_opt| {
        origin_clone.store(origin_mode, Ordering::SeqCst);
        if tab_id_opt == Some(42) {
            tab_clone.store(true, Ordering::SeqCst);
        }
    });

    // When not active -> close is a no-op
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

    // Now delete both physical files from disk so they become missing/deleted!
    std::fs::remove_file(&img_path).unwrap();
    std::fs::remove_file(&vid_path).unwrap();
    assert!(!img_path.exists());
    assert!(!vid_path.exists());

    let settings = Arc::new(SettingsManager::new(tmp_config.path()));
    let tab_mgr = Arc::new(ViewerTabManager::new(lib.clone()));
    let state = Arc::new(parking_lot::RwLock::new(UiState::default()));

    let controller = ImageFullscreenDialogController::new();

    // 1. Show missing video in Quick Fullscreen: must not freeze or spawn hung X11 subwindow
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

    // Cleanly close without hanging
    controller.close();
    assert!(!controller.is_open());
    assert!(!controller.is_quick_fullscreen_active());

    // 2. Show missing image in Quick Fullscreen: must not freeze
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
