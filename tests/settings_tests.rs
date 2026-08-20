use rustview::settings::{
    AppSettings, GridAspectRatio, LogObscurer, SettingsDiff, SettingsManager, SettingsStore,
    DEFAULT_AUTO_DETECT_EXTENSIONLESS_MEDIA, DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER,
    DEFAULT_BATCH_COMMIT_SIZE, DEFAULT_COL_AR_WIDTH, DEFAULT_COL_CREATED_DATE_WIDTH,
    DEFAULT_COL_DURATION_WIDTH, DEFAULT_COL_FILENAME_WIDTH, DEFAULT_COL_FORMAT_WIDTH,
    DEFAULT_COL_HEIGHT_WIDTH, DEFAULT_COL_MODIFIED_DATE_WIDTH, DEFAULT_COL_RATING_WIDTH,
    DEFAULT_COL_SIZE_WIDTH, DEFAULT_COL_TAGS_WIDTH, DEFAULT_COL_WIDTH_WIDTH,
    DEFAULT_CONFIRM_ON_FILE_DELETE, DEFAULT_CUSTOM_PREVIEW_SETTINGS, DEFAULT_DARK_THEME,
    DEFAULT_DELETION_MODE, DEFAULT_ENABLE_HOVER_PREVIEWS, DEFAULT_ENABLE_TOOLTIPS,
    DEFAULT_GENERATE_VIDEO_PREVIEWS, DEFAULT_GRID_ITEM_BORDER_WIDTH,
    DEFAULT_GRID_ITEM_CORNER_RADIUS, DEFAULT_GRID_ITEM_PADDING_H, DEFAULT_GRID_ITEM_PADDING_V,
    DEFAULT_GRID_PADDING, DEFAULT_GRID_SPACING, DEFAULT_IS_TABLE_VIEW, DEFAULT_L1_CACHE_SLOTS,
    DEFAULT_OBFUSCATE_LOGS, DEFAULT_PERSIST_FOLDER_TREE_STATE, DEFAULT_PREVIEW_FORMAT,
    DEFAULT_PREVIEW_FRAME_COUNT, DEFAULT_PREVIEW_QUALITY, DEFAULT_REMEMBER_LAST_FOLDER,
    DEFAULT_SCALING_ALGORITHM, DEFAULT_SHORT_VIDEO_THRESHOLD_SECS, DEFAULT_SHOW_DELETED_FOLDERS,
    DEFAULT_SHOW_FILENAMES, DEFAULT_SHOW_FOLDER_METRICS, DEFAULT_SHOW_MEDIA_TYPE_BADGES,
    DEFAULT_SHOW_RATINGS_OVERLAY, DEFAULT_SHOW_TAGS_OVERLAY, DEFAULT_SIDEBAR_LEFT_WIDTH,
    DEFAULT_SIDEBAR_RIGHT_WIDTH, DEFAULT_SORT_COLUMN, DEFAULT_SORT_ORDER,
    DEFAULT_SUPPORTED_IMAGE_FORMATS, DEFAULT_SUPPORTED_VIDEO_FORMATS, DEFAULT_TAG_MATCH_MODE,
    DEFAULT_TAG_SCOPE_MODE, DEFAULT_THUMBNAIL_ASPECT_RATIO, DEFAULT_THUMBNAIL_DB_SIZE,
    DEFAULT_THUMBNAIL_FORMAT, DEFAULT_THUMBNAIL_QUALITY, DEFAULT_THUMBNAIL_WIDTH,
    DEFAULT_TOOLTIP_DELAY_MS, DEFAULT_VIDEO_ENGINE, DEFAULT_VIDEO_OFFSET_PERCENT,
    DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_MAXIMIZED, DEFAULT_WINDOW_WIDTH, MAX_COL_WIDTH,
    MAX_GRID_ITEM_BORDER_WIDTH, MAX_GRID_ITEM_CORNER_RADIUS, MAX_GRID_ITEM_PADDING_H,
    MAX_SHORT_VIDEO_THRESHOLD_SECS, MAX_TOOLTIP_DELAY_MS, MIN_COL_WIDTH,
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
        initial.appearance.show_media_type_badges,
        DEFAULT_SHOW_MEDIA_TYPE_BADGES
    );
    assert_eq!(
        initial.appearance.grid_item_padding_horizontal,
        DEFAULT_GRID_ITEM_PADDING_H
    );
    assert_eq!(
        initial.appearance.grid_item_padding_vertical,
        DEFAULT_GRID_ITEM_PADDING_V
    );
    assert_eq!(
        initial.appearance.grid_spacing,
        DEFAULT_GRID_SPACING
    );
    assert_eq!(
        initial.appearance.grid_padding,
        DEFAULT_GRID_PADDING
    );
    assert_eq!(
        initial.appearance.grid_item_border_width,
        DEFAULT_GRID_ITEM_BORDER_WIDTH
    );
    assert_eq!(
        initial.appearance.grid_item_corner_radius,
        DEFAULT_GRID_ITEM_CORNER_RADIUS
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
        initial.performance.short_video_threshold_secs,
        DEFAULT_SHORT_VIDEO_THRESHOLD_SECS
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

    // Formats Defaults
    assert_eq!(
        initial.formats.auto_detect_extensionless_media,
        DEFAULT_AUTO_DETECT_EXTENSIONLESS_MEDIA
    );
    assert!(initial.formats.auto_detect_extensionless_media);
    assert!(DEFAULT_SUPPORTED_IMAGE_FORMATS.contains("psd"));
    assert!(DEFAULT_SUPPORTED_IMAGE_FORMATS.contains("jfif"));
    assert!(DEFAULT_SUPPORTED_IMAGE_FORMATS.contains("jpe"));
    assert!(DEFAULT_SUPPORTED_VIDEO_FORMATS.contains("mpg"));
    assert!(DEFAULT_SUPPORTED_VIDEO_FORMATS.contains("mpeg"));
    assert!(DEFAULT_SUPPORTED_VIDEO_FORMATS.contains("flv"));

    // View Defaults - All 11 Columns
    assert_eq!(initial.view.is_table_view, DEFAULT_IS_TABLE_VIEW);
    assert_eq!(initial.view.sort_column, DEFAULT_SORT_COLUMN);
    assert_eq!(initial.view.sort_order, DEFAULT_SORT_ORDER);
    assert_eq!(initial.view.col_filename_width, DEFAULT_COL_FILENAME_WIDTH);
    assert_eq!(initial.view.col_modified_date_width, DEFAULT_COL_MODIFIED_DATE_WIDTH);
    assert_eq!(initial.view.col_created_date_width, DEFAULT_COL_CREATED_DATE_WIDTH);
    assert_eq!(initial.view.col_size_width, DEFAULT_COL_SIZE_WIDTH);
    assert_eq!(initial.view.col_rating_width, DEFAULT_COL_RATING_WIDTH);
    assert_eq!(initial.view.col_tags_width, DEFAULT_COL_TAGS_WIDTH);
    assert_eq!(initial.view.col_width_width, DEFAULT_COL_WIDTH_WIDTH);
    assert_eq!(initial.view.col_height_width, DEFAULT_COL_HEIGHT_WIDTH);
    assert_eq!(initial.view.col_ar_width, DEFAULT_COL_AR_WIDTH);
    assert_eq!(initial.view.col_duration_width, DEFAULT_COL_DURATION_WIDTH);
    assert_eq!(initial.view.col_format_width, DEFAULT_COL_FORMAT_WIDTH);
    assert_eq!(initial.view.sidebar_left_width, DEFAULT_SIDEBAR_LEFT_WIDTH);
    assert_eq!(initial.view.sidebar_right_width, DEFAULT_SIDEBAR_RIGHT_WIDTH);

    // Tag Defaults
    assert_eq!(initial.tags.default_match_mode, DEFAULT_TAG_MATCH_MODE);
    assert_eq!(initial.tags.default_scope_mode, DEFAULT_TAG_SCOPE_MODE);

    // Debug & Logging Defaults
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
    initial.appearance.show_media_type_badges = false;
    initial.appearance.grid_item_padding_horizontal = 16;
    initial.appearance.grid_item_padding_vertical = 10;
    initial.appearance.grid_spacing = 20;
    initial.appearance.grid_padding = 16;
    initial.appearance.grid_item_border_width = 3;
    initial.appearance.grid_item_corner_radius = 8;
    initial.appearance.enable_tooltips = false;
    initial.appearance.tooltip_delay_ms = 450;
    initial.performance.short_video_threshold_secs = 20;
    initial.formats.auto_detect_extensionless_media = false;
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
    assert!(!loaded.appearance.show_media_type_badges);
    assert_eq!(loaded.appearance.grid_item_padding_horizontal, 16);
    assert_eq!(loaded.appearance.grid_item_padding_vertical, 10);
    assert_eq!(loaded.appearance.grid_spacing, 20);
    assert_eq!(loaded.appearance.grid_padding, 16);
    assert_eq!(loaded.appearance.grid_item_border_width, 3);
    assert_eq!(loaded.appearance.grid_item_corner_radius, 8);
    assert!(!loaded.appearance.enable_tooltips);
    assert_eq!(loaded.appearance.tooltip_delay_ms, 450);
    assert_eq!(loaded.performance.short_video_threshold_secs, 20);
    assert!(!loaded.formats.auto_detect_extensionless_media);
    assert!(!loaded.debug.obfuscate_logs);

    // 5. Test validation & sanitization
    let mut invalid_settings = AppSettings::default();
    invalid_settings.appearance.thumbnail_aspect_ratio = "invalid_ratio".to_string();
    invalid_settings.appearance.tooltip_delay_ms = 99999;
    invalid_settings.appearance.grid_item_padding_horizontal = 100; // Above MAX (32)
    invalid_settings.appearance.grid_item_border_width = 50; // Above MAX (6)
    invalid_settings.appearance.grid_item_corner_radius = 100; // Above MAX (24)
    invalid_settings.performance.short_video_threshold_secs = 500;
    invalid_settings.view.col_filename_width = 10.0; // Below MIN_COL_WIDTH (40.0)
    invalid_settings.view.col_size_width = 5000.0; // Above MAX_COL_WIDTH (1200.0)
    reloaded_store.save(&invalid_settings).unwrap();

    let sanitized = reloaded_store.load();
    assert_eq!(
        sanitized.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
    assert_eq!(
        sanitized.appearance.tooltip_delay_ms,
        MAX_TOOLTIP_DELAY_MS
    );
    assert_eq!(
        sanitized.appearance.grid_item_padding_horizontal,
        MAX_GRID_ITEM_PADDING_H
    );
    assert_eq!(
        sanitized.appearance.grid_item_border_width,
        MAX_GRID_ITEM_BORDER_WIDTH
    );
    assert_eq!(
        sanitized.appearance.grid_item_corner_radius,
        MAX_GRID_ITEM_CORNER_RADIUS
    );
    assert_eq!(
        sanitized.performance.short_video_threshold_secs,
        MAX_SHORT_VIDEO_THRESHOLD_SECS
    );
    assert_eq!(sanitized.view.col_filename_width, MIN_COL_WIDTH);
    assert_eq!(sanitized.view.col_size_width, MAX_COL_WIDTH);
}

#[test]
fn test_settings_diff_granularity_and_noop_bypass() {
    let base = AppSettings::default();

    let diff_identical = SettingsDiff::compute(&base, &base);
    assert!(!diff_identical.has_changes);
    assert!(!diff_identical.root_folder_changed);
    assert!(!diff_identical.appearance_grid_changed);
    assert!(!diff_identical.tooltip_settings_changed);
    assert!(!diff_identical.formats_settings_changed);
    assert!(!diff_identical.debug_settings_changed);

    let mut modified_formats = base.clone();
    modified_formats.formats.auto_detect_extensionless_media = false;
    let diff_formats = SettingsDiff::compute(&base, &modified_formats);
    assert!(diff_formats.has_changes);
    assert!(diff_formats.formats_settings_changed);
    assert!(!diff_formats.root_folder_changed);
    assert!(!diff_formats.appearance_grid_changed);
    assert!(!diff_formats.debug_settings_changed);

    let mut modified_root = base.clone();
    modified_root.general.root_folder = Some(PathBuf::from("/media/photos"));
    let diff_root = SettingsDiff::compute(&base, &modified_root);
    assert!(diff_root.has_changes);
    assert!(diff_root.root_folder_changed);
    assert!(!diff_root.appearance_grid_changed);
    assert!(!diff_root.tooltip_settings_changed);
    assert!(!diff_root.debug_settings_changed);

    let summary = diff_formats.detailed_summary(&base, &modified_formats);
    assert_eq!(summary.len(), 1);
    assert!(summary[0].contains("auto_detect_extensionless_media"));
}

#[test]
fn test_tolerant_deserialization_with_missing_legacy_fields() {
    let tmp_config = tempdir().unwrap();
    let file_path = tmp_config.path().join("settings.json");

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

    assert!(!loaded.general.remember_last_folder);
    assert_eq!(loaded.appearance.default_thumbnail_size, 220);
    assert_eq!(loaded.performance.worker_threads, 8);

    assert_eq!(
        loaded.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
    assert_eq!(
        loaded.appearance.show_media_type_badges,
        DEFAULT_SHOW_MEDIA_TYPE_BADGES
    );
    assert_eq!(
        loaded.appearance.grid_item_padding_horizontal,
        DEFAULT_GRID_ITEM_PADDING_H
    );
    assert_eq!(
        loaded.appearance.grid_item_padding_vertical,
        DEFAULT_GRID_ITEM_PADDING_V
    );
    assert_eq!(
        loaded.appearance.grid_spacing,
        DEFAULT_GRID_SPACING
    );
    assert_eq!(
        loaded.appearance.grid_padding,
        DEFAULT_GRID_PADDING
    );
    assert_eq!(
        loaded.appearance.grid_item_border_width,
        DEFAULT_GRID_ITEM_BORDER_WIDTH
    );
    assert_eq!(
        loaded.appearance.grid_item_corner_radius,
        DEFAULT_GRID_ITEM_CORNER_RADIUS
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
        loaded.formats.auto_detect_extensionless_media,
        DEFAULT_AUTO_DETECT_EXTENSIONLESS_MEDIA
    );
    assert!(loaded.formats.auto_detect_extensionless_media);
    assert_eq!(
        loaded.performance.thumbnail_format,
        DEFAULT_THUMBNAIL_FORMAT
    );
    assert_eq!(
        loaded.performance.thumbnail_quality,
        DEFAULT_THUMBNAIL_QUALITY
    );
    assert_eq!(
        loaded.performance.short_video_threshold_secs,
        DEFAULT_SHORT_VIDEO_THRESHOLD_SECS
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
    assert_eq!(loaded.view.col_modified_date_width, DEFAULT_COL_MODIFIED_DATE_WIDTH);
    assert_eq!(loaded.view.col_created_date_width, DEFAULT_COL_CREATED_DATE_WIDTH);
    assert_eq!(loaded.view.col_size_width, DEFAULT_COL_SIZE_WIDTH);
    assert_eq!(loaded.view.col_rating_width, DEFAULT_COL_RATING_WIDTH);
    assert_eq!(loaded.view.col_tags_width, DEFAULT_COL_TAGS_WIDTH);
    assert_eq!(loaded.view.col_width_width, DEFAULT_COL_WIDTH_WIDTH);
    assert_eq!(loaded.view.col_height_width, DEFAULT_COL_HEIGHT_WIDTH);
    assert_eq!(loaded.view.col_ar_width, DEFAULT_COL_AR_WIDTH);
    assert_eq!(loaded.view.col_duration_width, DEFAULT_COL_DURATION_WIDTH);
    assert_eq!(loaded.view.col_format_width, DEFAULT_COL_FORMAT_WIDTH);
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

    let obscured_target = LogObscurer::target(test_path, Some(101));
    assert!(!obscured_target.contains("alice"));
    assert!(obscured_target.contains("MediaId(101) [*.jpg"));

    LogObscurer::set_obfuscate(DEFAULT_OBFUSCATE_LOGS);
}
