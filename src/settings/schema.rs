use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeletionMode {
    Trash,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashingMode {
    Blake3Full,
    FastHeader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub root_folder: Option<PathBuf>,
    pub remember_last_folder: bool,
    pub last_folder_path: Option<PathBuf>,
    pub confirm_on_file_delete: bool,
    pub deletion_mode: DeletionMode,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            root_folder: None,
            remember_last_folder: true,
            last_folder_path: None,
            confirm_on_file_delete: true,
            deletion_mode: DeletionMode::Trash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub default_thumbnail_size: u32,
    pub show_filenames: bool,
    pub show_ratings_overlay: bool,
    pub show_tags_overlay: bool,
    pub dark_theme: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            default_thumbnail_size: 150,
            show_filenames: true,
            show_ratings_overlay: true,
            show_tags_overlay: true,
            dark_theme: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceSettings {
    pub l1_cache_slots: usize,
    pub worker_threads: usize,
    pub hashing_mode: HashingMode,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            l1_cache_slots: 512,
            worker_threads: 4,
            hashing_mode: HashingMode::Blake3Full,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSettings {
    pub hwdec_mode: String,
    pub autoplay_videos: bool,
    pub default_volume: f32,
    pub keep_open: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            hwdec_mode: "auto-safe".to_string(),
            autoplay_videos: true,
            default_volume: 100.0,
            keep_open: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagSettings {
    pub default_match_mode: String,
    pub default_scope_mode: String,
    #[serde(default)]
    pub expanded_tag_ids: Vec<u32>,
}

impl Default for TagSettings {
    fn default() -> Self {
        Self {
            default_match_mode: "ALL".to_string(),
            default_scope_mode: "Folder".to_string(),
            expanded_tag_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    pub is_maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: 1280,
            height: 800,
            is_maximized: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub performance: PerformanceSettings,
    pub player: PlayerSettings,
    pub tags: TagSettings,
    pub window: WindowState,
}
