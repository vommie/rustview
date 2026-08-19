use crate::ctx_menu::{CtxMenu, CtxMenuActionId};
use crate::library::models::{FolderId, MediaId, TagId};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{QueryFilter, SortColumn, SortOrder};
use crate::settings::{DEFAULT_SORT_COLUMN, DEFAULT_SORT_ORDER};
use crate::system::DesktopApp;
use crate::telemetry::{FolderLoadBenchmark, ViewScrollBenchmark};
use crate::ui::bridge_folders::FolderTreeNode;
use crate::ui::file_selection::FileSelectionModel;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

// Standard File Context Menu Action IDs
pub const CTX_ACTION_OPEN: CtxMenuActionId = 1000;
pub const CTX_ACTION_OPEN_IN_FILE_MANAGER: CtxMenuActionId = 1008;
pub const CTX_ACTION_OPEN_WITH_OTHER: CtxMenuActionId = 1050;
pub const CTX_ACTION_OPEN_WITH_APP_BASE: CtxMenuActionId = 10_000;

pub const CTX_ACTION_RENAME: CtxMenuActionId = 1001;
pub const CTX_ACTION_SELECT_ALL: CtxMenuActionId = 1002;
pub const CTX_ACTION_CLEAR_SELECTION: CtxMenuActionId = 1003;
pub const CTX_ACTION_REFRESH_FILES: CtxMenuActionId = 1004;

pub const CTX_ACTION_COPY_NAME: CtxMenuActionId = 1005;
pub const CTX_ACTION_COPY_PATH: CtxMenuActionId = 1006;
pub const CTX_ACTION_COPY_IMAGE: CtxMenuActionId = 1007;

pub const CTX_ACTION_RATE_5: CtxMenuActionId = 1015;
pub const CTX_ACTION_RATE_4: CtxMenuActionId = 1014;
pub const CTX_ACTION_RATE_3: CtxMenuActionId = 1013;
pub const CTX_ACTION_RATE_2: CtxMenuActionId = 1012;
pub const CTX_ACTION_RATE_1: CtxMenuActionId = 1011;
pub const CTX_ACTION_RATE_0: CtxMenuActionId = 1010;

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
pub const CTX_FOLDER_MOVE_BASE: CtxMenuActionId = 300_000;

/// High-performance LRU-managed bounded in-memory thumbnail store preventing RAM bloat on 100k+ datasets.
#[derive(Debug, Clone)]
pub struct LruThumbnailStore {
    capacity: usize,
    map: HashMap<MediaId, SharedPixelBuffer<Rgb8Pixel>>,
    order: VecDeque<MediaId>,
}

impl Default for LruThumbnailStore {
    fn default() -> Self {
        Self::new(2048)
    }
}

impl LruThumbnailStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(128),
            map: HashMap::with_capacity(capacity.min(2048)),
            order: VecDeque::with_capacity(capacity.min(2048)),
        }
    }

    /// Inserts a thumbnail and returns the evicted MediaId if capacity was exceeded.
    #[inline]
    pub fn insert(&mut self, id: MediaId, buffer: SharedPixelBuffer<Rgb8Pixel>) -> Option<MediaId> {
        let mut evicted = None;
        if self.map.len() >= self.capacity
            && !self.map.contains_key(&id)
            && let Some(evicted_id) = self.order.pop_front()
        {
            self.map.remove(&evicted_id);
            evicted = Some(evicted_id);
        }
        self.map.insert(id, buffer);
        self.order.retain(|&i| i != id);
        self.order.push_back(id);
        evicted
    }

    #[inline]
    pub fn get(&self, id: &MediaId) -> Option<&SharedPixelBuffer<Rgb8Pixel>> {
        self.map.get(id)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Central persistent state for UI bridges, tree expansions, virtualization windowing, and caching.
pub struct UiState {
    pub current_folder_id: Option<FolderId>,
    pub current_folder_path: Option<PathBuf>,
    pub active_folder_token: usize,
    pub active_folder_cancel_flag: Option<Arc<AtomicBool>>,
    pub active_folder_bench: Option<Arc<FolderLoadBenchmark>>,
    pub scroll_bench: Arc<ViewScrollBenchmark>,
    pub all_folder_media: Vec<IndexedMediaRecord>,
    pub folder_thumbnails: LruThumbnailStore,
    pub active_indices: Vec<usize>,
    pub file_selection: FileSelectionModel,
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

    // === Viewport Windowing & Hysteresis Paging State ===
    pub loaded_slice_start: usize,
    pub loaded_slice_len: usize,
    pub loaded_slice_valid: bool,

    // === Hierarchical Folder Tree State & Fast In-Memory Cache ===
    pub expanded_folder_ids: HashSet<FolderId>,
    pub is_refreshing_folders: bool,
    pub folder_scan_count: usize,
    pub active_folder_dialog_target: Option<FolderId>,
    pub cached_folder_tree: Option<Vec<FolderTreeNode>>,
    pub cached_media_counts: HashMap<FolderId, usize>,

    // === Hierarchical Tag Tree State ===
    pub expanded_tag_ids: HashSet<TagId>,
    pub tag_filter_query: String,
    pub tag_filter_is_regex: bool,
    pub active_tag_dialog_target: Option<TagId>,
    pub active_context_menu: Option<CtxMenu>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            current_folder_id: None,
            current_folder_path: None,
            active_folder_token: 0,
            active_folder_cancel_flag: None,
            active_folder_bench: None,
            scroll_bench: Arc::new(ViewScrollBenchmark::new(false)),
            all_folder_media: Vec::new(),
            folder_thumbnails: LruThumbnailStore::default(),
            active_indices: Vec::new(),
            file_selection: FileSelectionModel::new(),
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
            loaded_slice_start: 0,
            loaded_slice_len: 0,
            loaded_slice_valid: false,
            expanded_folder_ids: HashSet::new(),
            is_refreshing_folders: false,
            folder_scan_count: 0,
            active_folder_dialog_target: None,
            cached_folder_tree: None,
            cached_media_counts: HashMap::new(),
            expanded_tag_ids: HashSet::new(),
            tag_filter_query: String::new(),
            tag_filter_is_regex: false,
            active_tag_dialog_target: None,
            active_context_menu: None,
        }
    }
}

impl UiState {
    /// Single Point of Truth determining whether there is an active query/filter state that can be reset.
    #[inline]
    pub fn is_query_resettable(&self) -> bool {
        !self.is_assign_mode
            && (self.selected_rating > 0
                || !self.filter.tag_ids.is_empty()
                || self.is_query_active)
    }

    /// Invalidates the loaded slice window forcing an immediate refresh on the next chunk update.
    #[inline]
    pub fn invalidate_loaded_slice(&mut self) {
        self.loaded_slice_valid = false;
        self.loaded_slice_start = 0;
        self.loaded_slice_len = 0;
    }

    /// Invalidates the in-memory cached FolderTree hierarchy forcing a fresh topological build.
    #[inline]
    pub fn invalidate_folder_tree_cache(&mut self) {
        self.cached_folder_tree = None;
        self.cached_media_counts.clear();
    }

    /// Updates media count of a single folder in-place without rebuilding the entire hierarchy.
    pub fn update_folder_media_count_in_cache(&mut self, folder_id: FolderId, count: usize) {
        self.cached_media_counts.insert(folder_id, count);
        if let Some(ref mut tree) = self.cached_folder_tree {
            Self::update_tree_node_count(tree, folder_id, count);
        }
    }

    fn update_tree_node_count(nodes: &mut [FolderTreeNode], folder_id: FolderId, count: usize) -> bool {
        for node in nodes.iter_mut() {
            if node.record.id == folder_id {
                node.media_files_count = count;
                return true;
            }
            if Self::update_tree_node_count(&mut node.children, folder_id, count) {
                return true;
            }
        }
        false
    }
}
