use rustview::library::Library;
use rustview::media::{MediaFormat, MediaHasher};
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
    assert_eq!(report1.corrupted_media, 0);
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
            .any(|m| m.filename == "photo1.png" && m.width == 640 && m.height == 480 && !m.is_corrupted)
    );
    assert!(
        media_list
            .iter()
            .any(|m| m.filename == "photo2.png" && m.width == 1920 && m.height == 1080 && !m.is_corrupted)
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
    fs::remove_file(img2_path.clone()).unwrap();
    let report3 = lib
        .refresher()
        .refresh_folder(folder.id)
        .expect("Third refresh failed");
    assert_eq!(report3.deleted_media, 1);
    assert_eq!(report3.unchanged_media, 1);

    let active_media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(active_media.len(), 1);
    assert_eq!(active_media[0].filename, "photo1.png");

    let all_media = lib.media().get_all_by_folder(folder.id).unwrap();
    assert_eq!(all_media.len(), 2);
    let deleted_item = all_media.iter().find(|m| m.filename == "photo2.png").unwrap();
    assert!(deleted_item.is_deleted);

    // 9. Recreate photo2.png on disk and refresh folder
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

#[test]
fn test_corrupted_and_zero_byte_file_indexing_flags() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    // 1. Create a 0-byte file with media extension
    let zero_byte_path = tmp_media.path().join("empty_corrupt.png");
    fs::write(&zero_byte_path, b"").unwrap();

    // 2. Create a corrupted file with invalid image payload
    let corrupt_payload_path = tmp_media.path().join("garbage_corrupt.jpg");
    fs::write(&corrupt_payload_path, b"this is completely invalid header data").unwrap();

    // 3. Create a healthy file
    let healthy_path = tmp_media.path().join("healthy.png");
    image::RgbImage::new(320, 240).save(&healthy_path).unwrap();

    let report = lib.refresher().refresh_folder(folder.id).unwrap();
    assert_eq!(report.new_media, 3);
    assert_eq!(report.corrupted_media, 2);

    let media_list = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media_list.len(), 3);

    let zero_item = media_list.iter().find(|m| m.filename == "empty_corrupt.png").unwrap();
    assert!(zero_item.is_corrupted);
    assert_eq!(zero_item.file_size, 0);
    assert_eq!(zero_item.width, 0);

    let corrupt_item = media_list.iter().find(|m| m.filename == "garbage_corrupt.jpg").unwrap();
    assert!(corrupt_item.is_corrupted);
    assert_eq!(corrupt_item.width, 0);

    let healthy_item = media_list.iter().find(|m| m.filename == "healthy.png").unwrap();
    assert!(!healthy_item.is_corrupted);
    assert_eq!(healthy_item.width, 320);
    assert_eq!(healthy_item.height, 240);
}

#[test]
fn test_additional_supported_file_formats_detection() {
    assert_eq!(MediaFormat::from_extension("jfif"), MediaFormat::Jpeg);
    assert_eq!(MediaFormat::from_extension("jpe"), MediaFormat::Jpeg);
    assert_eq!(MediaFormat::from_extension("psd"), MediaFormat::Psd);
    assert_eq!(MediaFormat::from_extension("mpg"), MediaFormat::Mpeg);
    assert_eq!(MediaFormat::from_extension("mpeg"), MediaFormat::Mpeg);
    assert_eq!(MediaFormat::from_extension("flv"), MediaFormat::Flv);

    assert!(MediaFormat::from_extension("jfif").is_supported());
    assert!(MediaFormat::from_extension("jpe").is_supported());
    assert!(MediaFormat::from_extension("psd").is_supported());
    assert!(MediaFormat::from_extension("mpg").is_supported());
    assert!(MediaFormat::from_extension("mpeg").is_supported());
    assert!(MediaFormat::from_extension("flv").is_supported());

    assert_eq!(
        MediaFormat::from_extension("psd").media_type(),
        Some(rustview::media::MediaType::Image)
    );
    assert_eq!(
        MediaFormat::from_extension("mpg").media_type(),
        Some(rustview::media::MediaType::Video)
    );
    assert_eq!(
        MediaFormat::from_extension("flv").media_type(),
        Some(rustview::media::MediaType::Video)
    );
}

#[test]
fn test_auto_detect_and_rename_extensionless_media_on_refresh() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let jpeg_no_ext = tmp_media.path().join("raw_image_sample");
    let mut jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00];
    jpeg_bytes.extend(vec![0x00; 128]);
    fs::write(&jpeg_no_ext, &jpeg_bytes).unwrap();

    let png_no_ext = tmp_media.path().join("transparent_layer");
    let mut png_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    png_bytes.extend(vec![0x00; 64]);
    fs::write(&png_no_ext, &png_bytes).unwrap();

    let psd_no_ext = tmp_media.path().join("photoshop_project");
    let mut psd_bytes = b"8BPS".to_vec();
    psd_bytes.extend(vec![0x00; 64]);
    fs::write(&psd_no_ext, &psd_bytes).unwrap();

    let flv_no_ext = tmp_media.path().join("flash_stream");
    let mut flv_bytes = b"FLV\x01".to_vec();
    flv_bytes.extend(vec![0x00; 64]);
    fs::write(&flv_no_ext, &flv_bytes).unwrap();

    let mpeg_no_ext = tmp_media.path().join("movie_clip");
    let mut mpeg_bytes = vec![0x00, 0x00, 0x01, 0xBA];
    mpeg_bytes.extend(vec![0x00; 64]);
    fs::write(&mpeg_no_ext, &mpeg_bytes).unwrap();

    let text_no_ext = tmp_media.path().join("build_script");
    fs::write(&text_no_ext, b"#!/bin/bash\necho 'hello world'").unwrap();

    let hidden_dot_file = tmp_media.path().join(".hidden_image");
    fs::write(&hidden_dot_file, &jpeg_bytes).unwrap();

    let report = lib
        .refresher()
        .refresh_folder_with_options(folder.id, true, None, None::<fn(usize, usize)>)
        .expect("Refresh failed");

    assert!(!jpeg_no_ext.exists());
    assert!(tmp_media.path().join("raw_image_sample.jpg").exists());

    assert!(!png_no_ext.exists());
    assert!(tmp_media.path().join("transparent_layer.png").exists());

    assert!(!psd_no_ext.exists());
    assert!(tmp_media.path().join("photoshop_project.psd").exists());

    assert!(!flv_no_ext.exists());
    assert!(tmp_media.path().join("flash_stream.flv").exists());

    assert!(!mpeg_no_ext.exists());
    assert!(tmp_media.path().join("movie_clip.mpg").exists());

    assert!(text_no_ext.exists());
    assert!(!tmp_media.path().join("build_script.jpg").exists());
    assert!(hidden_dot_file.exists());

    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 5);
    assert_eq!(report.new_media, 5);

    let filenames: Vec<_> = media.iter().map(|m| m.filename.as_str()).collect();
    assert!(filenames.contains(&"raw_image_sample.jpg"));
    assert!(filenames.contains(&"transparent_layer.png"));
    assert!(filenames.contains(&"photoshop_project.psd"));
    assert!(filenames.contains(&"flash_stream.flv"));
    assert!(filenames.contains(&"movie_clip.mpg"));
}

#[test]
fn test_auto_detect_disabled_leaves_extensionless_files_untouched() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let jpeg_no_ext = tmp_media.path().join("unmodified_image");
    let mut jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    jpeg_bytes.extend(vec![0x00; 64]);
    fs::write(&jpeg_no_ext, &jpeg_bytes).unwrap();

    let report = lib
        .refresher()
        .refresh_folder_with_options(folder.id, false, None, None::<fn(usize, usize)>)
        .expect("Refresh failed");

    assert!(jpeg_no_ext.exists());
    assert!(!tmp_media.path().join("unmodified_image.jpg").exists());
    assert_eq!(report.new_media, 0);
    assert_eq!(lib.media().get_by_folder(folder.id).unwrap().len(), 0);
}
