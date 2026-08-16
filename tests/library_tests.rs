use rustview::library::Library;
use rustview::media::MediaHasher;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_library_full_lifecycle_and_zero_fs_guarantee() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");

    // 1. Add folder
    let folder = lib
        .folders()
        .add_folder(tmp_media.path())
        .expect("Failed to add folder");
    assert_eq!(folder.id, 1);

    // 2. Create sample images
    let img1_path = tmp_media.path().join("photo1.png");
    let img2_path = tmp_media.path().join("photo2.png");

    let img1 = image::RgbImage::new(640, 480);
    img1.save(&img1_path).unwrap();

    let img2 = image::RgbImage::new(1920, 1080);
    img2.save(&img2_path).unwrap();

    // 3. First Refresh with progress tracking
    let progress_calls = Arc::new(AtomicUsize::new(0));
    let p_clone = progress_calls.clone();

    let report1 = lib
        .refresher()
        .refresh_folder_with_progress(
            folder.id,
            None,
            Some(move |_cur, _tot| {
                p_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .expect("First refresh failed");

    assert_eq!(report1.new_media, 2);
    assert_eq!(report1.unchanged_media, 0);
    assert_eq!(report1.deleted_media, 0);
    assert!(progress_calls.load(Ordering::SeqCst) > 0);

    // 4. Verify Zero-FS Reading & 16-byte MD5 Hash verification
    let media_list = lib
        .media()
        .get_by_folder(folder.id)
        .expect("Failed to load folder media");
    assert_eq!(media_list.len(), 2);
    assert!(
        media_list
            .iter()
            .any(|m| m.filename == "photo1.png" && m.width == 640 && m.height == 480)
    );
    assert!(
        media_list
            .iter()
            .any(|m| m.filename == "photo2.png" && m.width == 1920 && m.height == 1080)
    );

    // Verify hash lookup via MD5
    let expected_md5_img1 = MediaHasher::hash_file(&img1_path).unwrap();
    let hash_lookup = lib.media().get_by_hash(&expected_md5_img1).unwrap();
    assert_eq!(hash_lookup.len(), 1);
    assert_eq!(hash_lookup[0].filename, "photo1.png");

    // 5. Unchanged Refresh (Fast Path test)
    let report2 = lib
        .refresher()
        .refresh_folder(folder.id)
        .expect("Second refresh failed");
    assert_eq!(report2.new_media, 0);
    assert_eq!(report2.unchanged_media, 2);
    assert_eq!(report2.deleted_media, 0);

    // 6. Test Hierarchical Tags
    let tag_nature = lib.tags().create_tag("Nature", None).unwrap();
    let tag_landscape = lib
        .tags()
        .create_tag("Landscapes", Some(tag_nature.id))
        .unwrap();

    let target_media = &media_list[0];
    lib.tags()
        .assign_tag(target_media.id, tag_landscape.id)
        .unwrap();

    let tags = lib.tags().get_tags_for_media(target_media.id).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "Landscapes");
    assert_eq!(tags[0].parent_id, Some(tag_nature.id));

    // 7. Test Rating
    lib.media().set_rating(target_media.id, 5).unwrap();
    let updated_media = lib.media().get(target_media.id).unwrap().unwrap();
    assert_eq!(updated_media.rating, 5);

    // 8. Test Zero-Data-Loss Soft Deletion & Restoration
    // Delete photo2.png from disk
    fs::remove_file(img2_path.clone()).unwrap();
    let report3 = lib
        .refresher()
        .refresh_folder(folder.id)
        .expect("Third refresh failed");
    assert_eq!(report3.deleted_media, 1);
    assert_eq!(report3.unchanged_media, 1);

    // Active media list must only contain photo1.png
    let active_media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(active_media.len(), 1);
    assert_eq!(active_media[0].filename, "photo1.png");

    // All media (including soft-deleted) contains both
    let all_media = lib.media().get_all_by_folder(folder.id).unwrap();
    assert_eq!(all_media.len(), 2);
    let deleted_item = all_media.iter().find(|m| m.filename == "photo2.png").unwrap();
    assert!(deleted_item.is_deleted);

    // 9. Recreate photo2.png on disk and refresh folder -> verify recovery with same ID, tags & ratings
    let img2_restored = image::RgbImage::new(1920, 1080);
    img2_restored.save(&img2_path).unwrap();

    let report4 = lib
        .refresher()
        .refresh_folder(folder.id)
        .expect("Fourth refresh failed");
    assert_eq!(report4.restored_media, 1);
    assert_eq!(report4.unchanged_media, 1);

    let final_media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(final_media.len(), 2);
    let restored_item = final_media.iter().find(|m| m.filename == "photo2.png").unwrap();
    assert!(!restored_item.is_deleted);
    assert_eq!(restored_item.id, deleted_item.id);
}
