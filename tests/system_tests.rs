use rustview::system::{DesktopApp, DesktopIntegration, SystemClipboard, SystemFileHandler, SystemHandlerError};
use std::fs;
use std::sync::mpsc::channel;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_system_file_handler_missing_file_returns_error() {
    let non_existent_path = std::path::PathBuf::from("/tmp/rustview_non_existent_file_12345.png");

    let result = SystemFileHandler::open_file(&non_existent_path);
    assert!(result.is_err());
    match result.unwrap_err() {
        SystemHandlerError::FileNotFound(p) => assert_eq!(p, non_existent_path),
        other => panic!("Expected SystemHandlerError::FileNotFound, got {:?}", other),
    }
}

#[test]
fn test_system_file_handler_missing_directory_returns_error() {
    let non_existent_dir = std::path::PathBuf::from("/tmp/rustview_non_existent_dir_99999/");

    let result = SystemFileHandler::open_directory(&non_existent_dir);
    assert!(result.is_err());
    match result.unwrap_err() {
        SystemHandlerError::FileNotFound(p) => assert_eq!(p, non_existent_dir),
        other => panic!("Expected SystemHandlerError::FileNotFound, got {:?}", other),
    }
}

#[test]
fn test_system_file_handler_valid_file_path_resolution() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("sample_media.jpg");
    fs::write(&file_path, b"sample image bytes").unwrap();

    assert!(file_path.exists());

    let res = SystemFileHandler::open_file(&file_path);
    assert!(res.is_ok());
}

#[test]
fn test_system_file_handler_folder_and_reveal_apis() {
    let tmp = tempdir().unwrap();
    let folder_dir = tmp.path().join("sub_photos");
    fs::create_dir_all(&folder_dir).unwrap();

    let file_a = folder_dir.join("photo_a.png");
    let file_b = folder_dir.join("photo_b.png");
    fs::write(&file_a, b"image a").unwrap();
    fs::write(&file_b, b"image b").unwrap();

    // 1. Test open_folder API
    let res_folder = SystemFileHandler::open_folder(&folder_dir);
    assert!(res_folder.is_ok());

    // 2. Test open_directory alias
    let res_dir = SystemFileHandler::open_directory(&folder_dir);
    assert!(res_dir.is_ok());

    // 3. Test reveal_in_file_manager with multiple paths
    let res_reveal_multi = SystemFileHandler::reveal_in_file_manager(&[&file_a, &file_b]);
    assert!(res_reveal_multi.is_ok());

    // 4. Test reveal_single_in_file_manager
    let res_reveal_single = SystemFileHandler::reveal_single_in_file_manager(&file_a);
    assert!(res_reveal_single.is_ok());
}

#[test]
fn test_desktop_integration_associated_apps_and_launcher_building() {
    let tmp = tempdir().unwrap();
    let img_path = tmp.path().join("landscape.jpg");
    fs::write(&img_path, b"fake jpeg payload").unwrap();

    let apps = DesktopIntegration::get_associated_apps(&img_path);
    tracing::info!("Found {} associated desktop apps for JPEG", apps.len());

    let mock_app = DesktopApp {
        id: "gimp.desktop".to_string(),
        name: "GIMP".to_string(),
        icon_name: "gimp".to_string(),
        icon_path: None,
        exec: "gimp-2.10 -s %U".to_string(),
    };

    let res = DesktopIntegration::launch_app(&mock_app, &[&img_path]);
    assert!(res.is_ok());
}

#[test]
fn test_system_clipboard_mime_type_detection() {
    assert_eq!(
        SystemClipboard::mime_type_from_path(std::path::Path::new("photo.jpg")),
        "image/jpeg"
    );
    assert_eq!(
        SystemClipboard::mime_type_from_path(std::path::Path::new("photo.jpeg")),
        "image/jpeg"
    );
    assert_eq!(
        SystemClipboard::mime_type_from_path(std::path::Path::new("image.png")),
        "image/png"
    );
    assert_eq!(
        SystemClipboard::mime_type_from_path(std::path::Path::new("graphic.webp")),
        "image/webp"
    );
    assert_eq!(
        SystemClipboard::mime_type_from_path(std::path::Path::new("anim.gif")),
        "image/gif"
    );
    assert_eq!(
        SystemClipboard::mime_type_from_path(std::path::Path::new("icon.svg")),
        "image/svg+xml"
    );
    assert_eq!(
        SystemClipboard::mime_type_from_path(std::path::Path::new("clip.mp4")),
        "video/mp4"
    );
}

#[test]
fn test_system_clipboard_text_and_lists() {
    // 1. Text copy
    let res = SystemClipboard::set_text("RustView Clipboard Integration Test");
    assert!(res.is_ok());

    // 2. File names copy
    let names = vec!["photo1.jpg", "video1.mp4", "art.png"];
    let copied_count = SystemClipboard::copy_file_names(&names).unwrap();
    assert_eq!(copied_count, 3);

    // 3. File paths copy
    let paths = vec![
        std::path::Path::new("/media/photos/photo1.jpg"),
        std::path::Path::new("/media/videos/video1.mp4"),
    ];
    let copied_paths_count = SystemClipboard::copy_file_paths(&paths).unwrap();
    assert_eq!(copied_paths_count, 2);
}

#[test]
fn test_system_clipboard_image_copying_sync_and_async() {
    let tmp = tempdir().unwrap();
    let img_path = tmp.path().join("clipboard_test_image.png");

    let img = image::RgbImage::new(128, 96);
    img.save(&img_path).unwrap();

    assert!(img_path.exists());

    // 1. Test synchronous copy of image
    let res_sync = SystemClipboard::copy_image_from_path_sync(&img_path);
    assert!(res_sync.is_ok());
    let (w, h, metrics) = res_sync.unwrap();
    assert_eq!(w, 128);
    assert_eq!(h, 96);
    assert!(metrics.file_size_bytes > 0);
    assert!(metrics.success);

    // 2. Test asynchronous non-blocking worker thread dispatch
    let (tx, rx) = channel();
    SystemClipboard::copy_image_from_path_async(img_path.clone(), move |res| {
        tx.send(res).unwrap();
    });

    let async_res = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("Async clipboard worker timed out");

    assert!(async_res.is_ok());
    let (w_async, h_async, metrics_async) = async_res.unwrap();
    assert_eq!(w_async, 128);
    assert_eq!(h_async, 96);
    assert!(metrics_async.success);
}
