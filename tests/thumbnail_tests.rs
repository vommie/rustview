use rustview::library::Library;
use rustview::thumbnails::{THUMBNAIL_BYTE_SIZE, ThumbnailManager};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_thumbnail_engine_full_pipeline() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_cache = tempdir().unwrap();

    let library = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = library.folders().add_folder(tmp_media.path()).unwrap();

    // 1. Create a test image
    let img_path = tmp_media.path().join("highres.jpg");
    let test_img = image::RgbImage::new(1920, 1080);
    test_img.save(&img_path).unwrap();

    // Refresh library
    let report = library.refresher().refresh_folder(folder.id).unwrap();
    assert_eq!(report.new_media, 1);

    let media_list = library.media().get_by_folder(folder.id).unwrap();
    let media = &media_list[0];

    // 2. Initialize Thumbnail Manager (128 slots = ~24MB L1 cache, 4 Rayon workers)
    let manager = ThumbnailManager::new(library.clone(), tmp_cache.path(), 128, 4).unwrap();

    // 3. Load Synchronously
    let handle = manager
        .get_or_load_sync(media.id, &img_path)
        .expect("Failed to generate and load thumbnail");

    // 4. Verify Arena RAM content with bound Arc lifetime
    let arena = manager.cache().arena();
    let raw_bytes = arena.read_slot(handle).expect("Slot must be valid");
    assert_eq!(raw_bytes.len(), THUMBNAIL_BYTE_SIZE);

    // 5. Verify L2 SSD Pack file exists and contains packed data
    let pack_path = tmp_cache.path().join("pack-000000.bin");
    assert!(pack_path.exists());
    assert!(pack_path.metadata().unwrap().len() > 0);

    // 6. Test L1 Cache Invalidation & Mmap Retrieval
    manager.cache().clear();
    let handle2 = manager
        .get_or_load_sync(media.id, &img_path)
        .expect("Reload from SSD pack failed");
    let reloaded_bytes = arena.read_slot(handle2).unwrap();
    assert_eq!(reloaded_bytes.len(), THUMBNAIL_BYTE_SIZE);
}
