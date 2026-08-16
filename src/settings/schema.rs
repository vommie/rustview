use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// =============================================================================
// SECTION 1: General Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_REMEMBER_LAST_FOLDER: bool = true;
pub const DEFAULT_CONFIRM_ON_FILE_DELETE: bool = true;
pub const DEFAULT_DELETION_MODE: DeletionMode = DeletionMode::Trash;

// =============================================================================
// SECTION 2: Folder Tree Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER: bool = true;
pub const DEFAULT_PERSIST_FOLDER_TREE_STATE: bool = true;
pub const DEFAULT_SHOW_DELETED_FOLDERS: bool = true;
pub const DEFAULT_SHOW_FOLDER_METRICS: bool = true;

// =============================================================================
// SECTION 3: Appearance & Grid Presentation - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_THUMBNAIL_WIDTH: u32 = 190;
pub const MIN_THUMBNAIL_WIDTH: u32 = 80;
pub const MAX_THUMBNAIL_WIDTH: u32 = 500;
pub const DEFAULT_THUMBNAIL_ASPECT_RATIO: &str = "4:3";
pub const DEFAULT_SHOW_FILENAMES: bool = true;
pub const DEFAULT_SHOW_RATINGS_OVERLAY: bool = true;
pub const DEFAULT_SHOW_TAGS_OVERLAY: bool = true;
pub const DEFAULT_ENABLE_HOVER_PREVIEWS: bool = true;
pub const DEFAULT_DARK_THEME: bool = true;

// =============================================================================
// SECTION 4: View & Column Layout - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_IS_TABLE_VIEW: bool = false;
pub const DEFAULT_SORT_COLUMN: &str = "Filename";
pub const DEFAULT_SORT_ORDER: &str = "Ascending";
pub const DEFAULT_COL_FILENAME_WIDTH: f32 = 280.0;
pub const DEFAULT_COL_DIMENSIONS_WIDTH: f32 = 140.0;
pub const DEFAULT_COL_SIZE_WIDTH: f32 = 110.0;
pub const DEFAULT_COL_RATING_WIDTH: f32 = 90.0;
pub const MIN_COL_WIDTH: f32 = 50.0;
pub const MAX_COL_WIDTH: f32 = 1200.0;

// =============================================================================
// SECTION 5: Performance, Video & Engine - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_L1_CACHE_SLOTS: usize = 512;
pub const MIN_L1_CACHE_SLOTS: usize = 100;
pub const MAX_L1_CACHE_SLOTS: usize = 8000;

pub const MIN_WORKER_THREADS: usize = 1;
pub const MAX_WORKER_THREADS: usize = 32;

pub const DEFAULT_THUMBNAIL_DB_SIZE: u32 = 256;
pub const MIN_THUMBNAIL_DB_SIZE: u32 = 64;
pub const MAX_THUMBNAIL_DB_SIZE: u32 = 1024;

pub const DEFAULT_THUMBNAIL_FORMAT: &str = "WebP";
pub const DEFAULT_THUMBNAIL_QUALITY: u8 = 80;
pub const MIN_QUALITY: u8 = 10;
pub const MAX_QUALITY: u8 = 100;

pub const DEFAULT_SCALING_ALGORITHM: &str = "CatmullRom";

pub const DEFAULT_BATCH_COMMIT_SIZE: usize = 32;
pub const MIN_BATCH_COMMIT_SIZE: usize = 4;
pub const MAX_BATCH_COMMIT_SIZE: usize = 256;

pub const DEFAULT_VIDEO_ENGINE: &str = "FFmpeg";
pub const DEFAULT_VIDEO_OFFSET_PERCENT: u8 = 10;
pub const MIN_VIDEO_OFFSET_PERCENT: u8 = 1;
pub const MAX_VIDEO_OFFSET_PERCENT: u8 = 90;

pub const DEFAULT_GENERATE_VIDEO_PREVIEWS: bool = true;
pub const DEFAULT_PREVIEW_FRAME_COUNT: u32 = 5;
pub const MIN_PREVIEW_FRAME_COUNT: u32 = 2;
pub const MAX_PREVIEW_FRAME_COUNT: u32 = 100;

pub const DEFAULT_CUSTOM_PREVIEW_SETTINGS: bool = true;
pub const DEFAULT_PREVIEW_FORMAT: &str = "WebP";
pub const DEFAULT_PREVIEW_QUALITY: u8 = 40;

/// Dynamically calculates default worker thread count based on physical CPU cores.
#[inline]
pub fn default_worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(MIN_WORKER_THREADS, MAX_WORKER_THREADS)
}

// =============================================================================
// SECTION 6: Video Player - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_HWDEC_MODE: &str = "auto-safe";
pub const DEFAULT_VIDEO_OUTPUT: &str = "gpu";
pub const DEFAULT_GPU_API: &str = "auto";
pub const DEFAULT_AUDIO_DEVICE: &str = "auto";
pub const DEFAULT_AUTOPLAY_VIDEOS: bool = true;
pub const DEFAULT_PLAYER_VOLUME: f32 = 100.0;
pub const MIN_PLAYER_VOLUME: f32 = 0.0;
pub const MAX_PLAYER_VOLUME: f32 = 100.0;
pub const DEFAULT_PLAYER_KEEP_OPEN: bool = true;

// =============================================================================
// SECTION 7: Tags & Categorization - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_TAG_MATCH_MODE: &str = "ALL";
pub const DEFAULT_TAG_SCOPE_MODE: &str = "Folder";

// =============================================================================
// SECTION 8: Window Geometry - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_WINDOW_WIDTH: u32 = 1280;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 800;
pub const MIN_WINDOW_WIDTH: u32 = 600;
pub const MIN_WINDOW_HEIGHT: u32 = 400;
pub const DEFAULT_WINDOW_MAXIMIZED: bool = false;

// =============================================================================
// Enums & Domain Data Models
// =============================================================================

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
            _ => Self::Ratio4x3,
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
#[serde(default)]
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
            remember_last_folder: DEFAULT_REMEMBER_LAST_FOLDER,
            last_folder_path: None,
            confirm_on_file_delete: DEFAULT_CONFIRM_ON_FILE_DELETE,
            deletion_mode: DEFAULT_DELETION_MODE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderSettings {
    pub auto_reveal_remembered_folder: bool,
    pub persist_folder_tree_state: bool,
    pub show_deleted_folders: bool,
    pub show_folder_metrics: bool,
    pub expanded_folder_ids: Vec<u64>,
}

impl Default for FolderSettings {
    fn default() -> Self {
        Self {
            auto_reveal_remembered_folder: DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER,
            persist_folder_tree_state: DEFAULT_PERSIST_FOLDER_TREE_STATE,
            show_deleted_folders: DEFAULT_SHOW_DELETED_FOLDERS,
            show_folder_metrics: DEFAULT_SHOW_FOLDER_METRICS,
            expanded_folder_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
            show_filenames: DEFAULT_SHOW_FILENAMES,
            show_ratings_overlay: DEFAULT_SHOW_RATINGS_OVERLAY,
            show_tags_overlay: DEFAULT_SHOW_TAGS_OVERLAY,
            enable_hover_previews: DEFAULT_ENABLE_HOVER_PREVIEWS,
            dark_theme: DEFAULT_DARK_THEME,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
            is_table_view: DEFAULT_IS_TABLE_VIEW,
            sort_column: DEFAULT_SORT_COLUMN.to_string(),
            sort_order: DEFAULT_SORT_ORDER.to_string(),
            col_filename_width: DEFAULT_COL_FILENAME_WIDTH,
            col_dimensions_width: DEFAULT_COL_DIMENSIONS_WIDTH,
            col_size_width: DEFAULT_COL_SIZE_WIDTH,
            col_rating_width: DEFAULT_COL_RATING_WIDTH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
            worker_threads: default_worker_threads(),
            l1_cache_slots: DEFAULT_L1_CACHE_SLOTS,
            thumbnail_db_size: DEFAULT_THUMBNAIL_DB_SIZE,
            thumbnail_format: DEFAULT_THUMBNAIL_FORMAT.to_string(),
            thumbnail_quality: DEFAULT_THUMBNAIL_QUALITY,
            scaling_algorithm: DEFAULT_SCALING_ALGORITHM.to_string(),
            batch_commit_size: DEFAULT_BATCH_COMMIT_SIZE,
            video_engine: DEFAULT_VIDEO_ENGINE.to_string(),
            video_offset_percent: DEFAULT_VIDEO_OFFSET_PERCENT,
            generate_video_previews: DEFAULT_GENERATE_VIDEO_PREVIEWS,
            preview_frame_count: DEFAULT_PREVIEW_FRAME_COUNT,
            custom_preview_settings: DEFAULT_CUSTOM_PREVIEW_SETTINGS,
            preview_format: DEFAULT_PREVIEW_FORMAT.to_string(),
            preview_quality: DEFAULT_PREVIEW_QUALITY,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    pub hwdec_mode: String,
    pub autoplay_videos: bool,
    pub default_volume: f32,
    pub keep_open: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            hwdec_mode: DEFAULT_HWDEC_MODE.to_string(),
            autoplay_videos: DEFAULT_AUTOPLAY_VIDEOS,
            default_volume: DEFAULT_PLAYER_VOLUME,
            keep_open: DEFAULT_PLAYER_KEEP_OPEN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagSettings {
    pub default_match_mode: String,
    pub default_scope_mode: String,
    pub expanded_tag_ids: Vec<u32>,
}

impl Default for TagSettings {
    fn default() -> Self {
        Self {
            default_match_mode: DEFAULT_TAG_MATCH_MODE.to_string(),
            default_scope_mode: DEFAULT_TAG_SCOPE_MODE.to_string(),
            expanded_tag_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            is_maximized: DEFAULT_WINDOW_MAXIMIZED,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub folders: FolderSettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub view: ViewSettings,
    #[serde(default)]
    pub performance: PerformanceSettings,
    #[serde(default)]
    pub player: PlayerSettings,
    #[serde(default)]
    pub tags: TagSettings,
    #[serde(default)]
    pub window: WindowState,
}
