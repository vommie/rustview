use rustview::library::error::LibraryError;
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

#[test]
fn test_file_operations_media_refresh_lifecycle() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib
        .folders()
        .add_folder(tmp_media.path())
        .expect("Failed to add folder");

    let img_path = tmp_media.path().join("test_refresh.png");
    let img = image::RgbImage::new(100, 100);
    img.save(&img_path).unwrap();

    lib.refresher().refresh_folder(folder.id).unwrap();
    let media_list = lib.media().get_by_folder(folder.id).unwrap();
    let media_id = media_list[0].id;
    let old_hash = media_list[0].file_hash;
    assert_eq!(media_list[0].width, 100);
    assert_eq!(media_list[0].height, 100);

    // Set rating and tag to verify they are strictly preserved
    lib.media().set_rating(media_id, 4).unwrap();
    let tag = lib.tags().create_tag("TestTag", None).unwrap();
    lib.tags().assign_tag(media_id, tag.id).unwrap();

    // Overwrite the file with a 200x300 image
    let new_img = image::RgbImage::new(200, 300);
    new_img.save(&img_path).unwrap();

    // Execute refresh_media via file_operations() as well as media_refresher()
    let refreshed = lib
        .file_operations()
        .refresh_media(media_id)
        .expect("Refresh media failed");

    assert_eq!(refreshed.id, media_id);
    assert_eq!(refreshed.width, 200);
    assert_eq!(refreshed.height, 300);
    assert_ne!(refreshed.file_hash, old_hash);
    assert_eq!(refreshed.rating, 4); // Rating preserved!

    // Verify tags preserved
    let tags = lib.tags().get_tags_for_media(media_id).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "TestTag");

    // Verify hash lookup updated
    let by_new_hash = lib.media().get_by_hash(&refreshed.file_hash).unwrap();
    assert_eq!(by_new_hash.len(), 1);
    assert_eq!(by_new_hash[0].id, media_id);

    let by_old_hash = lib.media().get_by_hash(&old_hash).unwrap();
    assert!(by_old_hash.is_empty());

    // Also test direct media_refresher() interface
    let re_refreshed = lib
        .media_refresher()
        .refresh_media(media_id)
        .expect("MediaRefresher interface failed");
    assert_eq!(re_refreshed.id, media_id);
    assert_eq!(re_refreshed.width, 200);
}

#[test]
fn test_file_operations_missing_file_error() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib
        .folders()
        .add_folder(tmp_media.path())
        .expect("Failed to add folder");

    let img_path = tmp_media.path().join("transient.png");
    let img = image::RgbImage::new(50, 50);
    img.save(&img_path).unwrap();

    lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    // Delete the file from filesystem to simulate missing file
    std::fs::remove_file(&img_path).unwrap();

    // refresh_media must return LibraryError::Io with NotFound
    let res = lib.file_operations().refresh_media(media.id);
    assert!(res.is_err());
    match res.unwrap_err() {
        LibraryError::Io(err) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
        other => panic!("Expected LibraryError::Io(NotFound), got {:?}", other),
    }
}
