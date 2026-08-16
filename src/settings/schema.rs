use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// =============================================================================
// Central Single Point of Truth Constants for Appearance & Grid Presentation
// =============================================================================
pub const DEFAULT_THUMBNAIL_WIDTH: u32 = 190;
pub const DEFAULT_THUMBNAIL_ASPECT_RATIO: &str = "4:3";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GridAspectRatio {
    Ratio1x1 = 0,
    Ratio4x3 = 1,
    Ratio5x4 = 2,
    Ratio16x9 = 3,
    Ratio3x4 = 4,
    Ratio4x5 = 5,
    Ratio9x16 = 6,
}

impl GridAspectRatio {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ratio1x1 => "1:1",
            Self::Ratio4x3 => "4:3",
            Self::Ratio5x4 => "5:4",
            Self::Ratio16x9 => "16:9",
            Self::Ratio3x4 => "3:4",
            Self::Ratio4x5 => "4:5",
            Self::Ratio9x16 => "9:16",
        }
    }

    pub fn from_str_name(s: &str) -> Self {
        match s.trim() {
            "1:1" => Self::Ratio1x1,
            "5:4" => Self::Ratio5x4,
            "16:9" => Self::Ratio16x9,
            "3:4" => Self::Ratio3x4,
            "4:5" => Self::Ratio4x5,
            "9:16" => Self::Ratio9x16,
            _ => Self::Ratio4x3, // Default 4:3
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Ratio1x1,
            2 => Self::Ratio5x4,
            3 => Self::Ratio16x9,
            4 => Self::Ratio3x4,
            5 => Self::Ratio4x5,
            6 => Self::Ratio9x16,
            _ => Self::Ratio4x3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeletionMode {
    Trash,
    Permanent,
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
pub struct FolderSettings {
    pub auto_reveal_remembered_folder: bool,
    pub persist_folder_tree_state: bool,
    pub show_deleted_folders: bool,
    pub show_folder_metrics: bool,
    #[serde(default)]
    pub expanded_folder_ids: Vec<u64>,
}

impl Default for FolderSettings {
    fn default() -> Self {
        Self {
            auto_reveal_remembered_folder: true,
            persist_folder_tree_state: true,
            show_deleted_folders: true,
            show_folder_metrics: true,
            expanded_folder_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub default_thumbnail_size: u32,
    pub thumbnail_aspect_ratio: String,
    pub show_filenames: bool,
    pub show_ratings_overlay: bool,
    pub show_tags_overlay: bool,
    pub enable_hover_previews: bool,
    pub dark_theme: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            default_thumbnail_size: DEFAULT_THUMBNAIL_WIDTH,
            thumbnail_aspect_ratio: DEFAULT_THUMBNAIL_ASPECT_RATIO.to_string(),
            show_filenames: true,
            show_ratings_overlay: true,
            show_tags_overlay: true,
            enable_hover_previews: true,
            dark_theme: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewSettings {
    pub is_table_view: bool,
    pub sort_column: String,
    pub sort_order: String,
    pub col_filename_width: f32,
    pub col_dimensions_width: f32,
    pub col_size_width: f32,
    pub col_rating_width: f32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            is_table_view: false,
            sort_column: "Filename".to_string(),
            sort_order: "Ascending".to_string(),
            col_filename_width: 280.0,
            col_dimensions_width: 140.0,
            col_size_width: 110.0,
            col_rating_width: 90.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceSettings {
    pub worker_threads: usize,
    pub l1_cache_slots: usize,
    pub thumbnail_db_size: u32,
    pub thumbnail_format: String,
    pub thumbnail_quality: u8,
    pub scaling_algorithm: String,
    pub batch_commit_size: usize,
    pub video_engine: String,
    pub video_offset_percent: u8,
    pub generate_video_previews: bool,
    pub preview_frame_count: u32,
    pub custom_preview_settings: bool,
    pub preview_format: String,
    pub preview_quality: u8,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            worker_threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(1, 32),
            l1_cache_slots: 512,
            thumbnail_db_size: 256,
            thumbnail_format: "WebP".to_string(),
            thumbnail_quality: 80,
            scaling_algorithm: "CatmullRom".to_string(),
            batch_commit_size: 32,
            video_engine: "FFmpeg".to_string(),
            video_offset_percent: 10,
            generate_video_previews: true,
            preview_frame_count: 5,
            custom_preview_settings: true,
            preview_format: "WebP".to_string(),
            preview_quality: 40,
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
    #[serde(default)]
    pub folders: FolderSettings,
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub view: ViewSettings,
    pub performance: PerformanceSettings,
    pub player: PlayerSettings,
    pub tags: TagSettings,
    pub window: WindowState,
}
