use super::schema::{
    AppSettings, GridAspectRatio, DEFAULT_PREVIEW_FORMAT, DEFAULT_SCALING_ALGORITHM,
    DEFAULT_THUMBNAIL_ASPECT_RATIO, DEFAULT_THUMBNAIL_FORMAT, DEFAULT_VIDEO_ENGINE,
    MAX_BATCH_COMMIT_SIZE, MAX_COL_WIDTH, MAX_L1_CACHE_SLOTS, MAX_PLAYER_VOLUME,
    MAX_PREVIEW_FRAME_COUNT, MAX_QUALITY, MAX_THUMBNAIL_DB_SIZE, MAX_THUMBNAIL_WIDTH,
    MAX_VIDEO_OFFSET_PERCENT, MAX_WORKER_THREADS, MIN_BATCH_COMMIT_SIZE, MIN_COL_WIDTH,
    MIN_L1_CACHE_SLOTS, MIN_PLAYER_VOLUME, MIN_PREVIEW_FRAME_COUNT, MIN_QUALITY,
    MIN_THUMBNAIL_DB_SIZE, MIN_THUMBNAIL_WIDTH, MIN_VIDEO_OFFSET_PERCENT, MIN_WORKER_THREADS,
};
use std::fs;
use std::path::{Path, PathBuf};

pub struct SettingsStore {
    file_path: PathBuf,
}

impl SettingsStore {
    pub fn new(config_dir: &Path) -> Self {
        let file_path = config_dir.join("settings.json");
        Self { file_path }
    }

    pub fn load(&self) -> AppSettings {
        if self.file_path.exists()
            && let Ok(content) = fs::read_to_string(&self.file_path)
            && let Ok(mut settings) = serde_json::from_str::<AppSettings>(&content)
        {
            self.validate_and_sanitize(&mut settings);
            return settings;
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
        if let Some(ref path) = settings.general.root_folder
            && !path.is_dir()
        {
            tracing::warn!(
                "Configured root folder does not exist: {:?}. Resetting.",
                path
            );
            settings.general.root_folder = None;
        }

        if let Some(ref path) = settings.general.last_folder_path
            && !path.is_dir()
        {
            tracing::warn!("Last visited folder does not exist: {:?}. Resetting.", path);
            settings.general.last_folder_path = None;
        }

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

        settings.player.default_volume = settings
            .player
            .default_volume
            .clamp(MIN_PLAYER_VOLUME, MAX_PLAYER_VOLUME);

        // Sanitize view column widths
        settings.view.col_filename_width = settings
            .view
            .col_filename_width
            .clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_dimensions_width = settings
            .view
            .col_dimensions_width
            .clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_size_width = settings
            .view
            .col_size_width
            .clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
        settings.view.col_rating_width = settings
            .view
            .col_rating_width
            .clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
    }
}
