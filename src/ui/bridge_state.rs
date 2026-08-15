use crate::ctx_menu::CtxMenuActionId;
use crate::library::models::{FolderId, MediaId};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{QueryFilter, SortColumn, SortOrder};
use crate::ui::file_selection::FileSelectionModel;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::path::PathBuf;

// Standardized Context Menu Action IDs
pub const CTX_ACTION_RENAME: CtxMenuActionId = 1001;
pub const CTX_ACTION_SELECT_ALL: CtxMenuActionId = 1002;
pub const CTX_ACTION_CLEAR_SELECTION: CtxMenuActionId = 1003;
pub const CTX_ACTION_RATE_5: CtxMenuActionId = 1015;
pub const CTX_ACTION_RATE_4: CtxMenuActionId = 1014;
pub const CTX_ACTION_RATE_3: CtxMenuActionId = 1013;
pub const CTX_ACTION_RATE_2: CtxMenuActionId = 1012;
pub const CTX_ACTION_RATE_1: CtxMenuActionId = 1011;
pub const CTX_ACTION_RATE_0: CtxMenuActionId = 1010;

/// Central persistent state for UI bridges and caching.
pub struct UiState {
    pub current_folder_id: Option<FolderId>,
    pub current_folder_path: Option<PathBuf>,
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
    pub active_file_rename_target: Option<MediaId>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            current_folder_id: None,
            current_folder_path: None,
            all_folder_media: Vec::new(),
            folder_thumbnails: HashMap::new(),
            active_indices: Vec::new(),
            file_selection: FileSelectionModel::new(),
            media_row_map: HashMap::new(),
            filter: QueryFilter::default(),
            last_executed_query: String::new(),
            sort_column: SortColumn::Filename,
            sort_order: SortOrder::Ascending,
            is_assign_mode: false,
            active_file_rename_target: None,
        }
    }
}
