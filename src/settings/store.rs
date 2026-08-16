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

    /// Loads settings from disk. Falls back to sanitized defaults if missing or corrupted.
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

    /// Atomically writes configuration to disk via temp file renaming.
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

    /// Validates filesystem paths and bounds-checks numerical UI configurations on startup.
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
        settings.performance.l1_cache_slots = settings.performance.l1_cache_slots.clamp(100, 4000);
        settings.performance.worker_threads = settings.performance.worker_threads.clamp(1, 32);
        settings.player.default_volume = settings.player.default_volume.clamp(0.0, 100.0);

        // Sanitize view column widths
        settings.view.col_filename_width = settings.view.col_filename_width.clamp(100.0, 1200.0);
        settings.view.col_dimensions_width = settings.view.col_dimensions_width.clamp(60.0, 600.0);
        settings.view.col_size_width = settings.view.col_size_width.clamp(50.0, 400.0);
        settings.view.col_rating_width = settings.view.col_rating_width.clamp(50.0, 300.0);
    }
}
