slint::include_modules!();

pub mod bridge;
pub mod bridge_ctx_menu;
pub mod bridge_ctx_menu_favorite;
pub mod bridge_ctx_menu_folder;
pub mod bridge_ctx_menu_media;
pub mod bridge_ctx_menu_presenter;
pub mod bridge_ctx_menu_tab;
pub mod bridge_ctx_menu_tag;
pub mod bridge_dnd;
pub mod bridge_favorite_dialog_rename;
pub mod bridge_favorites;
pub mod bridge_file_dialog_conflict;
pub mod bridge_file_dialog_delete;
pub mod bridge_file_dialog_missing;
pub mod bridge_file_dialog_remove_orphaned;
pub mod bridge_file_dialogs;
pub mod bridge_file_selection;
pub mod bridge_folder_dialog_delete;
pub mod bridge_folder_dialog_missing_delete;
pub mod bridge_folder_dialog_move;
pub mod bridge_folder_dialog_reborn_warn;
pub mod bridge_folder_dialog_remove_orphaned;
pub mod bridge_folder_dialog_rename;
pub mod bridge_folder_dialogs;
pub mod bridge_folder_move_sync;
pub mod bridge_folder_tree_sync;
pub mod bridge_folders;
pub mod bridge_inspector;
pub mod bridge_media;
pub mod bridge_rubberband;
pub mod bridge_sidebar_panel_tabs;
pub mod bridge_state;
pub mod bridge_status_bar;
pub mod bridge_tab_grid;
pub mod bridge_tag_dialogs;
pub mod bridge_tag_history;
pub mod bridge_tags;
pub mod bridge_thumbnails;
pub mod bridge_viewer_tabs;
pub mod browse_window;
pub mod button_feedback;
pub mod button_feedback_bridge;
pub mod color_picker;
pub mod dnd_foldertree_autoscroller;
pub mod fallback_view;
pub mod favorites;
pub mod file_selection;
pub mod keyboard_navigation;
pub mod models;
pub mod multiview;
pub mod perf_probe;
pub mod rubberband_selection;
pub mod shutdown;
pub mod sidebar_panel_tab_dnd;
pub mod tab_dnd;
pub mod tab_grid;
pub mod tab_icon;
pub mod tag_history;
pub mod thumbnail_badges;
pub mod thumbnail_regeneration;
pub mod video_preview_animator;
pub mod virtual_folder_wording;

pub use bridge::{RenameActionParams, UiBridge};
pub use bridge_ctx_menu::{FolderMenuContext, MediaMenuContext};
pub use bridge_ctx_menu_favorite::FavoriteContextMenu;
pub use bridge_ctx_menu_folder::FolderContextMenu;
pub use bridge_ctx_menu_media::MediaContextMenu;
pub use bridge_ctx_menu_presenter::CtxMenuPresenter;
pub use bridge_ctx_menu_tab::{TabActionContext, TabContextMenu};
pub use bridge_ctx_menu_tag::TagContextMenu;
pub use bridge_dnd::{BatchTransferParams, BridgeDnd, DragStartParams};
pub use bridge_favorite_dialog_rename::FavoriteRenameDialogController;
pub use bridge_favorites::BridgeFavorites;
pub use bridge_file_dialog_conflict::{
    ConflictDialogParams, ConflictThumbInfo, FileOpsConflictDialogController,
};
pub use bridge_file_dialog_delete::DeleteFileDialogController;
pub use bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
pub use bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
pub use bridge_file_dialogs::{FileRenameContext, FileRenameDialogController, RenameDialogParams};
pub use bridge_file_selection::{
    BridgeFileSelection, FileClickContext, FileClickEvent, LastClickRecord,
};
pub use bridge_folder_dialog_delete::DeleteFolderDialogController;
pub use bridge_folder_dialog_missing_delete::MissingFolderDeleteDialogController;
pub use bridge_folder_dialog_move::MoveFolderDialogController;
pub use bridge_folder_dialog_reborn_warn::RebornFolderWarnDialogController;
pub use bridge_folder_dialog_remove_orphaned::RemoveOrphanedFolderDialogController;
pub use bridge_folder_dialog_rename::RenameFolderDialogController;
pub use bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
pub use bridge_folder_move_sync::BridgeFolderMoveSync;
pub use bridge_folder_tree_sync::FolderTreeSync;
pub use multiview::MultiviewHandle;
pub use bridge_rubberband::{
    BridgeRubberband, RubberbandDragMoveParams, RubberbandDragStartParams,
};
pub use bridge_sidebar_panel_tabs::BridgeSidebarPanelTabs;
pub use bridge_state::*;
pub use bridge_status_bar::{StatusBarBridge, StatusMessageLevel};
pub use bridge_tab_grid::BridgeTabGrid;
pub use bridge_tag_dialogs::TagDialogController;
pub use bridge_tag_history::BridgeTagHistory;
pub use bridge_tags::{BridgeTags, TagActionContext};
pub use bridge_viewer_tabs::{BridgeViewerTabs, TabDragEndParams};
pub use dnd_foldertree_autoscroller::{DndFolderTreeAutoScroller, FolderTreeAutoScrollDirection};
pub use fallback_view::{FallbackEvaluationParams, FallbackViewEvaluator, FallbackViewState};
pub use favorites::{FavoriteEntry, FavoriteManager};
pub use file_selection::{FileSelectionModel, FileSelectionSummary};
pub use keyboard_navigation::{
    calculate_target_index, handle_keyboard_navigate, KeyboardNavigateParams, NavigationDirection,
};
pub use thumbnail_badges::ThumbnailBadgeBridge;
pub use rubberband_selection::{
    GridLayoutConfig, RectF32, RubberbandAutoScroller, RubberbandGeometry, RubberbandSession,
    RubberbandStartParams, TableLayoutConfig,
};
pub use sidebar_panel_tab_dnd::{
    SidebarPanelTabDndGeometry, SidebarPanelTabDndSession, SidebarPanelTabSlotInfo,
};
pub use tab_dnd::{
    TabAutoScrollDirection, TabDndAutoScroller, TabDndGeometry, TabDndSession, TabSlotInfo,
};
pub use tab_grid::{MasonryRect, ResponsiveBreakpoints, TabGridMasonryEngine, TabGridTile};
pub use tag_history::{
    TagHistoryItem, TagHistoryLayoutEngine, TagHistoryManager, TagHistoryRow,
};
pub use video_preview_animator::{VideoPreviewAnimator, DEFAULT_PREVIEW_INTERVAL_MS};
