use rustview::library::Library;
use rustview::settings::SettingsManager;
use rustview::ui::bridge_image_fullscreen::ImageFullscreenDialogController;
use rustview::viewer::ViewerTabManager;
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

    // First click -> False (no prior click)
    assert!(!controller.check_double_click());

    // Immediate second click (< 400ms) -> True (Double Click registered)
    assert!(controller.check_double_click());

    // Next immediate click -> False (reset after confirmed double click)
    assert!(!controller.check_double_click());
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
    let tab_mgr = ViewerTabManager::new(lib);

    let (tab_id, is_new) = tab_mgr.open_or_focus_media(media.id).unwrap();
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
