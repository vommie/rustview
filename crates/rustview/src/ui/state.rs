use crate::ctx_menu::{CtxMenu, CtxMenuActionId};
use crate::system::DesktopApp;
use crate::ui::browse_folders::{BrowseFolderRows, FolderRowsRequest, FolderTimes, NavigationHistory, Slot};
use crate::ui::browse_window::{RingWindow, ViewGeometry};
use crate::ui::selection::model::FileSelectionModel;
use crate::ui::selection::rubberband::RubberbandSession;
use crate::ui::sidebar::panel_tab_dnd::SidebarPanelTabDndSession;
use crate::ui::tabs::dnd::TabDndSession;
use crate::ui::tabs::grid::MasonryRect;
use crate::viewer::TabId;
use rustview_core::library::models::{FolderHierarchySnapshot, FolderId, MediaId, TagId};
use rustview_core::query::MediaStore;
use rustview_core::query::filters::{IndexedMediaRecord, PreparedFilter};
use rustview_core::query::{QueryFilter, SortColumn, SortOrder};
use rustview_core::settings::{
    DEFAULT_IS_INSPECTOR_COLLAPSED, DEFAULT_IS_TAG_HISTORY_COLLAPSED, DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB,
    DEFAULT_MAX_HISTORY_TAGS, DEFAULT_SORT_COLUMN, DEFAULT_SORT_ORDER, DEFAULT_TAB_GRID_AUTO_COLUMNS,
    DEFAULT_TAB_GRID_COLUMNS, DEFAULT_TAG_HISTORY_HEIGHT,
};
use rustview_core::telemetry::{FolderLoadBenchmark, ViewScrollBenchmark};
use rustview_core::thumbnails::ThumbnailRamCache;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
pub const CTX_FOLDER_NEW: CtxMenuActionId = 3009;
// Menus of the Browse view that belong to no single item: a mixed selection of folders and files, the free space.
pub const CTX_BROWSE_SELECTION_OPEN_IN_FILE_MANAGER: CtxMenuActionId = 3020;
pub const CTX_BROWSE_SELECTION_DELETE: CtxMenuActionId = 3021;
pub const CTX_BROWSE_TOGGLE_FOLDERS: CtxMenuActionId = 3022;
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

pub use rustview_core::query::RowMap;

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
    /// "Show Folders" is switched on in the Browse toolbar.
    pub show_folders: bool,
    /// The folder rows of the Browse view (empty unless `show_folders` and the view is a plain folder).
    pub folder_rows: BrowseFolderRows,
    /// Going up to this folder selects its first element once its list is shown (see `BrowseFolders::go_up`).
    pub pending_first_select: Option<FolderId>,
    /// Folders shown before the open one (Alt+Left) and after it (Alt+Right); see `NavigationHistory`.
    pub nav_history: NavigationHistory,
    /// Created / modified time of folders, read off the UI thread.
    pub folder_times: Arc<FolderTimes>,
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
    pub media_row_map: RowMap,
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
            show_folders: false,
            folder_rows: BrowseFolderRows::default(),
            pending_first_select: None,
            nav_history: NavigationHistory::default(),
            folder_times: FolderTimes::new(),
            list_total_bytes: AtomicU64::new(LIST_BYTES_DIRTY),
            file_selection: FileSelectionModel::new(),
            last_active_panel: LastActivePanel::Media,
            active_tab_media_ids: Vec::new(),
            rubberband_session: RubberbandSession::default(),
            active_tab_dnd_session: None,
            active_sidebar_panel_tab: DEFAULT_LEFT_SIDEBAR_ACTIVE_PANEL_TAB.to_string(),
            active_sidebar_panel_tab_dnd_session: None,
            is_inspector_collapsed: DEFAULT_IS_INSPECTOR_COLLAPSED,
            media_row_map: RowMap::default(),
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

    /// Stores the total size of the listed items when the code that built the list already summed it.
    pub fn set_visible_total_bytes(&self, total: u64) {
        self.list_total_bytes
            .store(total.min(LIST_BYTES_DIRTY - 1), Ordering::Relaxed);
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
        self.list_total_bytes
            .store(total.min(LIST_BYTES_DIRTY - 1), Ordering::Relaxed);
        total
    }

    /// The folder view is a plain folder: no query, no recursive view.
    #[inline]
    pub fn is_plain_folder_view(&self) -> bool {
        self.current_folder_id.is_some() && !self.is_virtual_folder_mode && !self.is_query_active
    }

    /// Recomputes the folder rows from the snapshot (memory only). Returns whether they changed.
    pub fn rebuild_folder_rows(
        &mut self,
        snapshot: &FolderHierarchySnapshot,
        show_hidden: bool,
        show_deleted: bool,
    ) -> bool {
        let request = FolderRowsRequest {
            enabled: self.show_folders && self.is_plain_folder_view(),
            current: self.current_folder_id,
            show_hidden,
            show_deleted,
        };
        // The filter field filters the folders by name as well (a regular expression when the regex toggle is on).
        let name_filter = PreparedFilter::from_filter(&self.filter);
        self.folder_rows
            .rebuild(snapshot, request, &|name| name_filter.matches_name(name))
    }

    /// The highlighted folders of the Browse view while that view has the focus, in list order (F2 and Del act on them, like on the
    /// selected folder of the tree). The "..." entry is no folder.
    pub fn focused_browse_folders(&self) -> Vec<FolderId> {
        if self.last_active_panel != LastActivePanel::Media {
            return Vec::new();
        }
        self.folder_rows.selected_folders_ordered()
    }

    /// Rows in front of the media (folders and "Up"): the media positions of the list start after them.
    #[inline]
    pub fn folder_front_len(&self) -> usize {
        self.folder_rows.front_len()
    }

    /// Length of the list the grid and the table show: folders, media and the folders created meanwhile.
    #[inline]
    pub fn view_len(&self) -> usize {
        self.folder_rows.view_len(self.active_indices.len())
    }

    /// What the tile at list `position` shows.
    #[inline]
    pub fn slot_at(&self, position: usize) -> Slot {
        self.folder_rows.slot_at(position, self.active_indices.len())
    }

    /// The list position of a medium, folder rows in front included (O(1)).
    #[inline]
    pub fn view_position_of(&self, media_id: MediaId) -> Option<usize> {
        self.media_row_map
            .get(&media_id)
            .map(|&row| row + self.folder_front_len())
    }

    /// The record at `position` of the sorted, filtered media list (the folder rows in front are not counted, see `slot_at`).
    #[inline]
    pub fn item_at(&self, position: usize) -> Option<&IndexedMediaRecord> {
        if self.active_indices.is_empty() {
            self.all_folder_media.get(position)
        } else {
            self.active_indices
                .get(position)
                .and_then(|&index| self.all_folder_media.get(index))
        }
    }

    /// The record of a media item that is part of the list the view shows (O(1)); hidden items yield `None`.
    #[inline]
    pub fn record_by_id(&self, media_id: MediaId) -> Option<&IndexedMediaRecord> {
        self.item_at(*self.media_row_map.get(&media_id)?)
    }

    /// The list position of a media item (O(1)); `None` for an item the view does not show.
    #[inline]
    pub fn row_of(&self, media_id: MediaId) -> Option<usize> {
        match self.media_row_map.get(&media_id) {
            Some(&row) => Some(row),
            None if self.active_indices.is_empty() => self.all_folder_media.position_of(media_id),
            None => None,
        }
    }

    /// The selected records in list order; costs O(selection), not O(list).
    pub fn get_selected_media_ordered(&self) -> Vec<&IndexedMediaRecord> {
        let selected_ids = self.file_selection.selected_ids();
        if selected_ids.is_empty() || self.active_indices.is_empty() {
            return Vec::new();
        }

        let mut rows: Vec<(usize, &IndexedMediaRecord)> = selected_ids
            .iter()
            .filter_map(|id| {
                let row = *self.media_row_map.get(id)?;
                Some((row, self.item_at(row)?))
            })
            .collect();
        rows.sort_unstable_by_key(|&(row, _)| row);
        rows.into_iter().map(|(_, item)| item).collect()
    }

    pub fn get_selected_paths_ordered(&self) -> Vec<PathBuf> {
        self.get_selected_media_ordered()
            .into_iter()
            .map(rustview_core::query::IndexedMediaRecord::full_path)
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
        (!self.active_tab_media_ids.is_empty()).then(|| self.active_tab_media_ids.iter().copied().collect())
    }

    #[inline]
    pub fn is_query_resettable(&self) -> bool {
        !self.is_assign_mode && (self.selected_rating > 0 || !self.filter.tag_ids.is_empty() || self.is_query_active)
    }

    /// Drops deleted media from the open folder. The positions the previous layout stored are void afterwards, so the next
    /// reload must lay the list out from scratch.
    pub fn remove_media(&mut self, ids: &HashSet<MediaId>) {
        self.all_folder_media.retain(|m| !ids.contains(&m.record.id));
        self.file_selection.clear();
        self.active_indices.clear();
        self.last_executed_query.clear();
        self.mark_list_bytes_dirty();
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
