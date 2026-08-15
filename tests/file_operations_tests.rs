use rustview::library::Library;
use tempfile::tempdir;

#[test]
fn test_file_operations_media_rename_lifecycle() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib
        .folders()
        .add_folder(tmp_media.path())
        .expect("Failed to add folder");

    let original_path = tmp_media.path().join("old_name.png");
    let img = image::RgbImage::new(100, 100);
    img.save(&original_path).unwrap();

    let report = lib.refresher().refresh_folder(folder.id).unwrap();
    assert_eq!(report.new_media, 1);

    let media_list = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media_list.len(), 1);
    let original_media_id = media_list[0].id;
    assert_eq!(media_list[0].filename, "old_name.png");

    // 1. Successful Rename via file_operations()
    let updated = lib
        .file_operations()
        .rename_media(original_media_id, "new_name.png")
        .expect("Rename operation failed");

    assert_eq!(updated.id, original_media_id);
    assert_eq!(updated.filename, "new_name.png");
    assert!(!original_path.exists());
    assert!(tmp_media.path().join("new_name.png").exists());

    // Verify LMDB state directly
    let reloaded = lib.media().get(original_media_id).unwrap().unwrap();
    assert_eq!(reloaded.filename, "new_name.png");

    // 2. Reject Invalid Filenames
    assert!(
        lib.file_operations()
            .rename_media(original_media_id, "   ")
            .is_err()
    );
    assert!(
        lib.file_operations()
            .rename_media(original_media_id, "foo/bar.png")
            .is_err()
    );
    assert!(
        lib.file_operations()
            .rename_media(original_media_id, "foo\\bar.png")
            .is_err()
    );

    // 3. Collision Protection
    let other_path = tmp_media.path().join("existing.png");
    img.save(&other_path).unwrap();
    assert!(
        lib.file_operations()
            .rename_media(original_media_id, "existing.png")
            .is_err()
    );
}
