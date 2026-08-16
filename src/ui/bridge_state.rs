use crate::ctx_menu::{CtxMenu, CtxMenuActionId};
use crate::library::models::{FolderId, MediaId, TagId};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{QueryFilter, SortColumn, SortOrder};
use crate::settings::{DEFAULT_SORT_COLUMN, DEFAULT_SORT_ORDER};
use crate::ui::file_selection::FileSelectionModel;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

// Standard File Context Menu Action IDs
pub const CTX_ACTION_RENAME: CtxMenuActionId = 1001;
pub const CTX_ACTION_SELECT_ALL: CtxMenuActionId = 1002;
pub const CTX_ACTION_CLEAR_SELECTION: CtxMenuActionId = 1003;
pub const CTX_ACTION_REFRESH_FILES: CtxMenuActionId = 1004;

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
pub const CTX_FOLDER_RENAME: CtxMenuActionId = 3001;
pub const CTX_FOLDER_DELETE: CtxMenuActionId = 3002;
pub const CTX_FOLDER_PURGE_DB: CtxMenuActionId = 3003;
pub const CTX_FOLDER_MOVE_BASE: CtxMenuActionId = 300_000;

/// Central persistent state for UI bridges, tree expansions, and caching.
pub struct UiState {
    pub current_folder_id: Option<FolderId>,
    pub current_folder_path: Option<PathBuf>,
    pub active_folder_token: usize,
    pub active_folder_cancel_flag: Option<Arc<AtomicBool>>,
    pub all_folder_media: Vec<IndexedMediaRecord>,
    pub folder_thumbnails: HashMap<MediaId, SharedPixelBuffer<Rgb8Pixel>>,
    pub active_indices: Vec<usize>,
    pub file_selection: FileSelectionModel,
    pub media_row_map: HashMap<MediaId, usize>,
    pub filter: QueryFilter,
    pub last_executed_query: String,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    pub is_assign_mode: bool,
    pub is_refreshing_media: bool,
    pub active_file_rename_target: Option<MediaId>,

    // === Hierarchical Folder Tree State ===
    pub expanded_folder_ids: HashSet<FolderId>,
    pub is_refreshing_folders: bool,
    pub folder_scan_count: usize,
    pub active_folder_dialog_target: Option<FolderId>,

    // === Hierarchical Tag Tree State ===
    pub expanded_tag_ids: HashSet<TagId>,
    pub tag_filter_query: String,
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
            all_folder_media: Vec::new(),
            folder_thumbnails: HashMap::new(),
            active_indices: Vec::new(),
            file_selection: FileSelectionModel::new(),
            media_row_map: HashMap::new(),
            filter: QueryFilter::default(),
            last_executed_query: String::new(),
            sort_column: SortColumn::from_name(DEFAULT_SORT_COLUMN),
            sort_order: SortOrder::from_name(DEFAULT_SORT_ORDER),
            is_assign_mode: false,
            is_refreshing_media: false,
            active_file_rename_target: None,
            expanded_folder_ids: HashSet::new(),
            is_refreshing_folders: false,
            folder_scan_count: 0,
            active_folder_dialog_target: None,
            expanded_tag_ids: HashSet::new(),
            tag_filter_query: String::new(),
            active_tag_dialog_target: None,
            active_context_menu: None,
        }
    }
}
