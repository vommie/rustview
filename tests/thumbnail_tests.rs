use rustview::library::Library;
use rustview::thumbnails::{
    PreparedTarget, ScalingAlgorithm, ThumbnailConfig, ThumbnailFormat, ThumbnailGenerator,
    ThumbnailManager, VideoThumbnailEngine, calculate_aspect_dimensions,
};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_aspect_ratio_calculations() {
    // 4:3 Landscape
    assert_eq!(calculate_aspect_dimensions(4000, 3000, 256), (256, 192));
    // 16:9 Landscape
    assert_eq!(calculate_aspect_dimensions(3840, 2160, 256), (256, 144));
    // 3:4 Portrait
    assert_eq!(calculate_aspect_dimensions(3000, 4000, 256), (192, 256));
    // 1:1 Square
    assert_eq!(calculate_aspect_dimensions(2000, 2000, 256), (256, 256));
}

#[test]
fn test_thumbnail_engine_webp_and_jpeg_mixed_pipeline() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_cache = tempdir().unwrap();

    let library = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = library.folders().add_folder(tmp_media.path()).unwrap();

    let img1_path = tmp_media.path().join("photo_landscape.jpg");
    let img2_path = tmp_media.path().join("photo_portrait.png");

    image::RgbImage::new(1920, 1080).save(&img1_path).unwrap();
    image::RgbImage::new(1080, 1920).save(&img2_path).unwrap();

    let report = library.refresher().refresh_folder(folder.id).unwrap();
    assert_eq!(report.new_media, 2);

    let media_list = library.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media_list.len(), 2);

    let manager =
        Arc::new(ThumbnailManager::new(library.clone(), tmp_cache.path(), 128, 4).unwrap());

    // 1. Generate with WebP format & custom preview config
    let webp_config = ThumbnailConfig {
        max_bound: 256,
        format: ThumbnailFormat::Webp,
        quality: 80,
        scaling_algorithm: ScalingAlgorithm::CatmullRom,
        batch_commit_size: 16,
        video_engine: VideoThumbnailEngine::Ffmpeg,
        video_offset_percent: 10,
        generate_video_previews: true,
        preview_frame_count: 5,
        custom_preview_settings: true,
        preview_format: ThumbnailFormat::Webp,
        preview_quality: 40,
    };

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

    manager.start_pipeline(folder.id, targets, webp_config.clone());

    let mut finished = false;
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let progress = manager.progress();
        if !progress.is_preloading && !progress.is_generating && progress.generate_current == 2 {
            finished = true;
            break;
        }
    }
    assert!(finished, "Thumbnail WebP pipeline did not finish in time");

    let batch = manager.drain_ready_batch(10);
    assert_eq!(batch.len(), 2);

    // Verify aspect ratios (1920x1080 -> 256x144, 1080x1920 -> 144x256)
    let landscape_item = batch
        .iter()
        .find(|i| i.media_id == media_list[0].id)
        .unwrap();
    assert_eq!((landscape_item.width, landscape_item.height), (256, 144));

    let portrait_item = batch
        .iter()
        .find(|i| i.media_id == media_list[1].id)
        .unwrap();
    assert_eq!((portrait_item.width, portrait_item.height), (144, 256));

    // 2. Test L1 Cache Invalidation & Seamless Reload from PackStore
    manager.cache().clear();
    let reloaded = manager
        .get_or_load_sync(media_list[0].id, &img1_path, webp_config.clone())
        .expect("Reload from WebP packfile failed");
    assert_eq!((reloaded.width, reloaded.height), (256, 144));
    assert_eq!(reloaded.pixels.len(), (256 * 144 * 3) as usize);

    // 3. Test Mixed format decoding in PackStore
    let generator = ThumbnailGenerator::new();
    let raw_rgb = vec![200u8; 256 * 144 * 3];

    let webp_bytes = generator
        .compress(&raw_rgb, 256, 144, ThumbnailFormat::Webp, 40)
        .unwrap();
    let (w1, h1, decoded_webp) = generator.decompress(&webp_bytes).unwrap();
    assert_eq!((w1, h1), (256, 144));
    assert_eq!(decoded_webp.len(), 256 * 144 * 3);

    let jpeg_bytes = generator
        .compress(&raw_rgb, 256, 144, ThumbnailFormat::Jpeg, 80)
        .unwrap();
    let (w2, h2, decoded_jpeg) = generator.decompress(&jpeg_bytes).unwrap();
    assert_eq!((w2, h2), (256, 144));
    assert_eq!(decoded_jpeg.len(), 256 * 144 * 3);
}
