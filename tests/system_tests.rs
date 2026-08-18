use rustview::system::{SystemFileHandler, SystemHandlerError};
use std::fs;
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

    // Spawning against mock sample file succeeds in dispatching to the launcher
    let res = SystemFileHandler::open_file(&file_path);
    assert!(res.is_ok());
}
