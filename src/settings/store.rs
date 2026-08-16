use super::schema::AppSettings;
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

        settings.appearance.default_thumbnail_size =
            settings.appearance.default_thumbnail_size.clamp(80, 300);

        // Performance sanitization
        settings.performance.l1_cache_slots = settings.performance.l1_cache_slots.clamp(100, 8000);
        settings.performance.worker_threads = settings.performance.worker_threads.clamp(1, 32);
        settings.performance.thumbnail_db_size =
            settings.performance.thumbnail_db_size.clamp(64, 1024);
        settings.performance.thumbnail_quality =
            settings.performance.thumbnail_quality.clamp(10, 100);
        settings.performance.batch_commit_size =
            settings.performance.batch_commit_size.clamp(4, 256);

        if !settings
            .performance
            .thumbnail_format
            .eq_ignore_ascii_case("jpeg")
            && !settings
                .performance
                .thumbnail_format
                .eq_ignore_ascii_case("webp")
        {
            settings.performance.thumbnail_format = "WebP".to_string();
        }

        let algo_lower = settings.performance.scaling_algorithm.to_lowercase();
        if !matches!(
            algo_lower.as_str(),
            "catmullrom" | "bilinear" | "triangle" | "lanczos" | "lanczos3" | "nearest"
        ) {
            settings.performance.scaling_algorithm = "CatmullRom".to_string();
        }

        settings.player.default_volume = settings.player.default_volume.clamp(0.0, 100.0);

        // Sanitize view column widths
        settings.view.col_filename_width = settings.view.col_filename_width.clamp(100.0, 1200.0);
        settings.view.col_dimensions_width = settings.view.col_dimensions_width.clamp(60.0, 600.0);
        settings.view.col_size_width = settings.view.col_size_width.clamp(50.0, 400.0);
        settings.view.col_rating_width = settings.view.col_rating_width.clamp(50.0, 300.0);
    }
}
