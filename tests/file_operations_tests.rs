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

    let updated = lib
        .file_operations()
        .rename_media(original_media_id, "new_name.png")
        .expect("Rename operation failed");

    assert_eq!(updated.id, original_media_id);
    assert_eq!(updated.filename, "new_name.png");
    assert!(!original_path.exists());
    assert!(tmp_media.path().join("new_name.png").exists());

    let reloaded = lib.media().get(original_media_id).unwrap().unwrap();
    assert_eq!(reloaded.filename, "new_name.png");

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

    let other_path = tmp_media.path().join("existing.png");
    img.save(&other_path).unwrap();
    assert!(
        lib.file_operations()
            .rename_media(original_media_id, "existing.png")
            .is_err()
    );
}

#[test]
fn test_check_rename_conflict_detection() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("source.jpg");
    let p2 = tmp_media.path().join("occupied.jpg");

    image::RgbImage::new(100, 100).save(&p1).unwrap();
    image::RgbImage::new(200, 200).save(&p2).unwrap();

    lib.refresher().refresh_folder(folder.id).unwrap();
    let list = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(list.len(), 2);

    let m1 = list.iter().find(|m| m.filename == "source.jpg").unwrap();

    // 1. Same name -> Not a conflict
    let self_check = lib.file_operations().check_rename_conflict(m1.id, "source.jpg").unwrap();
    assert!(self_check.is_none());

    // 2. Free name -> Not a conflict
    let free_check = lib.file_operations().check_rename_conflict(m1.id, "available.jpg").unwrap();
    assert!(free_check.is_none());

    // 3. Occupied name -> Conflict detected
    let conflict_check = lib.file_operations().check_rename_conflict(m1.id, "occupied.jpg").unwrap();
    assert!(conflict_check.is_some());
    let report = conflict_check.unwrap();
    assert_eq!(report.source.filename, "source.jpg");
    assert_eq!(report.destination.filename, "occupied.jpg");
    assert_eq!(report.destination.width, 200);
}

#[test]
fn test_rename_media_with_overwrite_purges_old_db_record() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("keep.png");
    let p2 = tmp_media.path().join("replace.png");

    image::RgbImage::new(300, 300).save(&p1).unwrap();
    image::RgbImage::new(150, 150).save(&p2).unwrap();

    lib.refresher().refresh_folder(folder.id).unwrap();
    let list = lib.media().get_by_folder(folder.id).unwrap();
    let m1 = list.iter().find(|m| m.filename == "keep.png").unwrap().clone();
    let m2 = list.iter().find(|m| m.filename == "replace.png").unwrap().clone();

    lib.media().set_rating(m1.id, 5).unwrap();

    // Renaming m1 to replace.png with overwrite = true
    let updated = lib
        .file_operations()
        .rename_media_with_overwrite(m1.id, "replace.png", true)
        .expect("Overwrite rename failed");

    assert_eq!(updated.id, m1.id);
    assert_eq!(updated.filename, "replace.png");
    assert_eq!(updated.rating, 5);

    // Replaced record must be purged from LMDB
    assert!(lib.media().get(m2.id).unwrap().is_none());

    let folder_media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(folder_media.len(), 1);
    assert_eq!(folder_media[0].id, m1.id);
    assert_eq!(folder_media[0].filename, "replace.png");

    // Old filename "keep.png" is completely forgotten. Creating a new keep.png must be treated as new item!
    image::RgbImage::new(50, 50).save(&p1).unwrap();
    let refresh_report = lib.refresher().refresh_folder(folder.id).unwrap();
    assert_eq!(refresh_report.new_media, 1);

    let folder_media_new = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(folder_media_new.len(), 2);
    let new_keep = folder_media_new.iter().find(|m| m.filename == "keep.png").unwrap();
    assert_ne!(new_keep.id, m1.id);
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

    lib.media().set_rating(media_id, 4).unwrap();
    let tag = lib.tags().create_tag("TestTag", None).unwrap();
    lib.tags().assign_tag(media_id, tag.id).unwrap();

    let new_img = image::RgbImage::new(200, 300);
    new_img.save(&img_path).unwrap();

    let refreshed = lib
        .file_operations()
        .refresh_media(media_id)
        .expect("Refresh media failed");

    assert_eq!(refreshed.id, media_id);
    assert_eq!(refreshed.width, 200);
    assert_eq!(refreshed.height, 300);
    assert_ne!(refreshed.file_hash, old_hash);
    assert_eq!(refreshed.rating, 4);
    assert!(!refreshed.is_corrupted);

    let tags = lib.tags().get_tags_for_media(media_id).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "TestTag");

    let by_new_hash = lib.media().get_by_hash(&refreshed.file_hash).unwrap();
    assert_eq!(by_new_hash.len(), 1);
    assert_eq!(by_new_hash[0].id, media_id);

    let by_old_hash = lib.media().get_by_hash(&old_hash).unwrap();
    assert!(by_old_hash.is_empty());

    let re_refreshed = lib
        .media_refresher()
        .refresh_media(media_id)
        .expect("MediaRefresher interface failed");
    assert_eq!(re_refreshed.id, media_id);
    assert_eq!(re_refreshed.width, 200);
}

#[test]
fn test_purge_media_from_db() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("file1.png");
    let p2 = tmp_media.path().join("file2.png");
    image::RgbImage::new(50, 50).save(&p1).unwrap();
    image::RgbImage::new(50, 50).save(&p2).unwrap();

    lib.refresher().refresh_folder(folder.id).unwrap();
    let list = lib.media().get_all_by_folder(folder.id).unwrap();
    assert_eq!(list.len(), 2);

    let id_to_purge = list[0].id;
    let purged = lib.file_operations().purge_media_from_db(&[id_to_purge]).unwrap();
    assert_eq!(purged, 1);

    let after_list = lib.media().get_all_by_folder(folder.id).unwrap();
    assert_eq!(after_list.len(), 1);
    assert_eq!(after_list[0].id, list[1].id);
    assert!(lib.media().get(id_to_purge).unwrap().is_none());
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

    std::fs::remove_file(&img_path).unwrap();

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

    std::fs::write(folder.join("photo.jpg"), b"base").unwrap();
    let cand1 = FileOperations::generate_unique_filename(folder, "photo.jpg");
    assert_eq!(cand1, "photo (1).jpg");

    std::fs::write(folder.join("photo (1).jpg"), b"1").unwrap();
    let cand2 = FileOperations::generate_unique_filename(folder, "photo.jpg");
    assert_eq!(cand2, "photo (2).jpg");

    std::fs::write(folder.join("photo (2).jpg"), b"2").unwrap();
    let cand3 = FileOperations::generate_unique_filename(folder, "photo.jpg");
    assert_eq!(cand3, "photo (3).jpg");

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

    let conflict_none = lib
        .file_operations()
        .check_transfer_conflict(media_a.id, folder_b.id, FileTransferKind::Move)
        .unwrap();
    assert!(conflict_none.is_none());

    let moved_record = lib
        .file_operations()
        .move_single_media(media_a.id, folder_b.id, None, false)
        .expect("Move failed");

    assert_eq!(moved_record.folder_id, folder_b.id);
    assert_eq!(moved_record.filename, "vacation.png");
    assert!(!img_a_path.exists());
    assert!(tmp_media_b.path().join("vacation.png").exists());

    assert_eq!(lib.media().get_by_folder(folder_a.id).unwrap().len(), 0);
    assert_eq!(lib.media().get_by_folder(folder_b.id).unwrap().len(), 1);

    image::RgbImage::new(800, 600).save(&img_a_path).unwrap();
    lib.refresher().refresh_folder(folder_a.id).unwrap();
    let media_a2 = lib.media().get_by_folder(folder_a.id).unwrap()[0].clone();

    let conflict_report = lib
        .file_operations()
        .check_transfer_conflict(media_a2.id, folder_b.id, FileTransferKind::Move)
        .unwrap()
        .expect("Conflict must be detected");

    assert_eq!(conflict_report.source.filename, "vacation.png");
    assert_eq!(conflict_report.source.width, 800);
    assert_eq!(conflict_report.destination.filename, "vacation.png");
    assert_eq!(conflict_report.destination.width, 1920);

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

    lib.media().set_rating(media_src.id, 5).unwrap();
    let tag = lib.tags().create_tag("DigitalArt", None).unwrap();
    lib.tags().assign_tag(media_src.id, tag.id).unwrap();

    let copied_record = lib
        .file_operations()
        .copy_single_media(media_src.id, folder_dst.id, None, false)
        .expect("Copy failed");

    assert!(src_file.exists());
    assert!(tmp_media_dst.path().join("artwork.png").exists());

    assert_ne!(copied_record.id, media_src.id);
    assert_eq!(copied_record.folder_id, folder_dst.id);
    assert_eq!(copied_record.rating, 5);
    assert_eq!(copied_record.width, 1000);
    assert_eq!(copied_record.height, 800);

    let copied_tags = lib.tags().get_tags_for_media(copied_record.id).unwrap();
    assert_eq!(copied_tags.len(), 1);
    assert_eq!(copied_tags[0].name, "DigitalArt");

    assert_eq!(lib.media().get_by_folder(folder_src.id).unwrap().len(), 1);
    assert_eq!(lib.media().get_by_folder(folder_dst.id).unwrap().len(), 1);

    let conflict = lib
        .file_operations()
        .check_transfer_conflict(media_src.id, folder_dst.id, FileTransferKind::Copy)
        .unwrap();
    assert!(conflict.is_some());

    let auto_name = FileOperations::generate_unique_filename(tmp_media_dst.path(), &media_src.filename);
    assert_eq!(auto_name, "artwork (1).png");

    let auto_copied = lib
        .file_operations()
        .copy_single_media(media_src.id, folder_dst.id, Some(&auto_name), false)
        .expect("Auto-rename copy failed");

    assert_eq!(auto_copied.filename, "artwork (1).png");
    assert!(tmp_media_dst.path().join("artwork (1).png").exists());
    assert_eq!(lib.media().get_by_folder(folder_dst.id).unwrap().len(), 2);

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

    let self_moved = lib
        .file_operations()
        .move_single_media(media.id, folder.id, None, true)
        .expect("Self move must succeed as safe identity no-op");
    assert_eq!(self_moved.id, media.id);
    assert!(file_path.exists());
    assert_eq!(std::fs::metadata(&file_path).unwrap().len(), media.file_size);

    let self_copy_fail = lib
        .file_operations()
        .copy_single_media(media.id, folder.id, None, false);
    assert!(self_copy_fail.is_err());

    let self_copy_overwrite = lib
        .file_operations()
        .copy_single_media(media.id, folder.id, None, true)
        .expect("Self-copy overwrite must safely no-op without truncating file");
    assert_eq!(self_copy_overwrite.id, media.id);
    assert!(file_path.exists());
    assert_eq!(std::fs::metadata(&file_path).unwrap().len(), media.file_size);

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

    lib.media().set_rating(media_src.id, 5).unwrap();

    let overwritten = lib
        .file_operations()
        .move_single_media(media_src.id, folder_dst.id, None, true)
        .expect("Overwrite move failed");

    assert_eq!(overwritten.id, media_src.id);
    assert_eq!(overwritten.folder_id, folder_dst.id);
    assert_eq!(overwritten.rating, 5);

    assert!(lib.media().get(media_dst_old.id).unwrap().is_none());

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
