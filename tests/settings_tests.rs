use rustview::settings::{
    AppSettings, GridAspectRatio, LogObscurer, SettingsDiff, SettingsManager, SettingsStore,
    DEFAULT_AUDIO_DEVICE, DEFAULT_AUTOPLAY_VIDEOS, DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER,
    DEFAULT_BATCH_COMMIT_SIZE, DEFAULT_COL_DIMENSIONS_WIDTH, DEFAULT_COL_FILENAME_WIDTH,
    DEFAULT_COL_RATING_WIDTH, DEFAULT_COL_SIZE_WIDTH, DEFAULT_CONFIRM_ON_FILE_DELETE,
    DEFAULT_CUSTOM_PREVIEW_SETTINGS, DEFAULT_DARK_THEME, DEFAULT_DELETION_MODE,
    DEFAULT_ENABLE_HOVER_PREVIEWS, DEFAULT_ENABLE_TOOLTIPS, DEFAULT_GENERATE_VIDEO_PREVIEWS,
    DEFAULT_GPU_API, DEFAULT_HWDEC_MODE, DEFAULT_IS_TABLE_VIEW, DEFAULT_L1_CACHE_SLOTS,
    DEFAULT_OBFUSCATE_LOGS, DEFAULT_PERSIST_FOLDER_TREE_STATE, DEFAULT_PLAYER_KEEP_OPEN,
    DEFAULT_PLAYER_VOLUME, DEFAULT_PREVIEW_FORMAT, DEFAULT_PREVIEW_FRAME_COUNT,
    DEFAULT_PREVIEW_QUALITY, DEFAULT_REMEMBER_LAST_FOLDER, DEFAULT_SCALING_ALGORITHM,
    DEFAULT_SHOW_DELETED_FOLDERS, DEFAULT_SHOW_FILENAMES, DEFAULT_SHOW_FOLDER_METRICS,
    DEFAULT_SHOW_RATINGS_OVERLAY, DEFAULT_SHOW_TAGS_OVERLAY, DEFAULT_SIDEBAR_LEFT_WIDTH,
    DEFAULT_SIDEBAR_RIGHT_WIDTH, DEFAULT_SORT_COLUMN, DEFAULT_SORT_ORDER, DEFAULT_TAG_MATCH_MODE,
    DEFAULT_TAG_SCOPE_MODE, DEFAULT_THUMBNAIL_ASPECT_RATIO, DEFAULT_THUMBNAIL_DB_SIZE,
    DEFAULT_THUMBNAIL_FORMAT, DEFAULT_THUMBNAIL_QUALITY, DEFAULT_THUMBNAIL_WIDTH,
    DEFAULT_TOOLTIP_DELAY_MS, DEFAULT_VIDEO_ENGINE, DEFAULT_VIDEO_OFFSET_PERCENT,
    DEFAULT_VIDEO_OUTPUT, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_MAXIMIZED, DEFAULT_WINDOW_WIDTH,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    assert_eq!(initial.appearance.enable_tooltips, DEFAULT_ENABLE_TOOLTIPS);
    assert_eq!(
        initial.appearance.tooltip_delay_ms,
        DEFAULT_TOOLTIP_DELAY_MS
    );
    assert_eq!(DEFAULT_TOOLTIP_DELAY_MS, 750);

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
    assert_eq!(initial.view.sidebar_left_width, DEFAULT_SIDEBAR_LEFT_WIDTH);
    assert_eq!(initial.view.sidebar_right_width, DEFAULT_SIDEBAR_RIGHT_WIDTH);

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

    // Debug & Logging Defaults (Default = true)
    assert_eq!(initial.debug.obfuscate_logs, DEFAULT_OBFUSCATE_LOGS);
    assert!(initial.debug.obfuscate_logs);

    // QueryFilter Defaults
    let filter = rustview::query::QueryFilter::default();
    assert_eq!(filter.match_mode.as_str(), DEFAULT_TAG_MATCH_MODE);
    assert_eq!(filter.scope_mode.as_str(), DEFAULT_TAG_SCOPE_MODE);

    // Window Defaults
    assert_eq!(initial.window.width, DEFAULT_WINDOW_WIDTH);
    assert_eq!(initial.window.height, DEFAULT_WINDOW_HEIGHT);
    assert_eq!(initial.window.is_maximized, DEFAULT_WINDOW_MAXIMIZED);

    // 2. Test event hook subscription with granular SettingsDiff
    let hook_fired = Arc::new(AtomicBool::new(false));
    let hook_flag = hook_fired.clone();

    manager.subscribe(move |_old, new, diff| {
        if diff.appearance_grid_changed && new.appearance.default_thumbnail_size == 240 {
            hook_flag.store(true, Ordering::SeqCst);
        }
    });

    // 3. Mutate and apply settings
    initial.appearance.default_thumbnail_size = 240;
    initial.appearance.thumbnail_aspect_ratio = "16:9".to_string();
    initial.appearance.show_filenames = false;
    initial.appearance.enable_tooltips = false;
    initial.appearance.tooltip_delay_ms = 450;
    initial.debug.obfuscate_logs = false;
    manager.update(initial);

    assert!(hook_fired.load(Ordering::SeqCst));
    assert!(!LogObscurer::is_active());

    // 4. Test persistent reload from disk
    let reloaded_store = SettingsStore::new(tmp_config.path());
    let loaded = reloaded_store.load();
    assert_eq!(loaded.appearance.default_thumbnail_size, 240);
    assert_eq!(loaded.appearance.thumbnail_aspect_ratio, "16:9");
    assert!(!loaded.appearance.show_filenames);
    assert!(!loaded.appearance.enable_tooltips);
    assert_eq!(loaded.appearance.tooltip_delay_ms, 450);
    assert!(!loaded.debug.obfuscate_logs);

    // 5. Test validation & sanitization
    let mut invalid_settings = AppSettings::default();
    invalid_settings.appearance.thumbnail_aspect_ratio = "invalid_ratio".to_string();
    invalid_settings.appearance.tooltip_delay_ms = 99999;
    reloaded_store.save(&invalid_settings).unwrap();

    let sanitized = reloaded_store.load();
    assert_eq!(
        sanitized.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
    assert_eq!(
        sanitized.appearance.tooltip_delay_ms,
        rustview::settings::MAX_TOOLTIP_DELAY_MS
    );
}

#[test]
fn test_settings_diff_granularity_and_noop_bypass() {
    let base = AppSettings::default();

    // 1. Identical settings -> has_changes is false
    let diff_identical = SettingsDiff::compute(&base, &base);
    assert!(!diff_identical.has_changes);
    assert!(!diff_identical.root_folder_changed);
    assert!(!diff_identical.appearance_grid_changed);
    assert!(!diff_identical.tooltip_settings_changed);
    assert!(!diff_identical.debug_settings_changed);

    // 2. Only tooltip delay changed -> only tooltip_settings_changed is true
    let mut modified_tooltips = base.clone();
    modified_tooltips.appearance.tooltip_delay_ms = 350;
    let diff_tooltips = SettingsDiff::compute(&base, &modified_tooltips);
    assert!(diff_tooltips.has_changes);
    assert!(diff_tooltips.tooltip_settings_changed);
    assert!(!diff_tooltips.root_folder_changed);
    assert!(!diff_tooltips.appearance_grid_changed);
    assert!(!diff_tooltips.folder_tree_presentation_changed);
    assert!(!diff_tooltips.debug_settings_changed);

    // 3. Only root folder path changed -> only root_folder_changed is true
    let mut modified_root = base.clone();
    modified_root.general.root_folder = Some(PathBuf::from("/media/photos"));
    let diff_root = SettingsDiff::compute(&base, &modified_root);
    assert!(diff_root.has_changes);
    assert!(diff_root.root_folder_changed);
    assert!(!diff_root.appearance_grid_changed);
    assert!(!diff_root.tooltip_settings_changed);
    assert!(!diff_root.debug_settings_changed);

    // 4. Only folder tree metrics changed -> only folder_tree_presentation_changed is true
    let mut modified_tree = base.clone();
    modified_tree.folders.show_folder_metrics = false;
    let diff_tree = SettingsDiff::compute(&base, &modified_tree);
    assert!(diff_tree.has_changes);
    assert!(diff_tree.folder_tree_presentation_changed);
    assert!(!diff_tree.root_folder_changed);
    assert!(!diff_tree.appearance_grid_changed);
    assert!(!diff_tree.debug_settings_changed);

    // 5. Only debug logging changed -> only debug_settings_changed is true
    let mut modified_debug = base.clone();
    modified_debug.debug.obfuscate_logs = false;
    let diff_debug = SettingsDiff::compute(&base, &modified_debug);
    assert!(diff_debug.has_changes);
    assert!(diff_debug.debug_settings_changed);
    assert!(!diff_debug.root_folder_changed);
    assert!(!diff_debug.appearance_grid_changed);
    assert!(!diff_debug.tooltip_settings_changed);

    // 6. Verify detailed summary output
    let summary = diff_root.detailed_summary(&base, &modified_root);
    assert_eq!(summary.len(), 1);
    assert!(summary[0].contains("root_folder"));

    let summary_debug = diff_debug.detailed_summary(&base, &modified_debug);
    assert_eq!(summary_debug.len(), 1);
    assert!(summary_debug[0].contains("obfuscate_logs"));
}

#[test]
fn test_tolerant_deserialization_with_missing_legacy_fields() {
    let tmp_config = tempdir().unwrap();
    let file_path = tmp_config.path().join("settings.json");

    // Simulate an incomplete legacy JSON file with missing new fields
    let legacy_json = r#"{
        "general": {
            "remember_last_folder": false
        },
        "appearance": {
            "default_thumbnail_size": 220
        },
        "performance": {
            "worker_threads": 8
        }
    }"#;

    fs::write(&file_path, legacy_json).unwrap();

    let store = SettingsStore::new(tmp_config.path());
    let loaded = store.load();

    // Preserved existing fields
    assert!(!loaded.general.remember_last_folder);
    assert_eq!(loaded.appearance.default_thumbnail_size, 220);
    assert_eq!(loaded.performance.worker_threads, 8);

    // Missing fields automatically populated from Single Point of Truth defaults
    assert_eq!(
        loaded.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
    assert_eq!(
        loaded.appearance.enable_hover_previews,
        DEFAULT_ENABLE_HOVER_PREVIEWS
    );
    assert_eq!(loaded.appearance.enable_tooltips, DEFAULT_ENABLE_TOOLTIPS);
    assert_eq!(
        loaded.appearance.tooltip_delay_ms,
        DEFAULT_TOOLTIP_DELAY_MS
    );
    assert_eq!(loaded.appearance.tooltip_delay_ms, 750);
    assert_eq!(
        loaded.performance.thumbnail_format,
        DEFAULT_THUMBNAIL_FORMAT
    );
    assert_eq!(
        loaded.performance.thumbnail_quality,
        DEFAULT_THUMBNAIL_QUALITY
    );
    assert_eq!(
        loaded.performance.custom_preview_settings,
        DEFAULT_CUSTOM_PREVIEW_SETTINGS
    );
    assert_eq!(
        loaded.performance.preview_format,
        DEFAULT_PREVIEW_FORMAT
    );
    assert_eq!(
        loaded.performance.preview_quality,
        DEFAULT_PREVIEW_QUALITY
    );
    assert_eq!(loaded.performance.video_engine, DEFAULT_VIDEO_ENGINE);
    assert_eq!(loaded.view.col_filename_width, DEFAULT_COL_FILENAME_WIDTH);
    assert_eq!(loaded.view.sidebar_left_width, DEFAULT_SIDEBAR_LEFT_WIDTH);
    assert_eq!(loaded.view.sidebar_right_width, DEFAULT_SIDEBAR_RIGHT_WIDTH);
    assert_eq!(loaded.debug.obfuscate_logs, DEFAULT_OBFUSCATE_LOGS);
}

#[test]
fn test_log_obscurer_behavior_enabled_and_disabled() {
    let test_path = Path::new("/home/alice/Pictures/Summer_Vacation_2026/beach_party.jpg");
    let test_filename = "beach_party.jpg";
    let test_tag_name = "Family Vacation";
    let test_query = "beach 2026";

    // 1. When Obfuscation is DISABLED (Cleartext with contextual info)
    LogObscurer::set_obfuscate(false);
    assert!(!LogObscurer::is_active());

    assert_eq!(LogObscurer::path(test_path), test_path.display().to_string());
    assert_eq!(
        LogObscurer::folder(Some(42), test_path),
        format!("Folder#42({})", test_path.display())
    );
    assert_eq!(
        LogObscurer::media(Some(101), test_filename),
        "Media#101(beach_party.jpg)"
    );
    assert_eq!(
        LogObscurer::tag(Some(7), test_tag_name),
        "Tag#7(Family Vacation)"
    );
    assert_eq!(LogObscurer::query(test_query), "\"beach 2026\"");

    // 2. When Obfuscation is ENABLED (Privacy-Protected with IDs / Hash surrogates)
    LogObscurer::set_obfuscate(true);
    assert!(LogObscurer::is_active());

    let obscured_path = LogObscurer::path(test_path);
    assert!(!obscured_path.contains("alice"));
    assert!(!obscured_path.contains("Summer_Vacation"));
    assert!(!obscured_path.contains("beach_party"));
    assert!(obscured_path.starts_with("[Path:"));
    assert!(obscured_path.ends_with(".jpg]"));

    let obscured_folder = LogObscurer::folder(Some(42), test_path);
    assert_eq!(obscured_folder, "Folder#42");
    assert!(!obscured_folder.contains("alice"));

    let obscured_media = LogObscurer::media(Some(101), test_filename);
    assert_eq!(obscured_media, "Media#101[*.jpg]");
    assert!(!obscured_media.contains("beach_party"));

    let obscured_tag = LogObscurer::tag(Some(7), test_tag_name);
    assert_eq!(obscured_tag, "Tag#7");
    assert!(!obscured_tag.contains("Vacation"));

    let obscured_query = LogObscurer::query(test_query);
    assert_eq!(obscured_query, "[Query:10 chars]");
    assert!(!obscured_query.contains("beach"));

    // Target formatting
    let obscured_target = LogObscurer::target(test_path, Some(101));
    assert!(!obscured_target.contains("alice"));
    assert!(obscured_target.contains("MediaId(101) [*.jpg"));

    // Cleanup: Reset to SPoT default state
    LogObscurer::set_obfuscate(DEFAULT_OBFUSCATE_LOGS);
}
