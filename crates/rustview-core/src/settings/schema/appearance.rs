//! Appearance and view settings of the browse window (thumbnail grid, table, badges).

use super::{
    DEFAULT_COL_AR_WIDTH, DEFAULT_COL_CREATED_DATE_WIDTH, DEFAULT_COL_DURATION_WIDTH, DEFAULT_COL_FILENAME_WIDTH,
    DEFAULT_COL_FLAGS_WIDTH, DEFAULT_COL_FORMAT_WIDTH, DEFAULT_COL_HEIGHT_WIDTH, DEFAULT_COL_MODIFIED_DATE_WIDTH,
    DEFAULT_COL_PATH_WIDTH, DEFAULT_COL_RATING_WIDTH, DEFAULT_COL_SIZE_WIDTH, DEFAULT_COL_TAGS_WIDTH,
    DEFAULT_COL_WIDTH_WIDTH, DEFAULT_DARK_THEME, DEFAULT_ENABLE_HOVER_PREVIEWS, DEFAULT_ENABLE_TOOLTIPS,
    DEFAULT_GRID_ITEM_BORDER_WIDTH, DEFAULT_GRID_ITEM_CORNER_RADIUS, DEFAULT_GRID_ITEM_PADDING_H,
    DEFAULT_GRID_ITEM_PADDING_V, DEFAULT_GRID_PADDING, DEFAULT_GRID_SPACING, DEFAULT_IS_INSPECTOR_COLLAPSED,
    DEFAULT_IS_TABLE_VIEW, DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB, DEFAULT_LEFT_SIDEBAR_PANEL_TABS,
    DEFAULT_MEDIA_TYPE_FILTER, DEFAULT_SHOW_ANIMATED_BADGE, DEFAULT_SHOW_DURATION_BADGE, DEFAULT_SHOW_FILENAMES,
    DEFAULT_SHOW_FOLDERS_IN_BROWSE, DEFAULT_SHOW_MEDIA_TYPE_BADGES, DEFAULT_SHOW_RATINGS_OVERLAY,
    DEFAULT_SHOW_TAGS_OVERLAY, DEFAULT_SIDEBAR_LEFT_WIDTH, DEFAULT_SIDEBAR_RIGHT_WIDTH, DEFAULT_SORT_COLUMN,
    DEFAULT_SORT_ORDER, DEFAULT_TAB_GRID_AUTO_COLUMNS, DEFAULT_TAB_GRID_COLUMNS, DEFAULT_THUMBNAIL_ASPECT_RATIO,
    DEFAULT_THUMBNAIL_BADGE_OVERFLOW, DEFAULT_THUMBNAIL_WIDTH, DEFAULT_TOOLTIP_DELAY_MS,
    DEFAULT_TYPE_AHEAD_CENTER_TARGET, DEFAULT_TYPE_AHEAD_CYCLE_REPEATED_KEYS, DEFAULT_TYPE_AHEAD_ENABLED,
    DEFAULT_TYPE_AHEAD_INCLUDE_FOLDERS, DEFAULT_TYPE_AHEAD_MULTIVIEW_DELAY_MS, DEFAULT_TYPE_AHEAD_NEW_SEARCH_GAP_MS,
    DEFAULT_TYPE_AHEAD_NO_MATCH_MESSAGE, DEFAULT_TYPE_AHEAD_TIMEOUT_MS,
};
use serde::{Deserialize, Serialize};

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

    /// Added in 0.4.4: the state of the Settings page Hotkeys.
    pub hotkeys_page: super::HotkeysPageState,
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

            left_sidebar_panel_tabs: DEFAULT_LEFT_SIDEBAR_PANEL_TABS
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            left_sidebar_active_panel_tab: DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB.to_string(),
            hotkeys_page: super::HotkeysPageState::default(),
        }
    }
}
