//! Single point of truth for every default, minimum and maximum of the settings (one section per topic).

use super::DeletionMode;

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

/// How long a button or menu entry that asks to be held must be held before it acts (0 = a click is enough).
pub const DEFAULT_HOLD_TO_CONFIRM_MS: u32 = 1000;

pub const MAX_HOLD_TO_CONFIRM_MS: u32 = 5000;

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
    let available = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);

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

/// The log level of Settings > Debug (`rustview_core::logging::LogLevel`, stored by name).
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// Log files are written only when the user switches them on.
pub const DEFAULT_LOG_FILES: bool = false;

/// libmpv's detailed log (`mpv.log`) only when the user switches it on.
pub const DEFAULT_MPV_LOG: bool = false;

/// The cards of Settings > Debug that are open the first time.
pub const DEFAULT_DEBUG_OPEN_CARDS: [&str; 2] = ["dependencies", "health"];

/// The size of the log window the first time it opens (logical pixels).
pub const DEFAULT_LOG_WINDOW_WIDTH: u32 = 900;
pub const DEFAULT_LOG_WINDOW_HEIGHT: u32 = 560;

// =============================================================================
// SECTION 10: Viewer Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_VIEWER_OPEN_IMAGES_IN_TAB: bool = true;

pub const DEFAULT_VIEWER_ALLOW_MULTIPLE_IMAGE_TABS: bool = false;

pub const DEFAULT_VIEWER_OPEN_VIDEOS_IN_TAB: bool = true;

pub const DEFAULT_VIEWER_ALLOW_MULTIPLE_VIDEO_TABS: bool = true;

/// How far the jump keys of the video player move (Settings > Hotkeys > Video Player), in seconds.
pub const DEFAULT_VIEWER_VIDEO_JUMP_SECS: u32 = 5;

/// How far the long jump keys move, in seconds.
pub const DEFAULT_VIEWER_VIDEO_JUMP_LONG_SECS: u32 = 60;

pub const MIN_VIEWER_VIDEO_JUMP_SECS: u32 = 1;

pub const MAX_VIEWER_VIDEO_JUMP_SECS: u32 = 600;

/// Support for the mpv script `ab_repeat.lua` (Settings > Multiview, `docs/mpv_ab_repeat.md`): look for it in the players.
pub const DEFAULT_AB_REPEAT_ENABLED: bool = true;

/// Let the script show its own on-screen messages in `RustView`'s players (as in mpv; off sets `ab_repeat-osd_messages=no`).
pub const DEFAULT_AB_REPEAT_OSD_MESSAGES: bool = true;

/// The colors of saved repeats in the playbar: "each" (each its own color) or "grey" (all grey, the active A-B yellow).
pub const DEFAULT_AB_REPEAT_COLORS: &str = "each";

/// A click on a saved repeat in the playbar loads it.
pub const DEFAULT_AB_REPEAT_CLICK_LOADS: bool = true;

/// The pointer over a saved repeat in the playbar shows its name / its times.
pub const DEFAULT_AB_REPEAT_HOVER_NAME: bool = true;

pub const DEFAULT_AB_REPEAT_HOVER_TIMES: bool = false;

/// The A and B flags of the playbar can be dragged.
pub const DEFAULT_AB_REPEAT_DRAG_FLAGS: bool = true;

/// The load menu: "pages" (pages of `page_size` entries) or "scroll" (one list, `page_size` rows high).
pub const DEFAULT_AB_REPEAT_MENU: &str = "pages";

pub const DEFAULT_AB_REPEAT_PAGE_SIZE: u32 = 10;

pub const MIN_AB_REPEAT_PAGE_SIZE: u32 = 5;

pub const MAX_AB_REPEAT_PAGE_SIZE: u32 = 50;

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

/// The crop button of an image (the crop tool; Save, Undo and Redo appear by themselves while a picture has edits).
pub const DEFAULT_FULLSCREEN_MEDIA_BAR_SHOW_CROP: bool = true;

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

/// Who has the sound while several videos are shown: "first" (leftmost), "newest" (the video that joined last), "selected"
/// (the selected video; the leftmost until one is selected) or "nobody" (every video starts muted).
pub const DEFAULT_FULLSCREEN_SOUND_FOLLOWS: &str = "first";

/// The sound passes to another video when the video that had it leaves.
pub const DEFAULT_FULLSCREEN_SOUND_HAND_OVER: bool = true;
/// Whether a rating given with Ctrl+0..5 in a Multiview shows its icon on the tiles for a moment.
pub const DEFAULT_FULLSCREEN_RATING_SHOW_FEEDBACK: bool = true;

/// How the selected medium of a Multiview of several media is marked: "off", "weak", "medium" or "strong" (the lighter area
/// around it) and "short", "medium" or "long" (the corner marks).
pub const DEFAULT_FULLSCREEN_SELECTION_BRIGHTEN: &str = "medium";

pub const DEFAULT_FULLSCREEN_SELECTION_CORNER_MARKS: &str = "medium";

/// The marks of the selected medium are hidden while the resting mouse cursor is hidden.
pub const DEFAULT_FULLSCREEN_HIDE_SELECTION_WITH_CURSOR: bool = true;

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

/// "`bottom_left`" or "`bottom_right`".
pub const DEFAULT_FULLSCREEN_ZOOM_MINIMAP_POSITION: &str = "bottom_left";

/// The minimap is shown for as long as the picture is zoomed in (otherwise: mouse-over and a few seconds after a zoom).
pub const DEFAULT_FULLSCREEN_ZOOM_MINIMAP_ALWAYS: bool = false;

/// How long the minimap stays after the zoom changed, in seconds.
pub const DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS: u32 = 3;

pub const MIN_FULLSCREEN_ZOOM_MINIMAP_SECS: u32 = 1;

pub const MAX_FULLSCREEN_ZOOM_MINIMAP_SECS: u32 = 30;

/// "lanczos3" (best quality), "`catmull_rom`" (bicubic), "bilinear" or "nearest" (hard pixels).
pub const DEFAULT_FULLSCREEN_ZOOM_RESAMPLE: &str = "lanczos3";

// --- Video control bar of the Multiview (Settings > Multiview > Player) ---
/// "bar" (a translucent bar over the whole width of the video) or "pill" (a floating pill with a margin to the edges).
pub const DEFAULT_FULLSCREEN_PLAYER_BAR_STYLE: &str = "bar";

/// How opaque the background of the bar is, in percent (only where a compositing manager blends the bar).
pub const DEFAULT_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT: u32 = 70;

pub const MIN_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT: u32 = 20;

pub const MAX_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT: u32 = 100;

/// The height of the bar in logical pixels; its buttons, texts and the progress bar grow with it.
pub const DEFAULT_FULLSCREEN_PLAYER_BAR_HEIGHT_PX: u32 = 30;

pub const MIN_FULLSCREEN_PLAYER_BAR_HEIGHT_PX: u32 = 20;

pub const MAX_FULLSCREEN_PLAYER_BAR_HEIGHT_PX: u32 = 64;

/// "bottom" (the pointer in the lower part of the video shows the bar) or "anywhere" (any movement over the video shows it,
/// it goes after `MULTIVIEW_VIDEO_BAR_IDLE_MS` without movement).
pub const DEFAULT_FULLSCREEN_PLAYER_BAR_REVEAL: &str = "bottom";

/// The bar fades in and out (`MULTIVIEW_VIDEO_BAR_FADE_MS`) instead of appearing and vanishing at once.
pub const DEFAULT_FULLSCREEN_PLAYER_BAR_FADE: bool = true;

/// The times of a new video start with milliseconds (a click on a time switches them for that video only).
pub const DEFAULT_FULLSCREEN_PLAYER_SHOW_MS: bool = false;

/// How a click or a drag on the progress bar seeks: "exact" (to the exact frame, also while dragging), "osc" (like mpv's own
/// controller: a click exactly, a drag by keyframes), "keyframes" (to the nearest keyframe, the fastest) or "mpv" (as mpv is
/// configured, its `hr-seek` option).
pub const DEFAULT_FULLSCREEN_PLAYER_SEEK: &str = "exact";

/// How long the bar fades in or out.
/// How long a key that moves a video (jumps, frame steps, the A-B repeat keys) shows its control bar, in milliseconds.
pub const MULTIVIEW_VIDEO_BAR_PULSE_MS: u64 = 1500;

pub const MULTIVIEW_VIDEO_BAR_FADE_MS: u64 = 150;

/// Reveal "anywhere": how long the bar stays after the pointer stopped moving over the video.
pub const MULTIVIEW_VIDEO_BAR_IDLE_MS: u64 = 1500;

/// Reveal "bottom": how far above the bar the pointer may be and still reveal it, in logical pixels.
pub const MULTIVIEW_VIDEO_BAR_REVEAL_MARGIN_LOGICAL_PX: f32 = 12.0;

/// The margin of the floating pill to the edges of the video, in logical pixels.
pub const MULTIVIEW_VIDEO_BAR_PILL_INSET_LOGICAL_PX: f32 = 10.0;

/// A video narrower or lower than this (logical pixels) gets no control bar.
pub const MULTIVIEW_VIDEO_BAR_MIN_VIDEO_WIDTH_LOGICAL_PX: f32 = 200.0;

/// The small control bar of a video in a tab grid item: its height in logical pixels (it does not follow the setting).
pub const MULTIVIEW_VIDEO_BAR_MINI_HEIGHT_LOGICAL_PX: f32 = 22.0;

/// A video in a tab grid item narrower than this (logical pixels) gets no small control bar.
pub const MULTIVIEW_VIDEO_BAR_MINI_MIN_VIDEO_WIDTH_LOGICAL_PX: f32 = 120.0;

/// The time badge of a video in a tab grid item: its font height and its distance from the edges, in logical pixels.
pub const MULTIVIEW_TIME_BADGE_FONT_LOGICAL_PX: f32 = 12.0;

pub const MULTIVIEW_TIME_BADGE_MARGIN_LOGICAL_PX: f32 = 5.0;

/// One notch of the mouse wheel over a video or its volume bar changes the volume by this many percent.
pub const MULTIVIEW_VIDEO_WHEEL_VOLUME_STEP: f64 = 5.0;

/// The volume bar a video shows briefly after the wheel or a key changed its volume: the height of the bar and of the speaker
/// icon left of it, in logical pixels (the bar never takes more than half the width of the video; a narrow one makes it smaller).
pub const MULTIVIEW_VOLUME_BAR_HEIGHT_LOGICAL_PX: f32 = 24.0;

/// The width of the bar itself (without the icon) where the video has room for it, in logical pixels.
pub const MULTIVIEW_VOLUME_BAR_WIDTH_LOGICAL_PX: f32 = 240.0;

/// The space between the speaker icon and the bar, in logical pixels.
pub const MULTIVIEW_VOLUME_BAR_GAP_LOGICAL_PX: f32 = 4.0;

/// The least distance of the volume bar from the bottom of the video, in logical pixels (above the control bar).
pub const MULTIVIEW_VOLUME_BAR_BOTTOM_LOGICAL_PX: f32 = 72.0;

/// The distance of the volume bar from the bottom of a tall video, as a share of the video's height.
pub const MULTIVIEW_VOLUME_BAR_BOTTOM_SHARE: f32 = 0.12;

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

/// The selected medium of a Multiview of 2+ tiles: white over its column (where the medium leaves room), in percent, for the
/// steps of Settings > Multiview > Selected Medium ("medium" is the default).
pub const MULTIVIEW_FOCUS_BRIGHTEN_PERCENT: u8 = 7;

pub const MULTIVIEW_FOCUS_BRIGHTEN_LEVELS: [(&str, u8); 4] = [
    ("off", 0),
    ("weak", 4),
    ("medium", MULTIVIEW_FOCUS_BRIGHTEN_PERCENT),
    ("strong", 11),
];

/// Length of the corner marks of the selected medium (logical pixels) for the steps of the setting, and their thickness.
pub const MULTIVIEW_FOCUS_CORNER_LOGICAL_PX: u32 = 24;

pub const MULTIVIEW_FOCUS_CORNER_LEVELS: [(&str, u32); 3] = [
    ("short", 16),
    ("medium", MULTIVIEW_FOCUS_CORNER_LOGICAL_PX),
    ("long", 32),
];

pub const MULTIVIEW_FOCUS_CORNER_THICKNESS_LOGICAL_PX: u32 = 2;

/// Ticks during which the display parameters of a recycled player are taken for those of the previous file if they are equal.
pub const MULTIVIEW_STALE_PROBE_TICKS: u32 = 20;

/// The native window handle appears a moment after `show()`; the session polls for it every `MULTIVIEW_OPEN_POLL_MS`.
pub const MULTIVIEW_OPEN_POLL_MS: u64 = 15;

pub const MULTIVIEW_OPEN_XID_ATTEMPTS: u32 = 40;

/// Videos smaller than this (host pixels) are not drawn (their textures would be too small to show anything).
pub const MULTIVIEW_MIN_VIDEO_WINDOW_PX: i32 = 16;

/// The action bars of the Multiview (`viewer/multiview/bar_placement.rs`, drawn by `ui/multiview_bar.slint`, which gets the sizes
/// below through its `MultiviewBarMetrics` global): the distance of a bar from the corner of its area.
pub const MULTIVIEW_BAR_MARGIN_LOGICAL_PX: f32 = 8.0;

/// A button of an action bar is a square of this size.
pub const MULTIVIEW_BAR_BUTTON_LOGICAL_PX: f32 = 28.0;

/// The gap between two buttons of an action bar.
pub const MULTIVIEW_BAR_SPACING_LOGICAL_PX: f32 = 2.0;

/// The padding of an action bar around its buttons.
pub const MULTIVIEW_BAR_INSET_LOGICAL_PX: f32 = 3.0;

/// The arrow of a split button (the Save button's drop-down) makes it this much wider.
pub const MULTIVIEW_BAR_ARROW_LOGICAL_PX: f32 = 16.0;

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

// =============================================================================
// SECTION 12: API & External Tools Settings - Single Point of Truth Constants
// =============================================================================
pub const DEFAULT_AUTO_COMPLETE_EXTERNAL_STUBS: bool = true;

// =============================================================================
// SECTION 13: Image Edit (cropping, saving, format presets) - Single Point of Truth Constants
// =============================================================================
/// The aspect ratios of the crop tool's ratio button, in this order (landscape ones on left click, portrait ones on right
/// click, square ones on both; the original ratio "O" is always there).
pub const DEFAULT_CROP_ASPECT_RATIOS: &[&str] = &[
    "1:1", "5:4", "4:3", "3:2", "16:10", "16:9", "21:9", "4:5", "3:4", "2:3", "9:16",
];
/// At most this many presets are kept.
pub const MAX_CROP_ASPECT_RATIOS: usize = 32;
/// When the rule-of-thirds lines show inside the crop rectangle: "dragging", "always" or "never".
pub const DEFAULT_CROP_GRID: &str = "dragging";
/// The top left corner of a JPEG crop keeps to the block grid, so it is always saved without loss.
pub const DEFAULT_CROP_ALIGN_JPEG_TO_LOSSLESS_GRID: bool = false;
/// How the original is replaced: "safe" (a backup until the new file is in place), "trash" or "direct".
pub const DEFAULT_SAVE_OVERWRITE_MODE: &str = "safe";
pub const DEFAULT_PRESET_JPEG_QUALITY: u8 = 90;
pub const DEFAULT_PRESET_JPEG_SUBSAMPLING: &str = "420";
pub const DEFAULT_PRESET_JPEG_PROGRESSIVE: bool = false;
pub const DEFAULT_PRESET_PNG_COMPRESSION: &str = "default";
pub const DEFAULT_PRESET_WEBP_LOSSLESS: bool = false;
pub const DEFAULT_PRESET_WEBP_QUALITY: u8 = 90;
pub const DEFAULT_PRESET_AVIF_QUALITY: u8 = 80;
pub const DEFAULT_PRESET_AVIF_SPEED: u8 = 6;
pub const DEFAULT_PRESET_TIFF_COMPRESSION: &str = "lzw";
