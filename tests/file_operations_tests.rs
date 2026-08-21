use rustview::library::error::LibraryError;
use rustview::library::file_operations::FileOperations;
use rustview::library::models::FileTransferKind;
use rustview::library::Library;
use rustview::ui::{MissingFileDialogController, MissingFileItem};
use std::path::PathBuf;
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

#[test]
fn test_generate_unique_filename_iteration() {
    let tmp_dir = tempdir().unwrap();
    let folder = tmp_dir.path();

    // No existing files -> returns candidate "photo (1).jpg"
    std::fs::write(folder.join("photo.jpg"), b"base").unwrap();
    let cand1 = FileOperations::generate_unique_filename(folder, "photo.jpg");
    assert_eq!(cand1, "photo (1).jpg");

    // If "photo (1).jpg" exists -> returns "photo (2).jpg"
    std::fs::write(folder.join("photo (1).jpg"), b"1").unwrap();
    let cand2 = FileOperations::generate_unique_filename(folder, "photo.jpg");
    assert_eq!(cand2, "photo (2).jpg");

    // If "photo (2).jpg" exists -> returns "photo (3).jpg"
    std::fs::write(folder.join("photo (2).jpg"), b"2").unwrap();
    let cand3 = FileOperations::generate_unique_filename(folder, "photo.jpg");
    assert_eq!(cand3, "photo (3).jpg");

    // File without extension
    std::fs::write(folder.join("data"), b"raw").unwrap();
    let cand_no_ext = FileOperations::generate_unique_filename(folder, "data");
    assert_eq!(cand_no_ext, "data (1)");
}

#[test]
fn test_check_move_conflict_and_move_single_media() {
    let tmp_db = tempdir().unwrap();
    let tmp_media_a = tempdir().unwrap();
    let tmp_media_b = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder_a = lib.folders().add_folder(tmp_media_a.path()).unwrap();
    let folder_b = lib.folders().add_folder(tmp_media_b.path()).unwrap();

    let img_a_path = tmp_media_a.path().join("vacation.png");
    image::RgbImage::new(1920, 1080).save(&img_a_path).unwrap();

    lib.refresher().refresh_folder(folder_a.id).unwrap();
    let media_a = lib.media().get_by_folder(folder_a.id).unwrap()[0].clone();

    // 1. Check conflict when destination has NO file with that name -> returns None
    let conflict_none = lib
        .file_operations()
        .check_transfer_conflict(media_a.id, folder_b.id, FileTransferKind::Move)
        .unwrap();
    assert!(conflict_none.is_none());

    // 2. Perform direct move
    let moved_record = lib
        .file_operations()
        .move_single_media(media_a.id, folder_b.id, None, false)
        .expect("Move failed");

    assert_eq!(moved_record.folder_id, folder_b.id);
    assert_eq!(moved_record.filename, "vacation.png");
    assert!(!img_a_path.exists());
    assert!(tmp_media_b.path().join("vacation.png").exists());

    // Verify folder media counts in LMDB
    assert_eq!(lib.media().get_by_folder(folder_a.id).unwrap().len(), 0);
    assert_eq!(lib.media().get_by_folder(folder_b.id).unwrap().len(), 1);

    // 3. Create a new file in folder A with the same name "vacation.png"
    image::RgbImage::new(800, 600).save(&img_a_path).unwrap();
    lib.refresher().refresh_folder(folder_a.id).unwrap();
    let media_a2 = lib.media().get_by_folder(folder_a.id).unwrap()[0].clone();

    // Check conflict -> must detect collision!
    let conflict_report = lib
        .file_operations()
        .check_transfer_conflict(media_a2.id, folder_b.id, FileTransferKind::Move)
        .unwrap()
        .expect("Conflict must be detected");

    assert_eq!(conflict_report.source.filename, "vacation.png");
    assert_eq!(conflict_report.source.width, 800);
    assert_eq!(conflict_report.destination.filename, "vacation.png");
    assert_eq!(conflict_report.destination.width, 1920);

    // 4. Test Auto-Rename Move
    let auto_name = FileOperations::generate_unique_filename(tmp_media_b.path(), &media_a2.filename);
    assert_eq!(auto_name, "vacation (1).png");

    let moved_renamed = lib
        .file_operations()
        .move_single_media(media_a2.id, folder_b.id, Some(&auto_name), false)
        .unwrap();

    assert_eq!(moved_renamed.folder_id, folder_b.id);
    assert_eq!(moved_renamed.filename, "vacation (1).png");
    assert!(tmp_media_b.path().join("vacation (1).png").exists());
    assert!(tmp_media_b.path().join("vacation.png").exists());
    assert_eq!(lib.media().get_by_folder(folder_b.id).unwrap().len(), 2);
}

#[test]
fn test_copy_single_media_lifecycle_and_overwrites() {
    let tmp_db = tempdir().unwrap();
    let tmp_media_src = tempdir().unwrap();
    let tmp_media_dst = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder_src = lib.folders().add_folder(tmp_media_src.path()).unwrap();
    let folder_dst = lib.folders().add_folder(tmp_media_dst.path()).unwrap();

    let src_file = tmp_media_src.path().join("artwork.png");
    image::RgbImage::new(1000, 800).save(&src_file).unwrap();

    lib.refresher().refresh_folder(folder_src.id).unwrap();
    let media_src = lib.media().get_by_folder(folder_src.id).unwrap()[0].clone();

    // Assign rating 5 and a Tag to source
    lib.media().set_rating(media_src.id, 5).unwrap();
    let tag = lib.tags().create_tag("DigitalArt", None).unwrap();
    lib.tags().assign_tag(media_src.id, tag.id).unwrap();

    // 1. Execute direct Copy to destination folder
    let copied_record = lib
        .file_operations()
        .copy_single_media(media_src.id, folder_dst.id, None, false)
        .expect("Copy failed");

    // Both files must physically exist on disk
    assert!(src_file.exists());
    assert!(tmp_media_dst.path().join("artwork.png").exists());

    // New record must have new ID, target folder_id, and cloned tags/rating
    assert_ne!(copied_record.id, media_src.id);
    assert_eq!(copied_record.folder_id, folder_dst.id);
    assert_eq!(copied_record.rating, 5);
    assert_eq!(copied_record.width, 1000);
    assert_eq!(copied_record.height, 800);

    let copied_tags = lib.tags().get_tags_for_media(copied_record.id).unwrap();
    assert_eq!(copied_tags.len(), 1);
    assert_eq!(copied_tags[0].name, "DigitalArt");

    // Both folders have 1 media item
    assert_eq!(lib.media().get_by_folder(folder_src.id).unwrap().len(), 1);
    assert_eq!(lib.media().get_by_folder(folder_dst.id).unwrap().len(), 1);

    // 2. Test Collision on second Copy to same destination folder
    let conflict = lib
        .file_operations()
        .check_transfer_conflict(media_src.id, folder_dst.id, FileTransferKind::Copy)
        .unwrap();
    assert!(conflict.is_some());

    // 3. Test Auto-Rename Copy
    let auto_name = FileOperations::generate_unique_filename(tmp_media_dst.path(), &media_src.filename);
    assert_eq!(auto_name, "artwork (1).png");

    let auto_copied = lib
        .file_operations()
        .copy_single_media(media_src.id, folder_dst.id, Some(&auto_name), false)
        .expect("Auto-rename copy failed");

    assert_eq!(auto_copied.filename, "artwork (1).png");
    assert!(tmp_media_dst.path().join("artwork (1).png").exists());
    assert_eq!(lib.media().get_by_folder(folder_dst.id).unwrap().len(), 2);

    // 4. Test Overwrite Copy
    let overwritten = lib
        .file_operations()
        .copy_single_media(media_src.id, folder_dst.id, Some("artwork.png"), true)
        .expect("Overwrite copy failed");

    assert_eq!(overwritten.filename, "artwork.png");
    assert_eq!(lib.media().get_by_folder(folder_dst.id).unwrap().len(), 2);
}

#[test]
fn test_self_transfer_guards_protect_against_data_loss() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let file_path = tmp_media.path().join("self_test.png");
    let original_img = image::RgbImage::new(128, 128);
    original_img.save(&file_path).unwrap();

    lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    // 1. Move onto itself -> safe no-op, file intact
    let self_moved = lib
        .file_operations()
        .move_single_media(media.id, folder.id, None, true)
        .expect("Self move must succeed as safe identity no-op");
    assert_eq!(self_moved.id, media.id);
    assert!(file_path.exists());
    assert_eq!(std::fs::metadata(&file_path).unwrap().len(), media.file_size);

    // 2. Copy onto itself without renaming -> rejected to prevent collision
    let self_copy_fail = lib
        .file_operations()
        .copy_single_media(media.id, folder.id, None, false);
    assert!(self_copy_fail.is_err());

    // 3. Copy onto itself with Overwrite (Self-Overwrite Guard) -> must NOT truncate the file!
    let self_copy_overwrite = lib
        .file_operations()
        .copy_single_media(media.id, folder.id, None, true)
        .expect("Self-copy overwrite must safely no-op without truncating file");
    assert_eq!(self_copy_overwrite.id, media.id);
    assert!(file_path.exists());
    // File size must be intact and non-zero
    assert_eq!(std::fs::metadata(&file_path).unwrap().len(), media.file_size);

    // 4. Copy onto itself with Auto-Rename -> creates clean duplicate "self_test (1).png"
    let auto_name = FileOperations::generate_unique_filename(tmp_media.path(), &media.filename);
    assert_eq!(auto_name, "self_test (1).png");

    let duplicate = lib
        .file_operations()
        .copy_single_media(media.id, folder.id, Some(&auto_name), false)
        .expect("Duplicate copy must succeed");

    assert_eq!(duplicate.filename, "self_test (1).png");
    assert!(tmp_media.path().join("self_test (1).png").exists());
    assert!(tmp_media.path().join("self_test.png").exists());

    let all_records = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(all_records.len(), 2);
}

#[test]
fn test_move_with_overwrite_replaces_target() {
    let tmp_db = tempdir().unwrap();
    let tmp_media_src = tempdir().unwrap();
    let tmp_media_dst = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder_src = lib.folders().add_folder(tmp_media_src.path()).unwrap();
    let folder_dst = lib.folders().add_folder(tmp_media_dst.path()).unwrap();

    let src_file = tmp_media_src.path().join("doc.png");
    let dst_file = tmp_media_dst.path().join("doc.png");

    image::RgbImage::new(100, 100).save(&src_file).unwrap();
    image::RgbImage::new(200, 200).save(&dst_file).unwrap();

    lib.refresher().refresh_folder(folder_src.id).unwrap();
    lib.refresher().refresh_folder(folder_dst.id).unwrap();

    let media_src = lib.media().get_by_folder(folder_src.id).unwrap()[0].clone();
    let media_dst_old = lib.media().get_by_folder(folder_dst.id).unwrap()[0].clone();

    // Assign rating 5 to source
    lib.media().set_rating(media_src.id, 5).unwrap();

    // Execute move with overwrite = true
    let overwritten = lib
        .file_operations()
        .move_single_media(media_src.id, folder_dst.id, None, true)
        .expect("Overwrite move failed");

    assert_eq!(overwritten.id, media_src.id);
    assert_eq!(overwritten.folder_id, folder_dst.id);
    assert_eq!(overwritten.rating, 5);

    // Old destination media record must be completely cleaned up from LMDB
    assert!(lib.media().get(media_dst_old.id).unwrap().is_none());

    // Destination folder must have exactly 1 media record
    let dst_items = lib.media().get_by_folder(folder_dst.id).unwrap();
    assert_eq!(dst_items.len(), 1);
    assert_eq!(dst_items[0].id, media_src.id);
}

#[test]
fn test_missing_files_formatting_produces_pure_paths() {
    let items = vec![
        MissingFileItem {
            filename: "photo1.jpg".to_string(),
            path: PathBuf::from("/media/photos/photo1.jpg"),
        },
        MissingFileItem {
            filename: "photo2.png".to_string(),
            path: PathBuf::from("/media/photos/photo2.png"),
        },
        MissingFileItem {
            filename: "video_clip.mp4".to_string(),
            path: PathBuf::from("/media/videos/holiday/video_clip.mp4"),
        },
    ];

    let formatted = MissingFileDialogController::format_missing_paths_list(&items);
    assert_eq!(
        formatted,
        "/media/photos/photo1.jpg\n/media/photos/photo2.png\n/media/videos/holiday/video_clip.mp4"
    );
    assert!(!formatted.contains('•'));
    assert!(!formatted.contains('('));
    assert!(!formatted.contains(')'));
}
