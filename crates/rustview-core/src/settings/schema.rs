use crate::media::FormatMask;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod appearance;
mod constants;
mod hotkeys;
mod image_edit;
mod viewer;
pub use appearance::AppearanceSettings;
pub use appearance::GridAspectRatio;
pub use appearance::ThumbnailBadgeOverflow;
pub use appearance::ViewSettings;
pub use constants::*;
pub use hotkeys::HotkeysPageState;
pub use image_edit::{CropSettings, FormatPresets, ImageEditSettings, LastExport, SaveSettings};
pub use viewer::FullscreenBackgroundSettings;
pub use viewer::FullscreenMediaBarSettings;
pub use viewer::FullscreenPlayerSettings;
pub use viewer::FullscreenPrimaryBarSettings;
pub use viewer::FullscreenRatingSettings;
pub use viewer::FullscreenSelectionSettings;
pub use viewer::FullscreenSettings;
pub use viewer::FullscreenSoundSettings;
pub use viewer::FullscreenZoomSettings;
pub use viewer::ViewerImageSettings;
pub use viewer::ViewerSettings;
pub use viewer::ViewerTabSettings;
pub use viewer::ViewerVideoSettings;

// =============================================================================
// Enums & Domain Data Models
// =============================================================================

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
    pub confirm_open_file_count: u32,
    pub deletion_mode: DeletionMode,
    /// If enabled, mount points located below the root directory are skipped by folder tree scans.
    pub ignore_nested_mounts: bool,
    /// Added in 0.4.4: how long a hold-to-confirm element must be held, in milliseconds (0 = a click is enough).
    pub hold_to_confirm_ms: u32,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            root_folder: None,
            remember_last_folder: DEFAULT_REMEMBER_LAST_FOLDER,
            last_folder_path: None,
            confirm_on_file_delete: DEFAULT_CONFIRM_ON_FILE_DELETE,
            confirm_open_file_count: DEFAULT_CONFIRM_OPEN_FILE_COUNT,
            deletion_mode: DEFAULT_DELETION_MODE,
            ignore_nested_mounts: DEFAULT_IGNORE_NESTED_MOUNTS,
            hold_to_confirm_ms: DEFAULT_HOLD_TO_CONFIRM_MS,
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
    pub show_hidden_folders: bool,
    pub support_dot_hidden_files: bool,
    pub expanded_folder_ids: Vec<u64>,
}

impl Default for FolderSettings {
    fn default() -> Self {
        Self {
            auto_reveal_remembered_folder: DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER,
            persist_folder_tree_state: DEFAULT_PERSIST_FOLDER_TREE_STATE,
            show_deleted_folders: DEFAULT_SHOW_DELETED_FOLDERS,
            show_folder_metrics: DEFAULT_SHOW_FOLDER_METRICS,
            show_hidden_folders: DEFAULT_SHOW_HIDDEN_FOLDERS,
            support_dot_hidden_files: DEFAULT_SUPPORT_DOT_HIDDEN_FILES,
            expanded_folder_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceSettings {
    pub worker_threads: usize,
    /// RAM the application may use for caches, in megabytes; `0` = automatic.
    pub memory_budget_mb: u32,
    pub thumbnail_db_size: u32,
    pub thumbnail_format: String,
    pub thumbnail_quality: u8,
    pub scaling_algorithm: String,
    pub batch_commit_size: usize,
    pub video_engine: String,
    pub video_offset_percent: u8,
    pub short_video_threshold_secs: u32,
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
            memory_budget_mb: DEFAULT_MEMORY_BUDGET_MB,
            thumbnail_db_size: DEFAULT_THUMBNAIL_DB_SIZE,
            thumbnail_format: DEFAULT_THUMBNAIL_FORMAT.to_string(),
            thumbnail_quality: DEFAULT_THUMBNAIL_QUALITY,
            scaling_algorithm: DEFAULT_SCALING_ALGORITHM.to_string(),
            batch_commit_size: DEFAULT_BATCH_COMMIT_SIZE,
            video_engine: DEFAULT_VIDEO_ENGINE.to_string(),
            video_offset_percent: DEFAULT_VIDEO_OFFSET_PERCENT,
            short_video_threshold_secs: DEFAULT_SHORT_VIDEO_THRESHOLD_SECS,
            generate_video_previews: DEFAULT_GENERATE_VIDEO_PREVIEWS,
            preview_frame_count: DEFAULT_PREVIEW_FRAME_COUNT,
            custom_preview_settings: DEFAULT_CUSTOM_PREVIEW_SETTINGS,
            preview_format: DEFAULT_PREVIEW_FORMAT.to_string(),
            preview_quality: DEFAULT_PREVIEW_QUALITY,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FormatsSettings {
    pub auto_detect_extensionless_media: bool,
    /// Stable ids (`FormatSpec::id`) of the formats the user switched off. Empty (the default) means every known format
    /// is on, so formats added by a later version are on as well. Their indexed records stay in the library.
    pub disabled_formats: Vec<String>,
}

impl Default for FormatsSettings {
    fn default() -> Self {
        Self {
            auto_detect_extensionless_media: DEFAULT_AUTO_DETECT_EXTENSIONLESS_MEDIA,
            disabled_formats: Vec::new(),
        }
    }
}

impl FormatsSettings {
    /// The disabled formats as a policy mask.
    pub fn format_mask(&self) -> FormatMask {
        FormatMask::from_disabled_ids(&self.disabled_formats)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagSettings {
    pub default_match_mode: String,
    pub default_scope_mode: String,
    pub expanded_tag_ids: Vec<u32>,
    pub history_tag_ids: Vec<u32>,
    pub max_history_tags: usize,
    pub is_history_collapsed: bool,
    pub history_height: f32,
}

impl Default for TagSettings {
    fn default() -> Self {
        Self {
            default_match_mode: DEFAULT_TAG_MATCH_MODE.to_string(),
            default_scope_mode: DEFAULT_TAG_SCOPE_MODE.to_string(),
            expanded_tag_ids: Vec::new(),
            history_tag_ids: Vec::new(),
            max_history_tags: DEFAULT_MAX_HISTORY_TAGS,
            is_history_collapsed: DEFAULT_IS_TAG_HISTORY_COLLAPSED,
            history_height: DEFAULT_TAG_HISTORY_HEIGHT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiSettings {
    pub auto_complete_external_stubs: bool,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            auto_complete_external_stubs: DEFAULT_AUTO_COMPLETE_EXTERNAL_STUBS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugSettings {
    pub obfuscate_logs: bool,
}

impl Default for DebugSettings {
    fn default() -> Self {
        Self {
            obfuscate_logs: DEFAULT_OBFUSCATE_LOGS,
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
    /// Client size of the window the last time it was closed maximized (0 = unknown). The next launch creates the window with
    /// this size, so its first frame already has the size the window manager maximizes it to.
    pub maximized_width: u32,
    pub maximized_height: u32,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            is_maximized: DEFAULT_WINDOW_MAXIMIZED,
            maximized_width: 0,
            maximized_height: 0,
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
    pub formats: FormatsSettings,
    #[serde(default)]
    pub viewer: ViewerSettings,
    #[serde(default)]
    pub fullscreen: FullscreenSettings,
    #[serde(default)]
    pub tags: TagSettings,
    #[serde(default)]
    pub api: ApiSettings,
    #[serde(default)]
    pub debug: DebugSettings,
    #[serde(default)]
    pub window: WindowState,
    /// Added in 0.3.87 (cropping and saving pictures); older files get the defaults.
    #[serde(default)]
    pub image_edit: ImageEditSettings,
    /// Added in 0.4.4: the keys that differ from the defaults (`docs/hotkeys.md`), action id -> list of key texts. Kept as
    /// JSON values so a hand-edited entry never breaks loading; the app validates them (`ui::keymap`).
    #[serde(default)]
    pub hotkeys: std::collections::BTreeMap<String, serde_json::Value>,
}
