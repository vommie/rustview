use super::schema::{
    AppSettings, GridAspectRatio, ThumbnailBadgeOverflow, DEFAULT_COL_FLAGS_WIDTH, DEFAULT_COL_PATH_WIDTH,
    DEFAULT_FULLSCREEN_FALLBACK_TARGET, DEFAULT_GRID_SPACING, DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB,
    DEFAULT_LEFT_SIDEBAR_PANEL_TABS, DEFAULT_MEDIA_TYPE_FILTER, DEFAULT_PREVIEW_FORMAT,
    DEFAULT_SCALING_ALGORITHM, DEFAULT_THUMBNAIL_ASPECT_RATIO, DEFAULT_THUMBNAIL_FORMAT,
    DEFAULT_VIDEO_ENGINE, DEFAULT_VIEWER_BACKGROUND_MODE, DEFAULT_WINDOW_HEIGHT,
    DEFAULT_WINDOW_WIDTH, MAX_BATCH_COMMIT_SIZE, MAX_COL_WIDTH, MAX_GRID_ITEM_BORDER_WIDTH,
    MAX_GRID_ITEM_CORNER_RADIUS, MAX_GRID_ITEM_PADDING_H, MAX_GRID_ITEM_PADDING_V,
    MAX_GRID_PADDING, MAX_GRID_SPACING, MAX_L1_CACHE_SLOTS, MAX_MAX_HISTORY_TAGS,
    MAX_PREVIEW_FRAME_COUNT, MAX_QUALITY, MAX_SHORT_VIDEO_THRESHOLD_SECS, MAX_SIDEBAR_WIDTH,
    MAX_TAB_GRID_COLUMNS, MAX_TAG_HISTORY_HEIGHT, MAX_THUMBNAIL_DB_SIZE, MAX_THUMBNAIL_WIDTH,
    MAX_TOOLTIP_DELAY_MS, MAX_VIDEO_OFFSET_PERCENT, MAX_WORKER_THREADS, MIN_BATCH_COMMIT_SIZE,
    MIN_COL_WIDTH, MIN_GRID_ITEM_BORDER_WIDTH, MIN_GRID_ITEM_CORNER_RADIUS, MIN_GRID_ITEM_PADDING_H,
    MIN_GRID_ITEM_PADDING_V, MIN_GRID_PADDING, MIN_GRID_SPACING, MIN_L1_CACHE_SLOTS,
    MIN_MAX_HISTORY_TAGS, MIN_PREVIEW_FRAME_COUNT, MIN_QUALITY,
    MIN_SHORT_VIDEO_THRESHOLD_SECS, MIN_SIDEBAR_WIDTH, MIN_TAB_GRID_COLUMNS,
    MIN_TAG_HISTORY_HEIGHT, MIN_THUMBNAIL_DB_SIZE, MIN_THUMBNAIL_WIDTH, MIN_TOOLTIP_DELAY_MS,
    MIN_VIDEO_OFFSET_PERCENT, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, MIN_WORKER_THREADS,
};
use super::schema::{
    FullscreenSettings, DEFAULT_FULLSCREEN_BACKGROUND_COLOR, DEFAULT_FULLSCREEN_BACKGROUND_SOURCE,
    DEFAULT_FULLSCREEN_SOUND_FOLLOWS, MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
    MAX_FULLSCREEN_IDLE_CURSOR_MS, MIN_FULLSCREEN_IDLE_CURSOR_MS, MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
};
use crate::util::hex_color;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct SettingsStore {
    file_path: PathBuf,
}

impl SettingsStore {
    pub fn new(config_dir: &Path) -> Self {
        let file_path = config_dir.join("settings.json");
        Self { file_path }
    }

    /// Loads settings from disk with tolerant forward/backward compatibility.
    pub fn load(&self) -> AppSettings {
        if self.file_path.exists() {
            match fs::read_to_string(&self.file_path) {
                Ok(content) => match serde_json::from_str::<AppSettings>(&content) {
                    Ok(mut settings) => {
                        self.validate_and_sanitize(&mut settings);
                        return settings;
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = ?self.file_path,
                            error = %e,
                            "[SettingsStore] Failed to parse settings JSON. Creating backup and restoring defaults"
                        );
                        let backup_path = self.file_path.with_extension("corrupted.bak");
                        let _ = fs::copy(&self.file_path, backup_path);
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        path = ?self.file_path,
                        error = %e,
                        "[SettingsStore] Failed to read settings file"
                    );
                }
            }
        }

        let mut defaults = AppSettings::default();
        self.validate_and_sanitize(&mut defaults);
        let _ = self.save(&defaults);
        defaults
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), std::io::Error> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = self.file_path.with_extension("tmp");
        let json_data = serde_json::to_string_pretty(settings)?;
        fs::write(&temp_path, json_data)?;
        fs::rename(temp_path, &self.file_path)?;

        Ok(())
    }

    fn validate_and_sanitize(&self, settings: &mut AppSettings) {
        // Appearance sanitization
        settings.appearance.default_thumbnail_size = settings
            .appearance
            .default_thumbnail_size
            .clamp(MIN_THUMBNAIL_WIDTH, MAX_THUMBNAIL_WIDTH);

        let ratio = GridAspectRatio::from_str_name(&settings.appearance.thumbnail_aspect_ratio);
        settings.appearance.thumbnail_aspect_ratio =
            if settings.appearance.thumbnail_aspect_ratio.is_empty() {
                DEFAULT_THUMBNAIL_ASPECT_RATIO.to_string()
            } else {
                ratio.as_str().to_string()
            };

        // Unknown overflow names (e.g. from hand-edited files) fall back to the default behavior.
        settings.appearance.thumbnail_badge_overflow =
            ThumbnailBadgeOverflow::from_str_name(&settings.appearance.thumbnail_badge_overflow)
                .as_str()
                .to_string();

        settings.appearance.tooltip_delay_ms = settings
            .appearance
            .tooltip_delay_ms
            .clamp(MIN_TOOLTIP_DELAY_MS, MAX_TOOLTIP_DELAY_MS);

        // Grid Items Layout sanitization
        settings.appearance.grid_item_padding_horizontal = settings
            .appearance
            .grid_item_padding_horizontal
            .clamp(MIN_GRID_ITEM_PADDING_H, MAX_GRID_ITEM_PADDING_H);

        settings.appearance.grid_item_padding_vertical = settings
            .appearance
            .grid_item_padding_vertical
            .clamp(MIN_GRID_ITEM_PADDING_V, MAX_GRID_ITEM_PADDING_V);

        // Auto-heal: If grid_spacing was corrupted/forced to 48 by the previous MIN_GRID_SPACING typo,
        // restore it cleanly back to the intended default of 12px.
        if settings.appearance.grid_spacing == 48 {
            settings.appearance.grid_spacing = DEFAULT_GRID_SPACING;
        }

        settings.appearance.grid_spacing = settings
            .appearance
            .grid_spacing
            .clamp(MIN_GRID_SPACING, MAX_GRID_SPACING);

        settings.appearance.grid_padding = settings
            .appearance
            .grid_padding
            .clamp(MIN_GRID_PADDING, MAX_GRID_PADDING);

        settings.appearance.grid_item_border_width = settings
            .appearance
            .grid_item_border_width
            .clamp(MIN_GRID_ITEM_BORDER_WIDTH, MAX_GRID_ITEM_BORDER_WIDTH);

        settings.appearance.grid_item_corner_radius = settings
            .appearance
            .grid_item_corner_radius
            .clamp(MIN_GRID_ITEM_CORNER_RADIUS, MAX_GRID_ITEM_CORNER_RADIUS);

        // Tab Grid Settings sanitization
        settings.view.tab_grid_columns = settings
            .view
            .tab_grid_columns
            .clamp(MIN_TAB_GRID_COLUMNS, MAX_TAB_GRID_COLUMNS);

        // Performance sanitization
        settings.performance.l1_cache_slots = settings
            .performance
            .l1_cache_slots
            .clamp(MIN_L1_CACHE_SLOTS, MAX_L1_CACHE_SLOTS);
        settings.performance.worker_threads = settings
            .performance
            .worker_threads
            .clamp(MIN_WORKER_THREADS, MAX_WORKER_THREADS);
        settings.performance.thumbnail_db_size = settings
            .performance
            .thumbnail_db_size
            .clamp(MIN_THUMBNAIL_DB_SIZE, MAX_THUMBNAIL_DB_SIZE);
        settings.performance.thumbnail_quality = settings
            .performance
            .thumbnail_quality
            .clamp(MIN_QUALITY, MAX_QUALITY);
        settings.performance.batch_commit_size = settings
            .performance
            .batch_commit_size
            .clamp(MIN_BATCH_COMMIT_SIZE, MAX_BATCH_COMMIT_SIZE);

        if !settings
            .performance
            .thumbnail_format
            .eq_ignore_ascii_case("jpeg")
            && !settings
                .performance
                .thumbnail_format
                .eq_ignore_ascii_case("webp")
        {
            settings.performance.thumbnail_format = DEFAULT_THUMBNAIL_FORMAT.to_string();
        }

        let algo_lower = settings.performance.scaling_algorithm.to_lowercase();
        if !matches!(
            algo_lower.as_str(),
            "catmullrom" | "bilinear" | "triangle" | "lanczos" | "lanczos3" | "nearest"
        ) {
            settings.performance.scaling_algorithm = DEFAULT_SCALING_ALGORITHM.to_string();
        }

        if !settings.performance.video_engine.eq_ignore_ascii_case("mpv")
            && !settings.performance.video_engine.eq_ignore_ascii_case("ffmpeg")
        {
            settings.performance.video_engine = DEFAULT_VIDEO_ENGINE.to_string();
        }

        settings.performance.video_offset_percent = settings
            .performance
            .video_offset_percent
            .clamp(MIN_VIDEO_OFFSET_PERCENT, MAX_VIDEO_OFFSET_PERCENT);
        settings.performance.short_video_threshold_secs = settings
            .performance
            .short_video_threshold_secs
            .clamp(MIN_SHORT_VIDEO_THRESHOLD_SECS, MAX_SHORT_VIDEO_THRESHOLD_SECS);
        settings.performance.preview_frame_count = settings
            .performance
            .preview_frame_count
            .clamp(MIN_PREVIEW_FRAME_COUNT, MAX_PREVIEW_FRAME_COUNT);

        if !settings
            .performance
            .preview_format
            .eq_ignore_ascii_case("jpeg")
            && !settings
                .performance
                .preview_format
                .eq_ignore_ascii_case("webp")
        {
            settings.performance.preview_format = DEFAULT_PREVIEW_FORMAT.to_string();
        }
        settings.performance.preview_quality = settings
            .performance
            .preview_quality
            .clamp(MIN_QUALITY, MAX_QUALITY);

        // Media Type Filter sanitization
        let m_lower = settings.view.media_type_filter.to_lowercase();
        if !matches!(
            m_lower.as_str(),
            "videos" | "video" | "images" | "image" | "all" | "both" | "videos & images" | "videos and images"
        ) {
            settings.view.media_type_filter = DEFAULT_MEDIA_TYPE_FILTER.to_string();
        }

        // Viewer Sanitization
        let bg_mode_lower = settings.viewer.background_mode.to_lowercase();
        if !matches!(
            bg_mode_lower.as_str(),
            "dark" | "deep black" | "black" | "transparent checkerboard" | "checkerboard"
        ) {
            settings.viewer.background_mode = DEFAULT_VIEWER_BACKGROUND_MODE.to_string();
        }

        // Fullscreen Sanitization
        settings.fullscreen.specific_monitor = settings.fullscreen.specific_monitor.trim().to_string();
        let fb_lower = settings.fullscreen.fallback_target.to_lowercase();
        if fb_lower != "same" && fb_lower != "different" {
            settings.fullscreen.fallback_target = DEFAULT_FULLSCREEN_FALLBACK_TARGET.to_string();
        }
        self.sanitize_fullscreen_extras(&mut settings.fullscreen);

        // Tag Settings Sanitization (including Tag History limits and height)
        settings.tags.max_history_tags = settings
            .tags
            .max_history_tags
            .clamp(MIN_MAX_HISTORY_TAGS, MAX_MAX_HISTORY_TAGS);
        settings.tags.history_tag_ids.truncate(settings.tags.max_history_tags);

        settings.tags.history_height = settings
            .tags
            .history_height
            .clamp(MIN_TAG_HISTORY_HEIGHT, MAX_TAG_HISTORY_HEIGHT);

        // Sanitize all 13 view column widths
        settings.view.col_filename_width = settings.view.col_filename_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        if settings.view.col_path_width == 0.0 {
            settings.view.col_path_width = DEFAULT_COL_PATH_WIDTH;
        } else {
            settings.view.col_path_width = settings.view.col_path_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        }
        settings.view.col_modified_date_width = settings.view.col_modified_date_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_created_date_width = settings.view.col_created_date_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_size_width = settings.view.col_size_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_rating_width = settings.view.col_rating_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_tags_width = settings.view.col_tags_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_width_width = settings.view.col_width_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_height_width = settings.view.col_height_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_ar_width = settings.view.col_ar_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_duration_width = settings.view.col_duration_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_format_width = settings.view.col_format_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        if settings.view.col_flags_width == 0.0 {
            settings.view.col_flags_width = DEFAULT_COL_FLAGS_WIDTH;
        } else {
            settings.view.col_flags_width = settings.view.col_flags_width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        }

        // Sanitize sidebar widths
        settings.view.sidebar_left_width = settings
            .view
            .sidebar_left_width
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        settings.view.sidebar_right_width = settings
            .view
            .sidebar_right_width
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);

        // Sanitize Left Sidebar Panel Tabs configuration
        let valid_tabs = DEFAULT_LEFT_SIDEBAR_PANEL_TABS;
        settings.view.left_sidebar_panel_tabs.retain(|t| valid_tabs.contains(&t.as_str()));
        if settings.view.left_sidebar_panel_tabs.is_empty() {
            settings.view.left_sidebar_panel_tabs = valid_tabs.iter().map(|s| s.to_string()).collect();
        }
        for &tab in valid_tabs {
            if !settings.view.left_sidebar_panel_tabs.iter().any(|t| t == tab) {
                settings.view.left_sidebar_panel_tabs.push(tab.to_string());
            }
        }
        if !settings.view.left_sidebar_panel_tabs.contains(&settings.view.left_sidebar_active_panel_tab) {
            settings.view.left_sidebar_active_panel_tab = DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB.to_string();
        }

        // Sanitize window geometry: enforce sane limits while strictly preserving maximization
        if settings.window.width < MIN_WINDOW_WIDTH {
            settings.window.width = DEFAULT_WINDOW_WIDTH;
        }
        if settings.window.height < MIN_WINDOW_HEIGHT {
            settings.window.height = DEFAULT_WINDOW_HEIGHT;
        }
        if let Some(x) = settings.window.x
            && !(0..=10_000).contains(&x)
        {
            settings.window.x = None;
        }
        if let Some(y) = settings.window.y
            && !(0..=10_000).contains(&y)
        {
            settings.window.y = None;
        }
    }

    /// Action bar, sound and background of the fullscreen: a hand-edited file can never leave an unknown mode or color.
    fn sanitize_fullscreen_extras(&self, fullscreen: &mut FullscreenSettings) {
        let bar = &mut fullscreen.media_bar;
        bar.reveal_distance_px = bar
            .reveal_distance_px
            .clamp(MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX);

        let primary = &mut fullscreen.primary_bar;
        primary.reveal_distance_px = primary
            .reveal_distance_px
            .clamp(MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX);
        primary.idle_cursor_ms = primary
            .idle_cursor_ms
            .clamp(MIN_FULLSCREEN_IDLE_CURSOR_MS, MAX_FULLSCREEN_IDLE_CURSOR_MS);

        let follows = fullscreen.sound.follows.trim().to_lowercase();
        fullscreen.sound.follows = if matches!(follows.as_str(), "first" | "newest" | "nobody") {
            follows
        } else {
            DEFAULT_FULLSCREEN_SOUND_FOLLOWS.to_string()
        };

        let source = fullscreen.background.source.trim().to_lowercase();
        fullscreen.background.source = if matches!(source.as_str(), "theme" | "viewer" | "custom") {
            source
        } else {
            DEFAULT_FULLSCREEN_BACKGROUND_SOURCE.to_string()
        };
        fullscreen.background.color = hex_color::parse_rgb(&fullscreen.background.color)
            .map(hex_color::format_rgb)
            .unwrap_or_else(|| DEFAULT_FULLSCREEN_BACKGROUND_COLOR.to_string());
    }
}
