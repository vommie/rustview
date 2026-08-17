use rustview::cli_api::{parse_api_args, run_api_with_library, run_cli_api};
use rustview::library::Library;
use tempfile::tempdir;

#[test]
fn test_cli_api_lifecycle_full_suite() {
    let tmp_data = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let db_path = tmp_data.path().join("library.lmdb");
    let lib = Library::open(&db_path).unwrap();

    // 1. Create Tags in Library
    let t_nature = lib.tags().create_tag("Nature", None).unwrap();
    let t_forest = lib.tags().create_tag("Forest", Some(t_nature.id)).unwrap();

    // 2. Test `tag-tree` via API
    let args_tree = vec![
        "rustview".to_string(),
        "api".to_string(),
        "tag-tree".to_string(),
        "--json".to_string(),
    ];
    let cmd_tree = parse_api_args(&args_tree).unwrap();
    assert_eq!(run_api_with_library(cmd_tree, &lib).unwrap(), 0);

    // 3. Create dummy media file on disk
    let file_path = tmp_media.path().join("clip01.mp4");
    std::fs::write(&file_path, b"test video mp4 content").unwrap();

    // 4. Test `set-media` (Creation + Rating 5 + Tags Nature & Forest)
    let args_set = vec![
        "rustview".to_string(),
        "api".to_string(),
        "set-media".to_string(),
        "--path".to_string(),
        file_path.to_string_lossy().to_string(),
        "--rating".to_string(),
        "5".to_string(),
        "--tags".to_string(),
        format!("{},{}", t_nature.id, t_forest.id),
        "--json".to_string(),
    ];
    let cmd_set = parse_api_args(&args_set).unwrap();
    assert_eq!(run_api_with_library(cmd_set, &lib).unwrap(), 0);

    // 5. Test `get-media`
    let args_get = vec![
        "rustview".to_string(),
        "api".to_string(),
        "get-media".to_string(),
        "--path".to_string(),
        file_path.to_string_lossy().to_string(),
        "--json".to_string(),
    ];
    let cmd_get = parse_api_args(&args_get).unwrap();
    assert_eq!(run_api_with_library(cmd_get, &lib).unwrap(), 0);

    // 6. Test `set-media` (Update Rating to 4 and Tag to Forest only)
    let args_update = vec![
        "rustview".to_string(),
        "api".to_string(),
        "set-media".to_string(),
        "--path".to_string(),
        file_path.to_string_lossy().to_string(),
        "--rating".to_string(),
        "4".to_string(),
        "--tags".to_string(),
        format!("{}", t_forest.id),
        "--json".to_string(),
    ];
    let cmd_update = parse_api_args(&args_update).unwrap();
    assert_eq!(run_api_with_library(cmd_update, &lib).unwrap(), 0);

    // Verify LMDB Content directly
    let folder = lib.folders().get_folder_by_path(tmp_media.path()).unwrap().unwrap();
    let media_list = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media_list.len(), 1);
    assert_eq!(media_list[0].filename, "clip01.mp4");
    assert_eq!(media_list[0].rating, 4);

    let assigned_tags = lib.tags().get_tags_for_media(media_list[0].id).unwrap();
    assert_eq!(assigned_tags.len(), 1);
    assert_eq!(assigned_tags[0].name, "Forest");

    // 7. Test `insert-hash`
    let dummy_hash = "d5dd11d265f453dee9d51a40ba717efb";
    let args_insert_hash = vec![
        "rustview".to_string(),
        "api".to_string(),
        "insert-hash".to_string(),
        "--hash".to_string(),
        dummy_hash.to_string(),
        "--json".to_string(),
    ];
    let cmd_insert_hash = parse_api_args(&args_insert_hash).unwrap();
    assert_eq!(run_api_with_library(cmd_insert_hash, &lib).unwrap(), 0);

    // 8. Test `check-hash`
    let args_check_hash = vec![
        "rustview".to_string(),
        "api".to_string(),
        "check-hash".to_string(),
        "--hash".to_string(),
        dummy_hash.to_string(),
        "--json".to_string(),
    ];
    let cmd_check_hash = parse_api_args(&args_check_hash).unwrap();
    assert_eq!(run_api_with_library(cmd_check_hash, &lib).unwrap(), 0);

    // 9. Test `delete-media`
    let args_delete = vec![
        "rustview".to_string(),
        "api".to_string(),
        "delete-media".to_string(),
        "--path".to_string(),
        file_path.to_string_lossy().to_string(),
        "--json".to_string(),
    ];
    let cmd_delete = parse_api_args(&args_delete).unwrap();
    assert_eq!(run_api_with_library(cmd_delete, &lib).unwrap(), 0);

    let media_after_delete = lib.media().get_by_folder(folder.id).unwrap();
    assert!(media_after_delete.is_empty());
}

#[test]
fn test_cli_api_standalone_invocation() {
    let tmp_data = tempdir().unwrap();

    let args_help = vec![
        "rustview".to_string(),
        "api".to_string(),
        "--help".to_string(),
    ];
    assert_eq!(run_cli_api(&args_help, tmp_data.path()).unwrap(), 0);
}
