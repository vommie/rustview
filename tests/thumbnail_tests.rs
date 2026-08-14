use rustview::library::Library;
use rustview::thumbnails::{PreparedTarget, THUMBNAIL_BYTE_SIZE, ThumbnailManager};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_thumbnail_engine_full_pipeline() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_cache = tempdir().unwrap();

    let library = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = library.folders().add_folder(tmp_media.path()).unwrap();

    // 1. Create test images
    let img1_path = tmp_media.path().join("photo1.jpg");
    let img2_path = tmp_media.path().join("photo2.png");

    let test_img1 = image::RgbImage::new(1920, 1080);
    test_img1.save(&img1_path).unwrap();

    let test_img2 = image::RgbImage::new(1280, 720);
    test_img2.save(&img2_path).unwrap();

    // Refresh library
    let report = library.refresher().refresh_folder(folder.id).unwrap();
    assert_eq!(report.new_media, 2);

    let media_list = library.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media_list.len(), 2);

    // 2. Initialize Thumbnail Manager
    let manager =
        Arc::new(ThumbnailManager::new(library.clone(), tmp_cache.path(), 128, 4).unwrap());

    let targets = vec![
        PreparedTarget {
            media_id: media_list[0].id,
            file_path: img1_path.clone(),
        },
        PreparedTarget {
            media_id: media_list[1].id,
            file_path: img2_path.clone(),
        },
    ];

    // 3. Start 2-Phase Pipeline
    manager.start_pipeline(folder.id, targets);

    // Wait for Rayon workers to complete pipeline
    let mut finished = false;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let progress = manager.progress();
        if !progress.is_preloading && !progress.is_generating && progress.generate_current == 2 {
            finished = true;
            break;
        }
    }
    assert!(finished, "Thumbnail pipeline did not finish in time");

    // 4. Drain batch and verify raw RGB8 pixel buffer
    let batch = manager.drain_ready_batch(10);
    assert_eq!(batch.len(), 2);

    for item in batch {
        assert_eq!(item.rgb8_pixels.len(), THUMBNAIL_BYTE_SIZE);
    }

    // 5. Test L1 Cache Invalidation & Instant Preload from Packfile
    manager.cache().clear();
    let reloaded_handle = manager
        .get_or_load_sync(media_list[0].id, &img1_path)
        .expect("Reload from packfile failed");
    let arena = manager.cache().arena();
    let reloaded_bytes = arena.read_slot(reloaded_handle).unwrap();
    assert_eq!(reloaded_bytes.len(), THUMBNAIL_BYTE_SIZE);
}
