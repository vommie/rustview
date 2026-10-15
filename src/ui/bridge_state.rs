use crate::ctx_menu::{CtxMenu, CtxMenuActionId};
use crate::library::models::{FolderHierarchySnapshot, FolderId, MediaId, TagId};
use crate::query::filters::IndexedMediaRecord;
use crate::query::MediaStore;
use crate::query::{QueryFilter, SortColumn, SortOrder};
use crate::settings::{
    DEFAULT_IS_INSPECTOR_COLLAPSED, DEFAULT_IS_TAG_HISTORY_COLLAPSED,
    DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB, DEFAULT_MAX_HISTORY_TAGS, DEFAULT_SORT_COLUMN,
    DEFAULT_SORT_ORDER, DEFAULT_TAB_GRID_AUTO_COLUMNS, DEFAULT_TAB_GRID_COLUMNS,
    DEFAULT_TAG_HISTORY_HEIGHT,
};
use crate::system::DesktopApp;
use crate::telemetry::{FolderLoadBenchmark, ViewScrollBenchmark};
use crate::ui::file_selection::FileSelectionModel;
use crate::ui::rubberband_selection::RubberbandSession;
use crate::ui::sidebar_panel_tab_dnd::SidebarPanelTabDndSession;
use crate::ui::tab_dnd::TabDndSession;
use crate::ui::tab_grid::MasonryRect;
use crate::viewer::TabId;
use crate::thumbnails::ThumbnailRamCache;
use crate::ui::browse_window::{RingWindow, ViewGeometry};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

// Standard File Context Menu Action IDs
pub const CTX_ACTION_OPEN: CtxMenuActionId = 1000;
pub const CTX_ACTION_RENAME: CtxMenuActionId = 1001;
pub const CTX_ACTION_SELECT_ALL: CtxMenuActionId = 1002;
pub const CTX_ACTION_CLEAR_SELECTION: CtxMenuActionId = 1003;
pub const CTX_ACTION_DELETE_MEDIA: CtxMenuActionId = 1004;
pub const CTX_ACTION_COPY_NAME: CtxMenuActionId = 1005;
pub const CTX_ACTION_COPY_PATH: CtxMenuActionId = 1006;
pub const CTX_ACTION_COPY_IMAGE: CtxMenuActionId = 1007;
pub const CTX_ACTION_OPEN_IN_FILE_MANAGER: CtxMenuActionId = 1008;
pub const CTX_ACTION_REFRESH_METADATA: CtxMenuActionId = 1009;
pub const CTX_ACTION_RATE_0: CtxMenuActionId = 1010;
pub const CTX_ACTION_RATE_1: CtxMenuActionId = 1011;
pub const CTX_ACTION_RATE_2: CtxMenuActionId = 1012;
pub const CTX_ACTION_RATE_3: CtxMenuActionId = 1013;
pub const CTX_ACTION_RATE_4: CtxMenuActionId = 1014;
pub const CTX_ACTION_RATE_5: CtxMenuActionId = 1015;
pub const CTX_ACTION_REMOVE_FROM_DB: CtxMenuActionId = 1016;
pub const CTX_ACTION_REFRESH_FILES: CtxMenuActionId = 1017;
pub const CTX_ACTION_REGENERATE_THUMB_RANDOM: CtxMenuActionId = 1018;
pub const CTX_ACTION_OPEN_IN_TAB: CtxMenuActionId = 1020;
pub const CTX_ACTION_OPEN_IN_BG_TAB: CtxMenuActionId = 1021;
pub const CTX_ACTION_OPEN_IN_FULLSCREEN: CtxMenuActionId = 1022;
pub const CTX_ACTION_OPEN_WITH_OTHER: CtxMenuActionId = 1050;
pub const CTX_ACTION_OPEN_WITH_APP_BASE: CtxMenuActionId = 10_000;

// Tag Tree Context Menu Action IDs
pub const CTX_TAG_ADD_ROOT: CtxMenuActionId = 2001;
pub const CTX_TAG_EXPAND_ALL: CtxMenuActionId = 2002;
pub const CTX_TAG_COLLAPSE_ALL: CtxMenuActionId = 2003;
pub const CTX_TAG_REFRESH: CtxMenuActionId = 2004;

pub const CTX_TAG_ADD_CHILD: CtxMenuActionId = 2010;
pub const CTX_TAG_RENAME: CtxMenuActionId = 2011;
pub const CTX_TAG_DELETE: CtxMenuActionId = 2012;
pub const CTX_TAG_MOVE_TO_ROOT: CtxMenuActionId = 2013;
pub const CTX_TAG_MOVE_TARGET_BASE: CtxMenuActionId = 100_000;

// Folder Tree Context Menu Action IDs
pub const CTX_FOLDER_OPEN_IN_FILE_MANAGER: CtxMenuActionId = 3000;
pub const CTX_FOLDER_RENAME: CtxMenuActionId = 3001;
pub const CTX_FOLDER_DELETE: CtxMenuActionId = 3002;
pub const CTX_FOLDER_PURGE_DB: CtxMenuActionId = 3003;
pub const CTX_FOLDER_REFRESH: CtxMenuActionId = 3004;
pub const CTX_FOLDER_REFRESH_FILES_FULL: CtxMenuActionId = 3004;
pub const CTX_FOLDER_REFRESH_METADATA: CtxMenuActionId = 3005;
pub const CTX_FOLDER_REFRESH_FILES_META: CtxMenuActionId = 3005;
pub const CTX_FOLDER_REFRESH_TREE_FULL: CtxMenuActionId = 3006;
pub const CTX_FOLDER_REFRESH_TREE_RELATIVE: CtxMenuActionId = 3007;
pub const CTX_FOLDER_ADD_TO_FAVORITES: CtxMenuActionId = 3008;
pub const CTX_FOLDER_MOVE_BASE: CtxMenuActionId = 300_000;

// Media Batch Destination Action IDs
pub const CTX_ACTION_COPY_TO_BASE: CtxMenuActionId = 500_000;
pub const CTX_ACTION_MOVE_TO_BASE: CtxMenuActionId = 600_000;

// Tab Context Menu Action IDs
pub const CTX_TAB_DUPLICATE: CtxMenuActionId = 4001;
pub const CTX_TAB_MOVE_TO_START: CtxMenuActionId = 4002;
pub const CTX_TAB_MOVE_LEFT: CtxMenuActionId = 4003;
pub const CTX_TAB_MOVE_RIGHT: CtxMenuActionId = 4004;
pub const CTX_TAB_MOVE_TO_END: CtxMenuActionId = 4005;
pub const CTX_TAB_CLOSE: CtxMenuActionId = 4006;
pub const CTX_TAB_CLOSE_TO_LEFT: CtxMenuActionId = 4007;
pub const CTX_TAB_CLOSE_TO_RIGHT: CtxMenuActionId = 4008;
pub const CTX_TAB_CLOSE_OTHERS: CtxMenuActionId = 4009;
pub const CTX_TAB_OPEN_MULTIVIEW: CtxMenuActionId = 4010;

// Favorites Context Menu Action IDs
pub const CTX_FAVORITE_OPEN_IN_FILE_MANAGER: CtxMenuActionId = 5001;
pub const CTX_FAVORITE_RENAME: CtxMenuActionId = 5002;
pub const CTX_FAVORITE_REMOVE: CtxMenuActionId = 5003;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LastActivePanel {
    #[default]
    Media,
    Folder,
    Favorite,
}

const LIST_BYTES_DIRTY: u64 = u64::MAX;

pub struct UiState {
    pub current_folder_id: Option<FolderId>,
    pub current_folder_path: Option<PathBuf>,
    pub is_virtual_folder_mode: bool,
    pub virtual_folder_count: usize,
    pub active_folder_token: usize,
    pub active_folder_cancel_flag: Option<Arc<AtomicBool>>,
    pub active_folder_bench: Option<Arc<FolderLoadBenchmark>>,
    pub scroll_bench: Arc<ViewScrollBenchmark>,
    pub all_folder_media: MediaStore,
    /// The one RAM cache of decoded thumbnails, shared with the thumbnail loader.
    pub thumbs: Arc<ThumbnailRamCache>,
    pub active_indices: Vec<usize>,
    /// Sum of the file sizes of the listed items (`LIST_BYTES_DIRTY` = not computed since the list or a size changed).
    list_total_bytes: AtomicU64,
    pub file_selection: FileSelectionModel,
    pub last_active_panel: LastActivePanel,
    /// Media of the active viewer tab: one for a single-media tab, the tiles of a Multiview; empty without a shown tab.
    pub active_tab_media_ids: Vec<MediaId>,
    pub rubberband_session: RubberbandSession,
    pub active_tab_dnd_session: Option<TabDndSession>,
    pub active_sidebar_panel_tab: String,
    pub active_sidebar_panel_tab_dnd_session: Option<SidebarPanelTabDndSession>,
    pub is_inspector_collapsed: bool,
    pub media_row_map: HashMap<MediaId, usize>,
    pub filter: QueryFilter,
    pub last_executed_query: String,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    pub is_assign_mode: bool,
    pub is_query_active: bool,
    pub selected_rating: u8,
    pub is_refreshing_media: bool,
    pub active_file_rename_target: Option<MediaId>,
    pub active_associated_apps: Vec<DesktopApp>,
    pub active_tab_dialog_target: Option<TabId>,
    pub active_favorite_target: Option<(String, PathBuf)>,
    pub selected_favorite_path: Option<PathBuf>,

    // Tab Grid Masonry State
    pub tab_grid_columns: usize,
    pub tab_grid_auto_columns: bool,
    pub cached_tab_grid_rects: Vec<MasonryRect>,
    pub cached_tab_grid_total_h: f32,

    // Viewport Windowing State: the tile pool of the grid/table and the viewport it was last filled for.
    pub ring: RingWindow,
    pub view_geometry: Option<ViewGeometry>,

    // Hierarchical Folder Tree SPoT & State
    pub folder_hierarchy: Option<Arc<FolderHierarchySnapshot>>,
    pub expanded_folder_ids: HashSet<FolderId>,
    pub is_refreshing_folders: bool,
    pub folder_scan_count: usize,
    pub active_folder_dialog_target: Option<FolderId>,

    // Hierarchical Tag Tree State
    pub expanded_tag_ids: HashSet<TagId>,
    pub tag_filter_query: String,
    pub tag_filter_is_regex: bool,
    pub active_tag_dialog_target: Option<TagId>,
    pub active_context_menu: Option<CtxMenu>,
    pub tag_tree_available_height: f32,
    pub tag_tree_rows_per_column: usize,

    // Assigned Tags History State
    pub tag_history_ids: Vec<TagId>,
    pub max_history_tags: usize,
    pub is_tag_history_collapsed: bool,
    pub tag_history_height: f32,
    pub tag_history_available_width: f32,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            current_folder_id: None,
            current_folder_path: None,
            is_virtual_folder_mode: false,
            virtual_folder_count: 0,
            active_folder_token: 0,
            active_folder_cancel_flag: None,
            active_folder_bench: None,
            scroll_bench: Arc::new(ViewScrollBenchmark::new(false)),
            all_folder_media: MediaStore::new(),
            thumbs: Arc::new(ThumbnailRamCache::new(0)),
            active_indices: Vec::new(),
            list_total_bytes: AtomicU64::new(LIST_BYTES_DIRTY),
            file_selection: FileSelectionModel::new(),
            last_active_panel: LastActivePanel::Media,
            active_tab_media_ids: Vec::new(),
            rubberband_session: RubberbandSession::default(),
            active_tab_dnd_session: None,
            active_sidebar_panel_tab: DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB.to_string(),
            active_sidebar_panel_tab_dnd_session: None,
            is_inspector_collapsed: DEFAULT_IS_INSPECTOR_COLLAPSED,
            media_row_map: HashMap::new(),
            filter: QueryFilter::default(),
            last_executed_query: String::new(),
            sort_column: SortColumn::from_name(DEFAULT_SORT_COLUMN),
            sort_order: SortOrder::from_name(DEFAULT_SORT_ORDER),
            is_assign_mode: false,
            is_query_active: false,
            selected_rating: 0,
            is_refreshing_media: false,
            active_file_rename_target: None,
            active_associated_apps: Vec::new(),
            active_tab_dialog_target: None,
            active_favorite_target: None,
            selected_favorite_path: None,
            tab_grid_columns: DEFAULT_TAB_GRID_COLUMNS,
            tab_grid_auto_columns: DEFAULT_TAB_GRID_AUTO_COLUMNS,
            cached_tab_grid_rects: Vec::new(),
            cached_tab_grid_total_h: 0.0,
            ring: RingWindow::new(1),
            view_geometry: None,
            folder_hierarchy: None,
            expanded_folder_ids: HashSet::new(),
            is_refreshing_folders: false,
            folder_scan_count: 0,
            active_folder_dialog_target: None,
            expanded_tag_ids: HashSet::new(),
            tag_filter_query: String::new(),
            tag_filter_is_regex: false,
            active_tag_dialog_target: None,
            active_context_menu: None,
            tag_tree_available_height: 450.0,
            tag_tree_rows_per_column: 18,
            tag_history_ids: Vec::new(),
            max_history_tags: DEFAULT_MAX_HISTORY_TAGS,
            is_tag_history_collapsed: DEFAULT_IS_TAG_HISTORY_COLLAPSED,
            tag_history_height: DEFAULT_TAG_HISTORY_HEIGHT,
            tag_history_available_width: 260.0,
        }
    }
}

impl UiState {
    /// The list or a file size changed: the next status bar update sums the sizes again.
    #[inline]
    pub fn mark_list_bytes_dirty(&self) {
        self.list_total_bytes.store(LIST_BYTES_DIRTY, Ordering::Relaxed);
    }

    /// Total size of the listed items; summed once after every change of the list, not on every selection click.
    pub fn visible_total_bytes(&self) -> u64 {
        let known = self.list_total_bytes.load(Ordering::Relaxed);
        if known != LIST_BYTES_DIRTY {
            return known;
        }
        let total: u64 = if self.active_indices.is_empty() {
            self.all_folder_media.iter().map(|item| item.record.file_size).sum()
        } else {
            self.active_indices
                .iter()
                .filter_map(|&index| self.all_folder_media.get(index))
                .map(|item| item.record.file_size)
                .sum()
        };
        self.list_total_bytes.store(total.min(LIST_BYTES_DIRTY - 1), Ordering::Relaxed);
        total
    }

    /// The record at `position` of the sorted, filtered list the grid and the table show.
    #[inline]
    pub fn item_at(&self, position: usize) -> Option<&IndexedMediaRecord> {
        if self.active_indices.is_empty() {
            self.all_folder_media.get(position)
        } else {
            self.active_indices.get(position).and_then(|&index| self.all_folder_media.get(index))
        }
    }

    /// The record of a media item that is part of the list the view shows (O(1)); hidden items yield `None`.
    #[inline]
    pub fn record_by_id(&self, media_id: MediaId) -> Option<&IndexedMediaRecord> {
        self.item_at(*self.media_row_map.get(&media_id)?)
    }

    pub fn get_selected_media_ordered(&self) -> Vec<&IndexedMediaRecord> {
        let selected_ids = self.file_selection.selected_ids();
        if selected_ids.is_empty() || self.active_indices.is_empty() {
            return Vec::new();
        }

        let mut ordered = Vec::with_capacity(selected_ids.len());
        for &idx in &self.active_indices {
            if let Some(item) = self.all_folder_media.get(idx)
                && selected_ids.contains(&item.record.id)
            {
                ordered.push(item);
            }
        }
        ordered
    }

    pub fn get_selected_paths_ordered(&self) -> Vec<PathBuf> {
        self.get_selected_media_ordered()
            .into_iter()
            .map(|item| item.full_path.clone())
            .collect()
    }

    pub fn get_selected_ids_ordered(&self) -> Vec<MediaId> {
        self.get_selected_media_ordered()
            .into_iter()
            .map(|item| item.record.id)
            .collect()
    }

    /// The media an action in the viewer tab acts on, or `None` while no tab content is shown (the caller then falls back to
    /// the Browse selection). A Multiview tab always yields its tiles, so its actions never leak to the Browse selection.
    pub fn active_tab_selection(&self) -> Option<HashSet<MediaId>> {
        (!self.active_tab_media_ids.is_empty())
            .then(|| self.active_tab_media_ids.iter().copied().collect())
    }

    #[inline]
    pub fn is_query_resettable(&self) -> bool {
        !self.is_assign_mode
            && (self.selected_rating > 0
                || !self.filter.tag_ids.is_empty()
                || self.is_query_active)
    }

    /// The tiles hold nothing valid any more: the next viewport update refills every one of them.
    #[inline]
    pub fn invalidate_view_window(&mut self) {
        let pool = self.ring.pool();
        self.ring.reset(pool);
    }

    /// Also forgets the viewport geometry (the other view, the grid or the table, takes over).
    #[inline]
    pub fn reset_view_window(&mut self) {
        self.invalidate_view_window();
        self.view_geometry = None;
    }

    #[inline]
    pub fn invalidate_folder_hierarchy(&mut self) {
        self.folder_hierarchy = None;
    }

    #[inline]
    pub fn set_folder_hierarchy(&mut self, snapshot: FolderHierarchySnapshot) {
        self.folder_hierarchy = Some(Arc::new(snapshot));
    }
}
