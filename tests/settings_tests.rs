use rustview::settings::{
    AppSettings, GridAspectRatio, SettingsManager, SettingsStore, DEFAULT_AUDIO_DEVICE,
    DEFAULT_AUTOPLAY_VIDEOS, DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER, DEFAULT_BATCH_COMMIT_SIZE,
    DEFAULT_COL_DIMENSIONS_WIDTH, DEFAULT_COL_FILENAME_WIDTH, DEFAULT_COL_RATING_WIDTH,
    DEFAULT_COL_SIZE_WIDTH, DEFAULT_CONFIRM_ON_FILE_DELETE, DEFAULT_CUSTOM_PREVIEW_SETTINGS,
    DEFAULT_DARK_THEME, DEFAULT_DELETION_MODE, DEFAULT_ENABLE_HOVER_PREVIEWS,
    DEFAULT_GENERATE_VIDEO_PREVIEWS, DEFAULT_GPU_API, DEFAULT_HWDEC_MODE, DEFAULT_IS_TABLE_VIEW,
    DEFAULT_L1_CACHE_SLOTS, DEFAULT_PERSIST_FOLDER_TREE_STATE, DEFAULT_PLAYER_KEEP_OPEN,
    DEFAULT_PLAYER_VOLUME, DEFAULT_PREVIEW_FORMAT, DEFAULT_PREVIEW_FRAME_COUNT,
    DEFAULT_PREVIEW_QUALITY, DEFAULT_REMEMBER_LAST_FOLDER, DEFAULT_SCALING_ALGORITHM,
    DEFAULT_SHOW_DELETED_FOLDERS, DEFAULT_SHOW_FILENAMES, DEFAULT_SHOW_FOLDER_METRICS,
    DEFAULT_SHOW_RATINGS_OVERLAY, DEFAULT_SHOW_TAGS_OVERLAY, DEFAULT_SORT_COLUMN,
    DEFAULT_SORT_ORDER, DEFAULT_TAG_MATCH_MODE, DEFAULT_TAG_SCOPE_MODE,
    DEFAULT_THUMBNAIL_ASPECT_RATIO, DEFAULT_THUMBNAIL_DB_SIZE, DEFAULT_THUMBNAIL_FORMAT,
    DEFAULT_THUMBNAIL_QUALITY, DEFAULT_THUMBNAIL_WIDTH, DEFAULT_VIDEO_ENGINE,
    DEFAULT_VIDEO_OFFSET_PERCENT, DEFAULT_VIDEO_OUTPUT, DEFAULT_WINDOW_HEIGHT,
    DEFAULT_WINDOW_MAXIMIZED, DEFAULT_WINDOW_WIDTH,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::tempdir;

#[test]
fn test_settings_persistence_sanitization_and_hooks() {
    let tmp_config = tempdir().unwrap();

    let manager = SettingsManager::new(tmp_config.path());
    let mut initial = manager.get();

    // 1. Verify ALL default values match the Single Point of Truth constants
    assert_eq!(
        initial.appearance.default_thumbnail_size,
        DEFAULT_THUMBNAIL_WIDTH
    );
    assert_eq!(
        initial.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
    assert_eq!(
        GridAspectRatio::from_str_name(&initial.appearance.thumbnail_aspect_ratio),
        GridAspectRatio::Ratio4x3
    );
    assert_eq!(
        initial.appearance.show_filenames,
        DEFAULT_SHOW_FILENAMES
    );
    assert_eq!(
        initial.appearance.show_ratings_overlay,
        DEFAULT_SHOW_RATINGS_OVERLAY
    );
    assert_eq!(
        initial.appearance.show_tags_overlay,
        DEFAULT_SHOW_TAGS_OVERLAY
    );
    assert_eq!(
        initial.appearance.enable_hover_previews,
        DEFAULT_ENABLE_HOVER_PREVIEWS
    );
    assert_eq!(initial.appearance.dark_theme, DEFAULT_DARK_THEME);

    // General Defaults
    assert_eq!(
        initial.general.remember_last_folder,
        DEFAULT_REMEMBER_LAST_FOLDER
    );
    assert_eq!(initial.general.root_folder, None);
    assert_eq!(
        initial.general.confirm_on_file_delete,
        DEFAULT_CONFIRM_ON_FILE_DELETE
    );
    assert_eq!(initial.general.deletion_mode, DEFAULT_DELETION_MODE);

    // Folder Tree Defaults
    assert_eq!(
        initial.folders.auto_reveal_remembered_folder,
        DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER
    );
    assert_eq!(
        initial.folders.persist_folder_tree_state,
        DEFAULT_PERSIST_FOLDER_TREE_STATE
    );
    assert_eq!(
        initial.folders.show_deleted_folders,
        DEFAULT_SHOW_DELETED_FOLDERS
    );
    assert_eq!(
        initial.folders.show_folder_metrics,
        DEFAULT_SHOW_FOLDER_METRICS
    );

    // Performance Defaults
    assert_eq!(initial.performance.l1_cache_slots, DEFAULT_L1_CACHE_SLOTS);
    assert_eq!(
        initial.performance.thumbnail_db_size,
        DEFAULT_THUMBNAIL_DB_SIZE
    );
    assert_eq!(
        initial.performance.thumbnail_format,
        DEFAULT_THUMBNAIL_FORMAT
    );
    assert_eq!(
        initial.performance.thumbnail_quality,
        DEFAULT_THUMBNAIL_QUALITY
    );
    assert_eq!(
        initial.performance.scaling_algorithm,
        DEFAULT_SCALING_ALGORITHM
    );
    assert_eq!(
        initial.performance.batch_commit_size,
        DEFAULT_BATCH_COMMIT_SIZE
    );
    assert_eq!(initial.performance.video_engine, DEFAULT_VIDEO_ENGINE);
    assert_eq!(
        initial.performance.video_offset_percent,
        DEFAULT_VIDEO_OFFSET_PERCENT
    );
    assert_eq!(
        initial.performance.generate_video_previews,
        DEFAULT_GENERATE_VIDEO_PREVIEWS
    );
    assert_eq!(
        initial.performance.preview_frame_count,
        DEFAULT_PREVIEW_FRAME_COUNT
    );
    assert_eq!(
        initial.performance.custom_preview_settings,
        DEFAULT_CUSTOM_PREVIEW_SETTINGS
    );
    assert_eq!(
        initial.performance.preview_format,
        DEFAULT_PREVIEW_FORMAT
    );
    assert_eq!(
        initial.performance.preview_quality,
        DEFAULT_PREVIEW_QUALITY
    );

    // View Defaults
    assert_eq!(initial.view.is_table_view, DEFAULT_IS_TABLE_VIEW);
    assert_eq!(initial.view.sort_column, DEFAULT_SORT_COLUMN);
    assert_eq!(initial.view.sort_order, DEFAULT_SORT_ORDER);
    assert_eq!(initial.view.col_filename_width, DEFAULT_COL_FILENAME_WIDTH);
    assert_eq!(
        initial.view.col_dimensions_width,
        DEFAULT_COL_DIMENSIONS_WIDTH
    );
    assert_eq!(initial.view.col_size_width, DEFAULT_COL_SIZE_WIDTH);
    assert_eq!(initial.view.col_rating_width, DEFAULT_COL_RATING_WIDTH);

    // Player Defaults
    assert_eq!(initial.player.hwdec_mode, DEFAULT_HWDEC_MODE);
    assert_eq!(initial.player.autoplay_videos, DEFAULT_AUTOPLAY_VIDEOS);
    assert_eq!(initial.player.default_volume, DEFAULT_PLAYER_VOLUME);
    assert_eq!(initial.player.keep_open, DEFAULT_PLAYER_KEEP_OPEN);

    // RenderConfig Defaults
    let render_cfg = rustview::player::RenderConfig::default();
    assert_eq!(render_cfg.video_output, DEFAULT_VIDEO_OUTPUT);
    assert_eq!(render_cfg.gpu_api, DEFAULT_GPU_API);
    assert_eq!(render_cfg.audio_device, DEFAULT_AUDIO_DEVICE);

    // Tag Defaults
    assert_eq!(initial.tags.default_match_mode, DEFAULT_TAG_MATCH_MODE);
    assert_eq!(initial.tags.default_scope_mode, DEFAULT_TAG_SCOPE_MODE);

    // QueryFilter Defaults
    let filter = rustview::query::QueryFilter::default();
    assert_eq!(filter.match_mode.as_str(), DEFAULT_TAG_MATCH_MODE);
    assert_eq!(filter.scope_mode.as_str(), DEFAULT_TAG_SCOPE_MODE);

    // Window Defaults
    assert_eq!(initial.window.width, DEFAULT_WINDOW_WIDTH);
    assert_eq!(initial.window.height, DEFAULT_WINDOW_HEIGHT);
    assert_eq!(initial.window.is_maximized, DEFAULT_WINDOW_MAXIMIZED);

    // 2. Test event hook subscription
    let hook_fired = Arc::new(AtomicBool::new(false));
    let hook_flag = hook_fired.clone();

    manager.subscribe(move |s| {
        if s.appearance.default_thumbnail_size == 240 {
            hook_flag.store(true, Ordering::SeqCst);
        }
    });

    // 3. Mutate and apply settings
    initial.appearance.default_thumbnail_size = 240;
    initial.appearance.thumbnail_aspect_ratio = "16:9".to_string();
    initial.appearance.show_filenames = false;
    manager.update(initial);

    assert!(hook_fired.load(Ordering::SeqCst));

    // 4. Test persistent reload from disk
    let reloaded_store = SettingsStore::new(tmp_config.path());
    let loaded = reloaded_store.load();
    assert_eq!(loaded.appearance.default_thumbnail_size, 240);
    assert_eq!(loaded.appearance.thumbnail_aspect_ratio, "16:9");
    assert!(!loaded.appearance.show_filenames);

    // 5. Test validation & sanitization (non-existent root is cleared to None)
    let mut invalid_settings = AppSettings::default();
    invalid_settings.general.root_folder = Some(tmp_config.path().join("does_not_exist"));
    invalid_settings.appearance.thumbnail_aspect_ratio = "invalid_ratio".to_string();
    reloaded_store.save(&invalid_settings).unwrap();

    let sanitized = reloaded_store.load();
    assert_eq!(sanitized.general.root_folder, None);
    assert_eq!(
        sanitized.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
}
