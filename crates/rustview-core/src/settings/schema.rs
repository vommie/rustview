use crate::media::FormatMask;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// =============================================================================
// SECTION 1: General Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_REMEMBER_LAST_FOLDER: bool = true;
pub const DEFAULT_CONFIRM_ON_FILE_DELETE: bool = true;
/// An "Open" entry (not the Multiview, which has its own limit) that would open this many files or more at once asks first.
pub const DEFAULT_CONFIRM_OPEN_FILE_COUNT: u32 = 5;
pub const MIN_CONFIRM_OPEN_FILE_COUNT: u32 = 2;
pub const MAX_CONFIRM_OPEN_FILE_COUNT: u32 = 500;
pub const DEFAULT_DELETION_MODE: DeletionMode = DeletionMode::Trash;
/// Skip mount points located below the root directory during manual folder tree scans.
pub const DEFAULT_IGNORE_NESTED_MOUNTS: bool = true;

// =============================================================================
// SECTION 2: Folder Tree Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_AUTO_REVEAL_REMEMBERED_FOLDER: bool = true;
pub const DEFAULT_PERSIST_FOLDER_TREE_STATE: bool = true;
pub const DEFAULT_SHOW_DELETED_FOLDERS: bool = true;
pub const DEFAULT_SHOW_FOLDER_METRICS: bool = true;
pub const DEFAULT_SHOW_HIDDEN_FOLDERS: bool = false;
pub const DEFAULT_SUPPORT_DOT_HIDDEN_FILES: bool = true;

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
pub const DEFAULT_SHOW_MEDIA_TYPE_BADGES: bool = true;
pub const DEFAULT_SHOW_DURATION_BADGE: bool = true;
pub const DEFAULT_SHOW_ANIMATED_BADGE: bool = true;
/// Persisted name of the badge overflow behavior ("hide" or "wrap"), see `ThumbnailBadgeOverflow`.
pub const DEFAULT_THUMBNAIL_BADGE_OVERFLOW: &str = "hide";
pub const DEFAULT_ENABLE_HOVER_PREVIEWS: bool = true;
pub const DEFAULT_DARK_THEME: bool = true;
pub const DEFAULT_ENABLE_TOOLTIPS: bool = true;
pub const DEFAULT_TOOLTIP_DELAY_MS: u32 = 750;
pub const MIN_TOOLTIP_DELAY_MS: u32 = 50;
pub const MAX_TOOLTIP_DELAY_MS: u32 = 2000;

// Type-ahead find in the Browse view
pub const DEFAULT_TYPE_AHEAD_ENABLED: bool = true;
/// Keys typed with less than this pause between them form ONE search string (Dolphin and Windows Explorer use one second,
/// Qt 400 ms); a longer pause starts a new search.
pub const DEFAULT_TYPE_AHEAD_TIMEOUT_MS: u32 = 1000;
pub const MIN_TYPE_AHEAD_TIMEOUT_MS: u32 = 300;
pub const MAX_TYPE_AHEAD_TIMEOUT_MS: u32 = 3000;
/// A key typed after a pause this long that does not continue the search string (no name starts with it) is taken for the
/// start of a NEW search; the same key typed faster is a typo inside a word and moves nothing. 0 = never.
pub const DEFAULT_TYPE_AHEAD_NEW_SEARCH_GAP_MS: u32 = 500;
pub const MAX_TYPE_AHEAD_NEW_SEARCH_GAP_MS: u32 = 1500;
pub const DEFAULT_TYPE_AHEAD_INCLUDE_FOLDERS: bool = true;
pub const DEFAULT_TYPE_AHEAD_CYCLE_REPEATED_KEYS: bool = true;
pub const DEFAULT_TYPE_AHEAD_CENTER_TARGET: bool = true;
/// A status bar message "No match for ..." when nothing starts with the typed text.
pub const DEFAULT_TYPE_AHEAD_NO_MATCH_MESSAGE: bool = true;
/// How long that message stays before the status bar shows what it showed before.
pub const TYPE_AHEAD_NO_MATCH_MESSAGE_MS: u64 = 2500;
/// An open Multiview that follows the selection waits this long after the last item type-ahead selected (0 = follows at once),
/// so it does not open a video for every letter typed.
pub const DEFAULT_TYPE_AHEAD_MULTIVIEW_DELAY_MS: u32 = 400;
pub const MAX_TYPE_AHEAD_MULTIVIEW_DELAY_MS: u32 = 2000;

// Grid Items Layout - Single Point of Truth Constants
pub const DEFAULT_GRID_ITEM_PADDING_H: u32 = 8;
pub const MIN_GRID_ITEM_PADDING_H: u32 = 0;
pub const MAX_GRID_ITEM_PADDING_H: u32 = 32;

pub const DEFAULT_GRID_ITEM_PADDING_V: u32 = 6;
pub const MIN_GRID_ITEM_PADDING_V: u32 = 0;
pub const MAX_GRID_ITEM_PADDING_V: u32 = 32;

pub const DEFAULT_GRID_SPACING: u32 = 12;
pub const MIN_GRID_SPACING: u32 = 0;
pub const MAX_GRID_SPACING: u32 = 48;

pub const DEFAULT_GRID_PADDING: u32 = 12;
pub const MIN_GRID_PADDING: u32 = 0;
pub const MAX_GRID_PADDING: u32 = 48;

pub const DEFAULT_GRID_ITEM_BORDER_WIDTH: u32 = 1;
pub const MIN_GRID_ITEM_BORDER_WIDTH: u32 = 0;
pub const MAX_GRID_ITEM_BORDER_WIDTH: u32 = 6;

pub const DEFAULT_GRID_ITEM_CORNER_RADIUS: u32 = 4;
pub const MIN_GRID_ITEM_CORNER_RADIUS: u32 = 0;
pub const MAX_GRID_ITEM_CORNER_RADIUS: u32 = 24;

// =============================================================================
// SECTION 4: View & Column Layout - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_IS_TABLE_VIEW: bool = false;
pub const DEFAULT_SORT_COLUMN: &str = "Filename";
pub const DEFAULT_SORT_ORDER: &str = "Ascending";
pub const DEFAULT_MEDIA_TYPE_FILTER: &str = "Videos & Images";
/// Subfolders of the open folder are listed in the Browse view (grid and table) in front of the media.
pub const DEFAULT_SHOW_FOLDERS_IN_BROWSE: bool = false;
pub const DEFAULT_IS_INSPECTOR_COLLAPSED: bool = false;

// Tab Grid View Defaults
pub const DEFAULT_TAB_GRID_COLUMNS: usize = 3;
pub const MIN_TAB_GRID_COLUMNS: usize = 1;
pub const MAX_TAB_GRID_COLUMNS: usize = 10;
pub const DEFAULT_TAB_GRID_AUTO_COLUMNS: bool = true;
/// Window chrome above the tab grid items (top toolbar + grid panel toolbar) and below them (status bar), in logical pixels.
pub const TAB_GRID_CONTENT_TOP_LOGICAL_PX: f32 = 64.0;
pub const TAB_GRID_STATUS_BAR_LOGICAL_PX: f32 = 28.0;
/// Header of a grid item (title row plus spacing) above its media canvas.
pub const TAB_GRID_ITEM_HEADER_LOGICAL_PX: f32 = 26.0;
/// Smallest media canvas of a grid item (X11 video containers are clamped to the same size).
pub const TAB_GRID_MIN_CANVAS_LOGICAL_PX: f32 = 16.0;

// All 13 Table Columns Defaults
pub const DEFAULT_COL_FILENAME_WIDTH: f32 = 260.0;
pub const DEFAULT_COL_PATH_WIDTH: f32 = 180.0;
pub const DEFAULT_COL_MODIFIED_DATE_WIDTH: f32 = 145.0;
pub const DEFAULT_COL_CREATED_DATE_WIDTH: f32 = 145.0;
pub const DEFAULT_COL_SIZE_WIDTH: f32 = 95.0;
pub const DEFAULT_COL_RATING_WIDTH: f32 = 65.0;
pub const DEFAULT_COL_TAGS_WIDTH: f32 = 65.0;
pub const DEFAULT_COL_WIDTH_WIDTH: f32 = 70.0;
pub const DEFAULT_COL_HEIGHT_WIDTH: f32 = 70.0;
pub const DEFAULT_COL_AR_WIDTH: f32 = 65.0;
pub const DEFAULT_COL_DURATION_WIDTH: f32 = 110.0;
pub const DEFAULT_COL_FORMAT_WIDTH: f32 = 75.0;
pub const DEFAULT_COL_FLAGS_WIDTH: f32 = 85.0;

pub const MIN_COL_WIDTH: f32 = 40.0;
pub const MAX_COL_WIDTH: f32 = 1200.0;

// Sidebar Width Constants
pub const DEFAULT_SIDEBAR_LEFT_WIDTH: f32 = 260.0;
pub const DEFAULT_SIDEBAR_RIGHT_WIDTH: f32 = 280.0;
pub const MIN_SIDEBAR_WIDTH: f32 = 150.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 700.0;

// Sidebar Panel Tabs Constants
pub const DEFAULT_LEFT_SIDEBAR_PANEL_TABS: &[&str] = &["folders", "favorites"];
pub const DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB: &str = "folders";

// =============================================================================
// SECTION 5: Performance, Video & Engine - Single Point of Truth Constants
// =============================================================================
/// RAM budget in megabytes; `0` = automatic (half of the usable RAM, see `settings::memory_budget`).
pub const DEFAULT_MEMORY_BUDGET_MB: u32 = 0;
pub const MIN_MEMORY_BUDGET_MB: u32 = 256;
pub const MAX_MEMORY_BUDGET_MB: u32 = 1_048_576;

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

pub const DEFAULT_SHORT_VIDEO_THRESHOLD_SECS: u32 = 10;
pub const MIN_SHORT_VIDEO_THRESHOLD_SECS: u32 = 1;
pub const MAX_SHORT_VIDEO_THRESHOLD_SECS: u32 = 60;

pub const DEFAULT_GENERATE_VIDEO_PREVIEWS: bool = true;
pub const DEFAULT_PREVIEW_FRAME_COUNT: u32 = 5;
pub const MIN_PREVIEW_FRAME_COUNT: u32 = 2;
pub const MAX_PREVIEW_FRAME_COUNT: u32 = 100;

pub const DEFAULT_CUSTOM_PREVIEW_SETTINGS: bool = true;
pub const DEFAULT_PREVIEW_FORMAT: &str = "WebP";
pub const DEFAULT_PREVIEW_QUALITY: u8 = 40;

/// Dynamically calculates default worker thread count, reserving CPU headroom for Slint UI responsiveness.
#[inline]
pub fn default_worker_threads() -> usize {
    let available = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);

    let reserved = if available > 4 {
        available.saturating_sub(2)
    } else if available > 1 {
        available.saturating_sub(1)
    } else {
        1
    };

    reserved.clamp(MIN_WORKER_THREADS, MAX_WORKER_THREADS)
}

// =============================================================================
// SECTION 6: File Formats & Tools - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_AUTO_DETECT_EXTENSIONLESS_MEDIA: bool = true;

// =============================================================================
// SECTION 7: Tags, Categorization & History - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_TAG_MATCH_MODE: &str = "ALL";
pub const DEFAULT_TAG_SCOPE_MODE: &str = "Global";

pub const DEFAULT_MAX_HISTORY_TAGS: usize = 20;
pub const MIN_MAX_HISTORY_TAGS: usize = 5;
pub const MAX_MAX_HISTORY_TAGS: usize = 100;
pub const DEFAULT_IS_TAG_HISTORY_COLLAPSED: bool = false;
pub const DEFAULT_TAG_HISTORY_HEIGHT: f32 = 96.0;
pub const MIN_TAG_HISTORY_HEIGHT: f32 = 40.0;
pub const MAX_TAG_HISTORY_HEIGHT: f32 = 500.0;

// =============================================================================
// SECTION 8: Window Geometry - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_WINDOW_WIDTH: u32 = 1280;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 800;
pub const MIN_WINDOW_WIDTH: u32 = 600;
/// Upper bound of a remembered maximized window size (physical pixels); anything larger is a damaged file.
pub const MAX_MAXIMIZED_WINDOW_SIZE: u32 = 16_384;
pub const MIN_WINDOW_HEIGHT: u32 = 400;
pub const DEFAULT_WINDOW_MAXIMIZED: bool = false;

// =============================================================================
// SECTION 9: Debug & Privacy Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_OBFUSCATE_LOGS: bool = true;

// =============================================================================
// SECTION 10: Viewer Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_VIEWER_OPEN_IMAGES_IN_TAB: bool = true;
pub const DEFAULT_VIEWER_ALLOW_MULTIPLE_IMAGE_TABS: bool = false;
pub const DEFAULT_VIEWER_OPEN_VIDEOS_IN_TAB: bool = true;
pub const DEFAULT_VIEWER_ALLOW_MULTIPLE_VIDEO_TABS: bool = true;
pub const DEFAULT_VIEWER_SHOW_TAB_RATING_BADGE: bool = false;
pub const DEFAULT_VIEWER_BACKGROUND_MODE: &str = "Dark";

// =============================================================================
// SECTION 11: Fullscreen Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_FULLSCREEN_ENABLE_SPECIFIC_MONITOR: bool = false;
pub const DEFAULT_FULLSCREEN_SPECIFIC_MONITOR: &str = "";
pub const DEFAULT_FULLSCREEN_ENABLE_FALLBACK_RULE: bool = false;
pub const DEFAULT_FULLSCREEN_FALLBACK_TARGET: &str = "same"; // "same" or "different"

// --- Fullscreen action bar (the hover buttons of a medium) ---
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_ENABLED: bool = true;
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_MUTE: bool = true;
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_STICKY: bool = true;
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DUPLICATE: bool = true;
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_AS_TAB: bool = true;
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_WITH: bool = true;
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DELETE: bool = true;
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_REMOVE: bool = true;
/// The zoom button of an image (slider with the zoom of the picture).
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_ZOOM: bool = true;
/// The bar appears when the pointer comes this close (logical pixels) to the place of the bar.
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX: u32 = 15;
pub const MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX: u32 = 0;
pub const MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX: u32 = 100;

// --- Fullscreen primary action bar (view-wide buttons in the top-left corner of the Multiview) ---
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_ENABLED: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_SEND_TO_TAB: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_CLOSE: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_MUTE_ALL: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PLAY_PAUSE_ALL: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_RESET_LAYOUT: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_HIDE_CURSOR: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PIN: bool = true;
pub const DEFAULT_FULLSCREEN_PRIMARY_BAR_REVEAL_DISTANCE_PX: u32 = 15;
/// Hide the mouse cursor over the Multiview after it rested for `idle_cursor_ms` (the button of the primary bar flips it).
pub const DEFAULT_FULLSCREEN_HIDE_IDLE_CURSOR: bool = false;
pub const DEFAULT_FULLSCREEN_IDLE_CURSOR_MS: u32 = 3000;
pub const MIN_FULLSCREEN_IDLE_CURSOR_MS: u32 = 500;
pub const MAX_FULLSCREEN_IDLE_CURSOR_MS: u32 = 30_000;

// --- Fullscreen sound (which videos are muted automatically while several videos are shown) ---
pub const DEFAULT_FULLSCREEN_SOUND_AUTO_CONTROL: bool = true;
/// Who has the sound while several videos are shown: "first" (leftmost), "newest" (the video that joined last) or
/// "nobody" (every video starts muted).
pub const DEFAULT_FULLSCREEN_SOUND_FOLLOWS: &str = "first";
/// The sound passes to another video when the video that had it leaves.
pub const DEFAULT_FULLSCREEN_SOUND_HAND_OVER: bool = true;

// --- Fullscreen image zoom (images only; mpv zooms videos itself) ---
/// A step of Ctrl + mouse wheel over an image, in percent of the fitted size (the wheel over the slider has its own steps: 5%, Shift 10%, Ctrl 25%).
pub const DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT: u32 = 15;
pub const MIN_FULLSCREEN_ZOOM_STEP_PERCENT: u32 = 1;
pub const MAX_FULLSCREEN_ZOOM_STEP_PERCENT: u32 = 100;
/// The largest zoom, in percent of the fitted size.
pub const DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT: u32 = 1000;
pub const MIN_FULLSCREEN_ZOOM_MAX_PERCENT: u32 = 200;
pub const MAX_FULLSCREEN_ZOOM_MAX_PERCENT: u32 = 6400;
/// How far the picture may shrink: the smallest zoom is 100% / (1 + this / 100), so 200 = a third, 0 = no zooming out at all.
pub const DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT: u32 = 200;
pub const MIN_FULLSCREEN_ZOOM_OUT_PERCENT: u32 = 0;
pub const MAX_FULLSCREEN_ZOOM_OUT_PERCENT: u32 = 1000;
/// Zooms glide (a short animation) instead of jumping.
pub const DEFAULT_FULLSCREEN_ZOOM_SMOOTH: bool = true;
/// "bottom_left" or "bottom_right".
pub const DEFAULT_FULLSCREEN_ZOOM_MINIMAP_POSITION: &str = "bottom_left";
/// The minimap is shown for as long as the picture is zoomed in (otherwise: mouse-over and a few seconds after a zoom).
pub const DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS: bool = false;
/// How long the minimap stays after the zoom changed, in seconds.
pub const DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS: u32 = 3;
pub const MIN_FULLSCREEN_ZOOM_MINIMAP_SECS: u32 = 1;
pub const MAX_FULLSCREEN_ZOOM_MINIMAP_SECS: u32 = 30;
/// "lanczos3" (best quality), "catmull_rom" (bicubic), "bilinear" or "nearest" (hard pixels).
pub const DEFAULT_FULLSCREEN_ZOOM_RESAMPLE: &str = "lanczos3";

// --- Fullscreen background ---
/// "theme" (color of the app theme), "viewer" (the background of the viewer settings) or "custom" (`color`).
pub const DEFAULT_FULLSCREEN_BACKGROUND_SOURCE: &str = "theme";
/// Custom color as `#rrggbb`; equals the dark theme background so that switching to "custom" starts from what is shown.
pub const DEFAULT_FULLSCREEN_BACKGROUND_COLOR: &str = "#11111b";

// --- Multiview: 1 to 4 media in the fullscreen (not user-configurable) ---
pub const MULTIVIEW_MAX_TILES: usize = 4;
/// Minimum aspect ratio (width / height) for a medium to count as widescreen; exactly four widescreen media form a 2x2 grid.
pub const MULTIVIEW_WRAP_MIN_ASPECT: f32 = 1.2;
pub const MULTIVIEW_FALLBACK_ASPECT: f32 = 16.0 / 9.0;
pub const MULTIVIEW_GUTTER_LOGICAL_PX: f32 = 16.0;
pub const MULTIVIEW_MIN_COLUMN_LOGICAL_PX: f32 = 160.0;
pub const MULTIVIEW_SNAP_LOGICAL_PX: f32 = 8.0;
/// A refused attempt to close a pinned fullscreen shows the primary bar for this long, and its pin button blinks every
/// `MULTIVIEW_PIN_BLINK_MS`.
pub const MULTIVIEW_PIN_PULSE_MS: u64 = 1600;
pub const MULTIVIEW_PIN_BLINK_MS: u64 = 200;
pub const MULTIVIEW_SELECTION_DEBOUNCE_MS: u64 = 150;
pub const MULTIVIEW_TICK_MS: u64 = 50;
pub const MULTIVIEW_DOUBLE_CLICK_MS: u64 = 400;
/// Ticks during which the display parameters of a recycled player are taken for those of the previous file if they are equal.
pub const MULTIVIEW_STALE_PROBE_TICKS: u32 = 20;
/// The native window handle appears a moment after `show()`; the session polls for it every `MULTIVIEW_OPEN_POLL_MS`.
pub const MULTIVIEW_OPEN_POLL_MS: u64 = 15;
pub const MULTIVIEW_OPEN_XID_ATTEMPTS: u32 = 40;
/// Video windows smaller than this (host pixels) are hidden: X11 containers are clamped to 16 px, which would distort the tile.
pub const MULTIVIEW_MIN_VIDEO_WINDOW_PX: i32 = 16;
/// The action bar floats in a small window of its own (see `viewer/multiview/bar_placement.rs`): its distance from the
/// corner of the hovered tile, and the size of the window, which leaves room for a tooltip below the bar.
pub const MULTIVIEW_BAR_MARGIN_LOGICAL_PX: f32 = 8.0;
pub const MULTIVIEW_OVERLAY_WIDTH_LOGICAL_PX: f32 = 360.0;
/// Room below the bar for a tooltip bubble and for the "Open With" drop-down.
pub const MULTIVIEW_OVERLAY_HEIGHT_LOGICAL_PX: f32 = 344.0;
/// Base title of a tab that holds a Multiview; further ones get the usual numbered suffix.
pub const MULTIVIEW_TAB_TITLE: &str = "Multiview";

// --- mpv event observer ---
/// Polling interval of the observer thread of a player that is shown (or in the tab grid) and of one that is hidden.
pub const MPV_OBSERVER_ACTIVE_POLL_MS: u64 = 20;
pub const MPV_OBSERVER_IDLE_POLL_MS: u64 = 100;

// --- shutdown ---
/// A shutdown that has not finished after this long is ended by the watchdog (a stuck teardown must not leave an invisible
/// process behind).
pub const SHUTDOWN_WATCHDOG_MS: u64 = 10_000;
/// How long the exit waits for the players that closed Multiviews are still shutting down on their worker threads.
pub const SHUTDOWN_TEARDOWN_WAIT_MS: u64 = 3_000;
/// How long the teardown of a closed video tab waits until nothing else holds its player (short-lived references of UI code that
/// is still finishing) before it gives up and leaves the window alone.
pub const PLAYER_TEARDOWN_WAIT_MS: u64 = 2_000;

// =============================================================================
// SECTION 12: API & External Tools Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_AUTO_COMPLETE_EXTERNAL_STUBS: bool = true;

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

/// Behavior of thumbnail badges when a thumbnail is too small to show all of them.
/// Persisted as a lowercase string so a hand-edited settings file can never break parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbnailBadgeOverflow {
    /// Hide badges one by one: secondary (right) first, then primary (left).
    Hide = 0,
    /// Wrap badges onto additional lines.
    Wrap = 1,
}

impl ThumbnailBadgeOverflow {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hide => "hide",
            Self::Wrap => "wrap",
        }
    }

    pub fn from_str_name(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "wrap" => Self::Wrap,
            _ => Self::Hide,
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::Wrap,
            _ => Self::Hide,
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
    pub confirm_open_file_count: u32,
    pub deletion_mode: DeletionMode,
    /// If enabled, mount points located below the root directory are skipped by folder tree scans.
    pub ignore_nested_mounts: bool,
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
pub struct AppearanceSettings {
    pub default_thumbnail_size: u32,
    pub thumbnail_aspect_ratio: String,
    pub show_filenames: bool,
    pub show_ratings_overlay: bool,
    pub show_tags_overlay: bool,
    pub show_media_type_badges: bool,
    pub show_duration_badge: bool,
    /// "ANIM" badge on animated pictures (GIF, APNG, WebP, AVIF sequences).
    pub show_animated_badge: bool,
    /// Overflow behavior of thumbnail badges: "hide" or "wrap" (see `ThumbnailBadgeOverflow`).
    pub thumbnail_badge_overflow: String,
    pub enable_hover_previews: bool,
    pub dark_theme: bool,
    pub enable_tooltips: bool,
    pub tooltip_delay_ms: u32,

    // Type-ahead find (Browse view)
    pub type_ahead_enabled: bool,
    pub type_ahead_timeout_ms: u32,
    /// 0 = a key that continues no name never starts a new search.
    pub type_ahead_new_search_gap_ms: u32,
    pub type_ahead_include_folders: bool,
    pub type_ahead_cycle_repeated_keys: bool,
    pub type_ahead_center_target: bool,
    pub type_ahead_show_no_match: bool,
    pub type_ahead_multiview_delay_ms: u32,

    // Grid Items Layout
    pub grid_item_padding_horizontal: u32,
    pub grid_item_padding_vertical: u32,
    pub grid_spacing: u32,
    pub grid_padding: u32,
    pub grid_item_border_width: u32,
    pub grid_item_corner_radius: u32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            default_thumbnail_size: DEFAULT_THUMBNAIL_WIDTH,
            thumbnail_aspect_ratio: DEFAULT_THUMBNAIL_ASPECT_RATIO.to_string(),
            show_filenames: DEFAULT_SHOW_FILENAMES,
            show_ratings_overlay: DEFAULT_SHOW_RATINGS_OVERLAY,
            show_tags_overlay: DEFAULT_SHOW_TAGS_OVERLAY,
            show_media_type_badges: DEFAULT_SHOW_MEDIA_TYPE_BADGES,
            show_duration_badge: DEFAULT_SHOW_DURATION_BADGE,
            show_animated_badge: DEFAULT_SHOW_ANIMATED_BADGE,
            thumbnail_badge_overflow: DEFAULT_THUMBNAIL_BADGE_OVERFLOW.to_string(),
            enable_hover_previews: DEFAULT_ENABLE_HOVER_PREVIEWS,
            dark_theme: DEFAULT_DARK_THEME,
            enable_tooltips: DEFAULT_ENABLE_TOOLTIPS,
            tooltip_delay_ms: DEFAULT_TOOLTIP_DELAY_MS,
            type_ahead_enabled: DEFAULT_TYPE_AHEAD_ENABLED,
            type_ahead_timeout_ms: DEFAULT_TYPE_AHEAD_TIMEOUT_MS,
            type_ahead_new_search_gap_ms: DEFAULT_TYPE_AHEAD_NEW_SEARCH_GAP_MS,
            type_ahead_include_folders: DEFAULT_TYPE_AHEAD_INCLUDE_FOLDERS,
            type_ahead_cycle_repeated_keys: DEFAULT_TYPE_AHEAD_CYCLE_REPEATED_KEYS,
            type_ahead_center_target: DEFAULT_TYPE_AHEAD_CENTER_TARGET,
            type_ahead_show_no_match: DEFAULT_TYPE_AHEAD_NO_MATCH_MESSAGE,
            type_ahead_multiview_delay_ms: DEFAULT_TYPE_AHEAD_MULTIVIEW_DELAY_MS,
            grid_item_padding_horizontal: DEFAULT_GRID_ITEM_PADDING_H,
            grid_item_padding_vertical: DEFAULT_GRID_ITEM_PADDING_V,
            grid_spacing: DEFAULT_GRID_SPACING,
            grid_padding: DEFAULT_GRID_PADDING,
            grid_item_border_width: DEFAULT_GRID_ITEM_BORDER_WIDTH,
            grid_item_corner_radius: DEFAULT_GRID_ITEM_CORNER_RADIUS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSettings {
    pub is_table_view: bool,
    pub sort_column: String,
    pub sort_order: String,
    pub media_type_filter: String,
    pub show_folders_in_browse: bool,
    pub is_inspector_collapsed: bool,

    // Tab Grid Settings
    pub tab_grid_columns: usize,
    pub tab_grid_auto_columns: bool,

    // All 13 Column Widths
    pub col_filename_width: f32,
    pub col_path_width: f32,
    pub col_modified_date_width: f32,
    pub col_created_date_width: f32,
    pub col_size_width: f32,
    pub col_rating_width: f32,
    pub col_tags_width: f32,
    pub col_width_width: f32,
    pub col_height_width: f32,
    pub col_ar_width: f32,
    pub col_duration_width: f32,
    pub col_format_width: f32,
    pub col_flags_width: f32,

    pub sidebar_left_width: f32,
    pub sidebar_right_width: f32,

    // Sidebar Panel Tabs Configuration (Dock panel tabs in left sidebar)
    pub left_sidebar_panel_tabs: Vec<String>,
    pub left_sidebar_active_panel_tab: String,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            is_table_view: DEFAULT_IS_TABLE_VIEW,
            sort_column: DEFAULT_SORT_COLUMN.to_string(),
            sort_order: DEFAULT_SORT_ORDER.to_string(),
            media_type_filter: DEFAULT_MEDIA_TYPE_FILTER.to_string(),
            show_folders_in_browse: DEFAULT_SHOW_FOLDERS_IN_BROWSE,
            is_inspector_collapsed: DEFAULT_IS_INSPECTOR_COLLAPSED,

            tab_grid_columns: DEFAULT_TAB_GRID_COLUMNS,
            tab_grid_auto_columns: DEFAULT_TAB_GRID_AUTO_COLUMNS,

            col_filename_width: DEFAULT_COL_FILENAME_WIDTH,
            col_path_width: DEFAULT_COL_PATH_WIDTH,
            col_modified_date_width: DEFAULT_COL_MODIFIED_DATE_WIDTH,
            col_created_date_width: DEFAULT_COL_CREATED_DATE_WIDTH,
            col_size_width: DEFAULT_COL_SIZE_WIDTH,
            col_rating_width: DEFAULT_COL_RATING_WIDTH,
            col_tags_width: DEFAULT_COL_TAGS_WIDTH,
            col_width_width: DEFAULT_COL_WIDTH_WIDTH,
            col_height_width: DEFAULT_COL_HEIGHT_WIDTH,
            col_ar_width: DEFAULT_COL_AR_WIDTH,
            col_duration_width: DEFAULT_COL_DURATION_WIDTH,
            col_format_width: DEFAULT_COL_FORMAT_WIDTH,
            col_flags_width: DEFAULT_COL_FLAGS_WIDTH,

            sidebar_left_width: DEFAULT_SIDEBAR_LEFT_WIDTH,
            sidebar_right_width: DEFAULT_SIDEBAR_RIGHT_WIDTH,

            left_sidebar_panel_tabs: DEFAULT_LEFT_SIDEBAR_PANEL_TABS.iter().map(|s| s.to_string()).collect(),
            left_sidebar_active_panel_tab: DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB.to_string(),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerImageSettings {
    pub open_in_tab: bool,
    pub allow_multiple_tabs: bool,
}

impl Default for ViewerImageSettings {
    fn default() -> Self {
        Self {
            open_in_tab: DEFAULT_VIEWER_OPEN_IMAGES_IN_TAB,
            allow_multiple_tabs: DEFAULT_VIEWER_ALLOW_MULTIPLE_IMAGE_TABS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerVideoSettings {
    pub open_in_tab: bool,
    pub allow_multiple_tabs: bool,
}

impl Default for ViewerVideoSettings {
    fn default() -> Self {
        Self {
            open_in_tab: DEFAULT_VIEWER_OPEN_VIDEOS_IN_TAB,
            allow_multiple_tabs: DEFAULT_VIEWER_ALLOW_MULTIPLE_VIDEO_TABS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerTabSettings {
    pub show_rating_badge: bool,
}

impl Default for ViewerTabSettings {
    fn default() -> Self {
        Self {
            show_rating_badge: DEFAULT_VIEWER_SHOW_TAB_RATING_BADGE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerSettings {
    pub images: ViewerImageSettings,
    pub videos: ViewerVideoSettings,
    pub tabs: ViewerTabSettings,
    pub background_mode: String,
}

impl Default for ViewerSettings {
    fn default() -> Self {
        Self {
            images: ViewerImageSettings::default(),
            videos: ViewerVideoSettings::default(),
            tabs: ViewerTabSettings::default(),
            background_mode: DEFAULT_VIEWER_BACKGROUND_MODE.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenMediaBarSettings {
    pub enabled: bool,
    pub show_mute: bool,
    pub show_sticky: bool,
    pub show_duplicate: bool,
    pub show_open_as_tab: bool,
    pub show_open_with: bool,
    pub show_delete: bool,
    /// Remove (several media) and Close (the last medium) are one button that changes its role.
    pub show_remove: bool,
    pub show_zoom: bool,
    pub reveal_distance_px: u32,
}

impl Default for FullscreenMediaBarSettings {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_FULLSCREEN_MEDIA_BAR_ENABLED,
            show_mute: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_MUTE,
            show_sticky: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_STICKY,
            show_duplicate: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DUPLICATE,
            show_open_as_tab: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_AS_TAB,
            show_open_with: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_OPEN_WITH,
            show_delete: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_DELETE,
            show_remove: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_REMOVE,
            show_zoom: DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_ZOOM,
            reveal_distance_px: DEFAULT_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenPrimaryBarSettings {
    pub enabled: bool,
    pub show_send_to_tab: bool,
    pub show_close: bool,
    pub show_mute_all: bool,
    pub show_play_pause_all: bool,
    pub show_reset_layout: bool,
    pub show_hide_cursor: bool,
    pub show_pin: bool,
    pub reveal_distance_px: u32,
    /// Shared with the "hide idle mouse cursor" button of the bar: there is one value, the button writes it.
    pub hide_idle_cursor: bool,
    pub idle_cursor_ms: u32,
}

impl Default for FullscreenPrimaryBarSettings {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_FULLSCREEN_PRIMARY_BAR_ENABLED,
            show_send_to_tab: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_SEND_TO_TAB,
            show_close: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_CLOSE,
            show_mute_all: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_MUTE_ALL,
            show_play_pause_all: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PLAY_PAUSE_ALL,
            show_reset_layout: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_RESET_LAYOUT,
            show_hide_cursor: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_HIDE_CURSOR,
            show_pin: DEFAULT_FULLSCREEN_PRIMARY_BAR_SHOW_PIN,
            reveal_distance_px: DEFAULT_FULLSCREEN_PRIMARY_BAR_REVEAL_DISTANCE_PX,
            hide_idle_cursor: DEFAULT_FULLSCREEN_HIDE_IDLE_CURSOR,
            idle_cursor_ms: DEFAULT_FULLSCREEN_IDLE_CURSOR_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenSoundSettings {
    pub auto_control: bool,
    pub follows: String, // "first", "newest" or "nobody"
    pub hand_over: bool,
}

impl Default for FullscreenSoundSettings {
    fn default() -> Self {
        Self {
            auto_control: DEFAULT_FULLSCREEN_SOUND_AUTO_CONTROL,
            follows: DEFAULT_FULLSCREEN_SOUND_FOLLOWS.to_string(),
            hand_over: DEFAULT_FULLSCREEN_SOUND_HAND_OVER,
        }
    }
}

/// Image zoom in the Multiview (fullscreen and tab view).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenZoomSettings {
    pub step_percent: u32,
    pub max_percent: u32,
    pub out_percent: u32,
    pub smooth: bool,
    pub minimap_position: String, // "bottom_left" or "bottom_right"
    pub minimap_always: bool,
    pub minimap_secs: u32,
    pub resample: String, // "lanczos3", "catmull_rom", "bilinear" or "nearest"
}

impl Default for FullscreenZoomSettings {
    fn default() -> Self {
        Self {
            step_percent: DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT,
            max_percent: DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT,
            out_percent: DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT,
            smooth: DEFAULT_FULLSCREEN_ZOOM_SMOOTH,
            minimap_position: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_POSITION.to_string(),
            minimap_always: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS,
            minimap_secs: DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS,
            resample: DEFAULT_FULLSCREEN_ZOOM_RESAMPLE.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenBackgroundSettings {
    pub source: String, // "theme", "viewer" or "custom"
    pub color: String,  // "#rrggbb"
}

impl Default for FullscreenBackgroundSettings {
    fn default() -> Self {
        Self {
            source: DEFAULT_FULLSCREEN_BACKGROUND_SOURCE.to_string(),
            color: DEFAULT_FULLSCREEN_BACKGROUND_COLOR.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenSettings {
    pub enable_specific_monitor: bool,
    pub specific_monitor: String,
    pub enable_fallback_rule: bool,
    pub fallback_target: String, // "same" or "different"
    /// Called `action_bar` before the primary action bar existed; old settings files keep working.
    #[serde(alias = "action_bar")]
    pub media_bar: FullscreenMediaBarSettings,
    pub primary_bar: FullscreenPrimaryBarSettings,
    pub sound: FullscreenSoundSettings,
    pub background: FullscreenBackgroundSettings,
    pub zoom: FullscreenZoomSettings,
}

impl Default for FullscreenSettings {
    fn default() -> Self {
        Self {
            enable_specific_monitor: DEFAULT_FULLSCREEN_ENABLE_SPECIFIC_MONITOR,
            specific_monitor: DEFAULT_FULLSCREEN_SPECIFIC_MONITOR.to_string(),
            enable_fallback_rule: DEFAULT_FULLSCREEN_ENABLE_FALLBACK_RULE,
            fallback_target: DEFAULT_FULLSCREEN_FALLBACK_TARGET.to_string(),
            media_bar: FullscreenMediaBarSettings::default(),
            primary_bar: FullscreenPrimaryBarSettings::default(),
            sound: FullscreenSoundSettings::default(),
            background: FullscreenBackgroundSettings::default(),
            zoom: FullscreenZoomSettings::default(),
        }
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
}
