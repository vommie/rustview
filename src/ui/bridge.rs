use super::multiview::handle::{AppContext, KEEP_VIEW};
use super::bridge_ctx_menu::{
    BridgeContextMenu, ContextMenuRegistration, MediaMenuContext,
};
use super::bridge_dnd::{BatchTransferParams, BridgeDnd, DragStartParams};
use super::bridge_favorite_dialog_rename::FavoriteRenameDialogController;
use super::bridge_favorites::BridgeFavorites;
use super::bridge_file_dialog_conflict::FileOpsConflictDialogController;
use super::bridge_file_dialog_delete::{DeleteFileDialogController, DeleteFollowup};
use super::bridge_file_dialog_missing::MissingFileDialogController;
use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
use super::bridge_file_dialogs::{FileDialogContext, FileRenameDialogController, RenameDialogParams};
use super::bridge_file_selection::{BridgeFileSelection, FileClickContext, LastClickRecord};
use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::browse_folders::{BrowseDragScroller, BrowseFolders};
use super::bridge_folders::BridgeFolders;
use super::multiview::{self, MultiviewHandle, bar_overlay};
use crate::viewer::multiview::prefs;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_rubberband::{BridgeRubberband, RubberbandDragMoveParams, RubberbandDragStartParams};
use super::bridge_sidebar_panel_tabs::BridgeSidebarPanelTabs;
use super::bridge_state::*;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tab_grid::BridgeTabGrid;
use super::bridge_tag_dialogs::TagDialogController;
use super::bridge_tag_history::BridgeTagHistory;
use super::bridge_tags::{BridgeTags, TagActionContext};
use super::bridge_thumbnails::BridgeThumbnails;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::dnd_foldertree_autoscroller::DndFolderTreeAutoScroller;
use super::favorites::FavoriteManager;
use super::formats_policy::{FormatPolicyApplier, FormatPolicyContext};
use super::keyboard_navigation::{handle_keyboard_navigate, KeyboardNavigateParams};
use super::shutdown::ShutdownTargets;
use super::multiview::tab_host;
use super::rubberband_selection::RubberbandAutoScroller;
use super::thumbnail_badges::ThumbnailBadgeBridge;
use super::type_ahead::{self, TypeAheadParams, TypeAheadState, handle_type_ahead_key};
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::CtxMenuDispatcher;
use crate::library::folders::normalize_path;
use crate::library::models::{FileTransferKind, FolderId, MediaId, TagId};
use crate::library::Library;
use crate::query::{MediaTypeFilter, ScopeMode, SortColumn, SortOrder};
use crate::settings::memory_budget::{BYTES_PER_MB, MemoryPlan, SystemMemory};
use crate::settings::{
    AppSettings, GridAspectRatio, LogObscurer, SettingsDialogController, SettingsDiff,
    SettingsManager,
};
use crate::thumbnails::{ThumbnailConfig, ThumbnailManager};
use crate::viewer::ViewerTabManager;
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub struct RenameActionParams<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub rename_dialog: &'a Rc<RefCell<Option<FileRenameDialog>>>,
    pub favorite_rename_dialog: &'a FavoriteRenameDialogController,
    pub favorites: &'a Arc<RwLock<FavoriteManager>>,
    pub missing_dialog: &'a MissingFileDialogController,
    pub conflict_dialog: &'a FileOpsConflictDialogController,
    pub folder_dialogs: &'a FolderDialogController,
    pub fullscreen: &'a MultiviewHandle,
}

pub struct DeleteActionParams<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub favorites: &'a Arc<RwLock<FavoriteManager>>,
    pub delete_file_dialog: &'a DeleteFileDialogController,
    pub remove_orphaned_file_dialog: &'a RemoveOrphanedFileDialogController,
    pub folder_dialogs: &'a FolderDialogController,
    pub fullscreen: &'a MultiviewHandle,
}

pub struct UiBridge {
    window: MainWindow,
    library: Arc<Library>,
    thumbnails: Arc<ThumbnailManager>,
    tab_manager: Arc<ViewerTabManager>,
    settings: Arc<SettingsManager>,
    settings_dialog: Rc<RefCell<SettingsDialogController>>,
    fullscreen: MultiviewHandle,
    rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    favorite_rename_dialog: FavoriteRenameDialogController,
    missing_file_dialog: MissingFileDialogController,
    delete_file_dialog: DeleteFileDialogController,
    remove_orphaned_dialog: RemoveOrphanedFileDialogController,
    conflict_dialog: FileOpsConflictDialogController,
    dnd_foldertree_autoscroller: DndFolderTreeAutoScroller,
    dnd_browse_scroller: BrowseDragScroller,
    rubberband_autoscroller: RubberbandAutoScroller,
    tag_dialogs: TagDialogController,
    folder_dialogs: FolderDialogController,
    favorites: Arc<RwLock<FavoriteManager>>,
    ctx_menu_dispatcher: Arc<CtxMenuDispatcher>,
    state: Arc<RwLock<UiState>>,
    last_click: Arc<Mutex<Option<LastClickRecord>>>,
    _thumbnail_timer: Rc<Timer>,
}

impl UiBridge {
    pub fn new(
        window: MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        config_dir: &Path,
    ) -> Self {
        let favorites = Arc::new(RwLock::new(FavoriteManager::new(config_dir)));
        let tab_manager = Arc::new(ViewerTabManager::new(library.clone()));

        let mut controller = SettingsDialogController::new(settings.clone());
        controller.set_main_window(window.as_weak());
        let counting_library = library.clone();
        controller.set_format_counter(Arc::new(move || {
            counting_library.media().count_by_format().unwrap_or_default()
        }));
        let settings_dialog = Rc::new(RefCell::new(controller));
        let fullscreen = MultiviewHandle::default();

        let rename_dialog = Rc::new(RefCell::new(None));
        let favorite_rename_dialog = FavoriteRenameDialogController::new();
        let missing_file_dialog = MissingFileDialogController::new();
        let delete_file_dialog = DeleteFileDialogController::new();
        let remove_orphaned_dialog = RemoveOrphanedFileDialogController::new();
        let conflict_dialog = FileOpsConflictDialogController::new();
        let dnd_foldertree_autoscroller = DndFolderTreeAutoScroller::new();
        let dnd_browse_scroller = BrowseDragScroller::new();
        let rubberband_autoscroller = RubberbandAutoScroller::new();
        let tag_dialogs = TagDialogController::new();
        let folder_dialogs = FolderDialogController::new();

        let mut initial_state = UiState::default();
        initial_state.thumbs = thumbnails.cache_handle();
        let restored_expanded_tags = settings.get().tags.expanded_tag_ids;
        initial_state.expanded_tag_ids = restored_expanded_tags.into_iter().collect();

        let restored_expanded_folders = if settings.get().folders.persist_folder_tree_state {
            settings.get().folders.expanded_folder_ids
        } else {
            Vec::new()
        };
        initial_state.expanded_folder_ids = restored_expanded_folders.into_iter().collect();

        initial_state.tag_history_ids = settings.get().tags.history_tag_ids.clone();
        initial_state.max_history_tags = settings.get().tags.max_history_tags;
        initial_state.is_tag_history_collapsed = settings.get().tags.is_history_collapsed;
        initial_state.tag_history_height = settings.get().tags.history_height;

        let v_settings = settings.get().view;
        window.set_is_table_view(v_settings.is_table_view);

        window.set_sidebar_left_width(v_settings.sidebar_left_width);
        window.set_sidebar_right_width(v_settings.sidebar_right_width);

        let initial_sort_col = SortColumn::from_name(&v_settings.sort_column);
        let initial_sort_ord = SortOrder::from_name(&v_settings.sort_order);
        initial_state.sort_column = initial_sort_col;
        initial_state.sort_order = initial_sort_ord;

        window.set_sort_column_idx(initial_sort_col.to_index());
        window.set_sort_order_idx(initial_sort_ord.to_index());

        let initial_media_type_filter = MediaTypeFilter::from_str_name(&v_settings.media_type_filter);
        initial_state.filter.media_type_filter = initial_media_type_filter;
        window.set_media_type_filter_idx(initial_media_type_filter.to_index());
        initial_state.show_folders = v_settings.show_folders_in_browse;
        window.set_show_folders(v_settings.show_folders_in_browse);

        initial_state.tab_grid_columns = v_settings.tab_grid_columns;
        initial_state.tab_grid_auto_columns = v_settings.tab_grid_auto_columns;
        window.set_tab_grid_columns(v_settings.tab_grid_columns as i32);
        window.set_tab_grid_auto_columns(v_settings.tab_grid_auto_columns);

        initial_state.is_inspector_collapsed = v_settings.is_inspector_collapsed;
        window.set_info_is_collapsed(v_settings.is_inspector_collapsed);

        window.set_col_filename_width(v_settings.col_filename_width);
        window.set_col_path_width(v_settings.col_path_width);
        window.set_col_modified_date_width(v_settings.col_modified_date_width);
        window.set_col_created_date_width(v_settings.col_created_date_width);
        window.set_col_size_width(v_settings.col_size_width);
        window.set_col_rating_width(v_settings.col_rating_width);
        window.set_col_tags_width(v_settings.col_tags_width);
        window.set_col_width_width(v_settings.col_width_width);
        window.set_col_height_width(v_settings.col_height_width);
        window.set_col_ar_width(v_settings.col_ar_width);
        window.set_col_duration_width(v_settings.col_duration_width);
        window.set_col_format_width(v_settings.col_format_width);
        window.set_col_flags_width(v_settings.col_flags_width);

        let app_settings = settings.get();
        window.set_thumb_size(app_settings.appearance.default_thumbnail_size as i32);
        let ratio = GridAspectRatio::from_str_name(&app_settings.appearance.thumbnail_aspect_ratio);
        window.set_thumb_aspect_idx(ratio.to_index());
        ThumbnailBadgeBridge::register(&window);
        ThumbnailBadgeBridge::apply_config(&window, &app_settings.appearance);
        window.set_grid_item_padding_h(app_settings.appearance.grid_item_padding_horizontal as i32);
        window.set_grid_item_padding_v(app_settings.appearance.grid_item_padding_vertical as i32);
        window.set_grid_spacing(app_settings.appearance.grid_spacing as i32);
        window.set_grid_padding(app_settings.appearance.grid_padding as i32);
        window.set_grid_item_border_width(app_settings.appearance.grid_item_border_width as i32);
        window.set_grid_item_corner_radius(app_settings.appearance.grid_item_corner_radius as i32);
        window.set_enable_tooltips(app_settings.appearance.enable_tooltips);
        window.set_tooltip_delay_ms(app_settings.appearance.tooltip_delay_ms as i32);
        bar_overlay::set_tooltip_settings(
            app_settings.appearance.enable_tooltips,
            app_settings.appearance.tooltip_delay_ms as i32,
        );
        type_ahead::config::apply(&app_settings.appearance);
        prefs::apply(&app_settings.fullscreen);
        window.set_show_hidden_folders(app_settings.folders.show_hidden_folders);

        window.set_tag_history_height(initial_state.tag_history_height);
        window.set_tag_history_is_collapsed(initial_state.is_tag_history_collapsed);

        StatusBarBridge::info(&window, "Initializing RustView...");

        window.set_is_browse_active(true);
        window.set_is_tab_grid_active(false);
        window.set_active_view_mode(0);
        window.set_tabs(ModelRc::new(VecModel::from(vec![])));
        window.set_viewer_show_tab_rating_badge(app_settings.viewer.tabs.show_rating_badge);

        let initial_win_sz = window.window().size();
        if let Some(main_xid) = crate::viewer::get_slint_window_xid(window.window()) {
            tab_manager.set_main_viewport_info(main_xid, 0, 64, initial_win_sz.width.max(800), initial_win_sz.height.max(600));
        }

        window.set_is_virtual_folder(false);
        window.set_is_virtual_folder_btn_enabled(false);
        window.set_virtual_folder_count(0);

        window.set_selected_rating(0);
        window.set_tag_is_assign_mode(false);
        window.set_tag_scope_mode_idx(ScopeMode::default().to_index());
        window.set_has_file_selection(true);
        window.set_has_active_query(false);
        window.set_partial_ratings(ModelRc::new(VecModel::from(vec![false; 6])));
        window.set_tag_filter_text("".into());
        window.set_tag_filter_is_regex(false);

        let state = Arc::new(RwLock::new(initial_state));
        let ctx_menu_dispatcher = Arc::new(CtxMenuDispatcher::new());
        let last_click = Arc::new(Mutex::new(None));

        BridgeSidebarPanelTabs::init(&window, &settings, &state);

        fullscreen.bind(
            AppContext {
                tab_manager: tab_manager.clone(),
                library: library.clone(),
                thumbnails: thumbnails.clone(),
                settings: settings.clone(),
                state: state.clone(),
            },
            window.as_weak(),
        );
        tab_host::bind(&window, fullscreen.clone());
        super::multiview::tab_plain::bind(fullscreen.clone());

        let win_fs_close = window.as_weak();
        let tm_fs_close = tab_manager.clone();
        let lib_fs_close = library.clone();
        let set_fs_close = settings.clone();
        let st_fs_close = state.clone();
        let fs_controller_close = fullscreen.clone();

        fullscreen.set_close_handler(move |origin_mode, tab_id_opt| {
            let win_w = win_fs_close.clone();
            let tm = tm_fs_close.clone();
            let lib = lib_fs_close.clone();
            let set = set_fs_close.clone();
            let st = st_fs_close.clone();
            let fs_ctrl = fs_controller_close.clone();

            let _ = win_w.upgrade_in_event_loop(move |w| {
                if origin_mode == KEEP_VIEW {
                    // A Multiview that followed the Browse selection closed: the main window shows what it shows now, and
                    // the tab list too (the Multiview may have been moved into a new tab as it closed).
                    BridgeViewerTabs::sync_ui(&w, &tm, &lib, &set, &st);
                    if w.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(&w, &tm, &lib, &set, &st);
                    }
                    BridgeViewerTabs::sync_video_embedding(&w, &tm, &fs_ctrl, &set);
                    StatusBarBridge::update_metrics(&w, &st);
                    return;
                }
                w.set_is_modal_locked(false);
                w.set_active_view_mode(origin_mode);
                w.set_is_browse_active(origin_mode == 0);
                w.set_is_tab_grid_active(origin_mode == 2);

                if origin_mode == 2 {
                    tm.set_all_videos_view_active(false);
                    BridgeViewerTabs::sync_ui(&w, &tm, &lib, &set, &st);
                    BridgeTabGrid::sync_tab_grid_ui(&w, &tm, &lib, &set, &st);
                } else if origin_mode == 1 {
                    if let Some(tab_id) = tab_id_opt {
                        tm.set_active_tab_internal(tab_id, true);
                    }
                    BridgeViewerTabs::sync_ui(&w, &tm, &lib, &set, &st);
                    BridgeViewerTabs::sync_video_embedding(&w, &tm, &fs_ctrl, &set);
                } else {
                    tm.activate_browse();
                    BridgeViewerTabs::sync_ui(&w, &tm, &lib, &set, &st);
                    BridgeViewerTabs::sync_video_embedding(&w, &tm, &fs_ctrl, &set);
                }

                StatusBarBridge::update_metrics(&w, &st);
            });
        });

        Self::install_tabs_changed_hook(&window, &tab_manager, &library, &settings, &state);

        let thumbnail_timer = BridgeThumbnails::start_drain_timer(
            thumbnails.clone(),
            state.clone(),
            window.as_weak(),
        );

        let bridge = Self {
            window,
            library,
            thumbnails,
            tab_manager,
            settings,
            settings_dialog,
            fullscreen,
            rename_dialog,
            favorite_rename_dialog,
            missing_file_dialog,
            delete_file_dialog,
            remove_orphaned_dialog,
            conflict_dialog,
            dnd_foldertree_autoscroller,
            dnd_browse_scroller,
            rubberband_autoscroller,
            tag_dialogs,
            folder_dialogs,
            favorites,
            ctx_menu_dispatcher,
            state,
            last_click,
            _thumbnail_timer: thumbnail_timer,
        };

        bridge.register_subsystem_callbacks();
        bridge.register_settings_subscriber();

        BridgeFavorites::init(&bridge.window, &bridge.favorites);
        bridge.reload_tags();
        BridgeFolders::reload_tree(
            &bridge.window,
            &bridge.library,
            &bridge.settings,
            &bridge.state,
        );

        BridgeTagHistory::init(
            &bridge.window,
            &bridge.library,
            &bridge.settings,
            &bridge.state,
        );

        let w_weak = bridge.window.as_weak();
        let lib = bridge.library.clone();
        let thumb = bridge.thumbnails.clone();
        let set = bridge.settings.clone();
        let st = bridge.state.clone();
        let tm_post = bridge.tab_manager.clone();

        Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(win) = w_weak.upgrade() {
                if let Some(main_xid) = crate::viewer::get_slint_window_xid(win.window()) {
                    let sz = win.window().size();
                    tm_post.set_main_viewport_info(main_xid, 0, 64, sz.width.max(800), sz.height.max(600));
                }
                BridgeFolders::sync_root(&win, &lib, &thumb, &set, &st);
                BridgeMedia::update_virtual_folder_button_state(&win, &lib, &st);
                StatusBarBridge::update_metrics(&win, &st);
            }
        });

        bridge
    }

    /// Whatever creates a tab (a click, a menu, an action button, a Multiview moved into a tab), the tab list is rendered
    /// right away: after a tab was created the UI thread checks whether the tab UI is behind and, if so, renders it. The paths
    /// that render on their own have already marked the tab UI as current, so the check is free for them.
    fn install_tabs_changed_hook(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.as_weak();
        let tm_weak = Arc::downgrade(tab_manager);
        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        tab_manager.set_tabs_changed_hook(move || {
            let (tm_weak, lib, set, st) = (tm_weak.clone(), lib.clone(), set.clone(), st.clone());
            let _ = win.upgrade_in_event_loop(move |w| {
                let Some(tm) = tm_weak.upgrade() else {
                    return;
                };
                if !tm.is_tab_ui_stale() {
                    return;
                }
                tracing::info!("[UiBridge: TabsChanged] A created tab was not rendered yet; rendering the tab list");
                BridgeViewerTabs::sync_ui(&w, &tm, &lib, &set, &st);
                if w.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(&w, &tm, &lib, &set, &st);
                }
            });
        });
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    pub fn tab_manager(&self) -> &Arc<ViewerTabManager> {
        &self.tab_manager
    }

    fn register_settings_subscriber(&self) {
        let w_weak = self.window.as_weak();
        let library = self.library.clone();
        let thumbnails = self.thumbnails.clone();
        let tab_manager = self.tab_manager.clone();
        let settings = self.settings.clone();
        let state = self.state.clone();

        self.settings.subscribe(move |old_settings: &AppSettings, new_settings: &AppSettings, diff: &SettingsDiff| {
            if !diff.has_changes {
                return;
            }
            let (old_formats, new_formats) = (old_settings.formats.format_mask(), new_settings.formats.format_mask());

            let library = library.clone();
            let thumbnails = thumbnails.clone();
            let tab_manager = tab_manager.clone();
            let settings = settings.clone();
            let state = state.clone();
            let diff_clone = diff.clone();
            let new_clone = new_settings.clone();

            let _ = w_weak.upgrade_in_event_loop(move |window| {
                if diff_clone.performance_cache_changed {
                    let plan = MemoryPlan::resolve(new_clone.performance.memory_budget_mb, SystemMemory::read());
                    thumbnails.cache().set_budget(plan.thumbnail_cache_bytes as usize);
                    tracing::info!(
                        automatic = plan.automatic,
                        budget_mb = plan.budget_bytes / BYTES_PER_MB,
                        thumbnail_cache_mb = plan.thumbnail_cache_bytes / BYTES_PER_MB,
                        "[Memory: Budget] RAM budget changed; the thumbnail cache was resized"
                    );
                    if !plan.warnings.is_empty() {
                        tracing::warn!(warnings = %plan.warning_text(), "[Memory: Budget] The configured RAM budget is questionable");
                    }
                }

                if diff_clone.performance_pipeline_changed {
                    let new_config = ThumbnailConfig::from_performance_settings(&new_clone.performance);
                    thumbnails.set_config(new_config);
                    thumbnails.cache().clear();

                    if state.read().current_folder_id.is_some() {
                        BridgeMedia::reload(&window, &library, &thumbnails, &settings, &state, true, false);
                    }
                }

                if diff_clone.formats_policy_changed {
                    let ctx = FormatPolicyContext {
                        window: &window,
                        library: &library,
                        thumbnails: &thumbnails,
                        tab_manager: &tab_manager,
                        settings: &settings,
                        state: &state,
                    };
                    FormatPolicyApplier::apply(&ctx, old_formats, new_formats);
                }

                if diff_clone.tooltip_settings_changed {
                    window.set_enable_tooltips(new_clone.appearance.enable_tooltips);
                    window.set_tooltip_delay_ms(new_clone.appearance.tooltip_delay_ms as i32);
                    bar_overlay::set_tooltip_settings(
                        new_clone.appearance.enable_tooltips,
                        new_clone.appearance.tooltip_delay_ms as i32,
                    );
                }

                if diff_clone.type_ahead_changed {
                    type_ahead::config::apply(&new_clone.appearance);
                }

                if diff_clone.fullscreen_settings_changed || diff_clone.viewer_settings_changed {
                    prefs::apply(&new_clone.fullscreen);
                    multiview::refresh_settings(&settings);
                }

                if diff_clone.view_settings_changed {
                    window.set_info_is_collapsed(new_clone.view.is_inspector_collapsed);
                }

                if diff_clone.appearance_grid_changed {
                    window.set_thumb_size(new_clone.appearance.default_thumbnail_size as i32);
                    let ratio = GridAspectRatio::from_str_name(&new_clone.appearance.thumbnail_aspect_ratio);
                    window.set_thumb_aspect_idx(ratio.to_index());
                    ThumbnailBadgeBridge::apply_config(&window, &new_clone.appearance);
                    window.set_grid_item_padding_h(new_clone.appearance.grid_item_padding_horizontal as i32);
                    window.set_grid_item_padding_v(new_clone.appearance.grid_item_padding_vertical as i32);
                    window.set_grid_item_border_width(new_clone.appearance.grid_item_border_width as i32);
                    window.set_grid_item_corner_radius(new_clone.appearance.grid_item_corner_radius as i32);

                    if window.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(&window, &tab_manager, &library, &settings, &state);
                    }
                }

                if diff_clone.viewer_settings_changed {
                    let bg_mode_idx = match new_clone.viewer.background_mode.to_lowercase().as_str() {
                        "deep black" | "black" => 1,
                        "transparent checkerboard" | "checkerboard" => 2,
                        _ => 0,
                    };
                    window.set_viewer_background_mode(bg_mode_idx);
                    window.set_viewer_show_tab_rating_badge(new_clone.viewer.tabs.show_rating_badge);
                }

                if diff_clone.debug_settings_changed {
                    LogObscurer::set_obfuscate(new_clone.debug.obfuscate_logs);
                }

                if diff_clone.folder_tree_presentation_changed && !diff_clone.root_folder_changed {
                    window.set_show_hidden_folders(new_clone.folders.show_hidden_folders);
                    BridgeFolders::reload_tree(&window, &library, &settings, &state);
                }

                if diff_clone.root_folder_changed {
                    state.write().invalidate_folder_hierarchy();
                    BridgeFolders::sync_root(&window, &library, &thumbnails, &settings, &state);
                }

                if diff_clone.tags_settings_changed {
                    {
                        let mut st = state.write();
                        st.max_history_tags = new_clone.tags.max_history_tags;
                        st.is_tag_history_collapsed = new_clone.tags.is_history_collapsed;
                        st.tag_history_ids = new_clone.tags.history_tag_ids.clone();
                        st.tag_history_height = new_clone.tags.history_height;
                    }
                    window.set_tag_history_height(new_clone.tags.history_height);
                    window.set_tag_history_is_collapsed(new_clone.tags.is_history_collapsed);
                    BridgeTagHistory::sync_ui(&window, &library, &state);
                }

                BridgeSidebarPanelTabs::sync_ui(&window, &settings, &state);

                StatusBarBridge::update_metrics(&window, &state);
            });
        });
    }

    fn register_subsystem_callbacks(&self) {
        BridgeViewerTabs::register_callbacks(
            &self.window,
            self.tab_manager.clone(),
            self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            self.state.clone(),
            self.fullscreen.clone(),
        );

        BridgeFolders::register_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            self.state.clone(),
            self.settings_dialog.clone(),
        );

        BridgeSidebarPanelTabs::register_callbacks(
            &self.window,
            self.settings.clone(),
            self.state.clone(),
        );

        BridgeFileSelection::register_callbacks(
            &self.window,
            self.library.clone(),
            self.settings.clone(),
            self.tab_manager.clone(),
            self.state.clone(),
            self.last_click.clone(),
            self.fullscreen.clone(),
        );

        BridgeContextMenu::register_callbacks(
            &self.window,
            ContextMenuRegistration {
                library: self.library.clone(),
                thumbnails: self.thumbnails.clone(),
                tab_manager: self.tab_manager.clone(),
                favorites: self.favorites.clone(),
                state: self.state.clone(),
                dispatcher: self.ctx_menu_dispatcher.clone(),
                rename_dialog: self.rename_dialog.clone(),
                favorite_rename_dialog: self.favorite_rename_dialog.clone(),
                missing_file_dialog: self.missing_file_dialog.clone(),
                delete_file_dialog: self.delete_file_dialog.clone(),
                remove_orphaned_dialog: self.remove_orphaned_dialog.clone(),
                conflict_dialog: self.conflict_dialog.clone(),
                tag_dialogs: self.tag_dialogs.clone(),
                folder_dialogs: self.folder_dialogs.clone(),
                fullscreen: self.fullscreen.clone(),
                settings: self.settings.clone(),
            },
        );

        BridgeTags::register_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.tab_manager.clone(),
            self.settings.clone(),
            self.state.clone(),
        );

        let set_th_col = self.settings.clone();
        let st_th_col = self.state.clone();
        self.window.on_tag_history_collapsed_toggled(move |collapsed| {
            BridgeTagHistory::handle_collapsed_toggled(collapsed, &set_th_col, &st_th_col);
        });

        let win_th_click = self.window.clone_strong();
        let lib_th_click = self.library.clone();
        let thumb_th_click = self.thumbnails.clone();
        let tm_th_click = self.tab_manager.clone();
        let set_th_click = self.settings.clone();
        let st_th_click = self.state.clone();
        self.window.on_tag_history_tag_clicked(move |tag_id| {
            BridgeTagHistory::handle_tag_history_clicked(
                tag_id as u32,
                &win_th_click,
                &lib_th_click,
                &thumb_th_click,
                &tm_th_click,
                &set_th_click,
                &st_th_click,
            );
        });

        let win_th_geom = self.window.clone_strong();
        let lib_th_geom = self.library.clone();
        let st_th_geom = self.state.clone();
        self.window.on_tag_history_geometry_changed(move |avail_w| {
            BridgeTagHistory::handle_geometry_changed(
                avail_w,
                &win_th_geom,
                &lib_th_geom,
                &st_th_geom,
            );
        });

        let last_th_resize = Arc::new(Mutex::new(Instant::now()));
        let th_resize_count = Arc::new(AtomicUsize::new(0));
        let last_th_clone = last_th_resize;
        let th_count_clone = th_resize_count;
        let st_th_resize = self.state.clone();

        self.window.on_tag_history_resized(move |new_height| {
            let count = th_count_clone.fetch_add(1, Ordering::Relaxed) + 1;
            st_th_resize.write().tag_history_height = new_height;

            let mut last_log = last_th_clone.lock();
            let elapsed = last_log.elapsed();
            if elapsed.as_millis() >= 100 {
                let hz = if elapsed.as_secs_f64() > 0.0 {
                    (count as f64) / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                tracing::debug!(
                    height = new_height,
                    event_rate_hz = format!("{:.1}", hz),
                    total_events = count,
                    "[Benchmark: TagHistory Drag] Smooth high-FPS Tag History height resize active"
                );
                *last_log = Instant::now();
                th_count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_th_fin = self.settings.clone();
        let st_th_fin = self.state.clone();
        self.window.on_tag_history_resize_finished(move |final_height| {
            tracing::info!(
                final_height,
                "[UiBridge: TagHistory] Tag History height resize finished and persisted"
            );
            st_th_fin.write().tag_history_height = final_height;
            let mut app_settings = set_th_fin.get();
            app_settings.tags.history_height = final_height;
            set_th_fin.save_quiet_async(&app_settings);
        });

        BridgeMedia::register_query_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.tab_manager.clone(),
            self.settings.clone(),
            self.state.clone(),
        );

        let win_fav = self.window.clone_strong();
        let lib_fav = self.library.clone();
        let thumb_fav = self.thumbnails.clone();
        let set_fav = self.settings.clone();
        let st_fav = self.state.clone();
        self.window.on_favorite_selected(move |path_str| {
            st_fav.write().last_active_panel = LastActivePanel::Favorite;
            BridgeFavorites::handle_favorite_clicked(
                path_str.as_str(),
                &win_fav,
                &lib_fav,
                &thumb_fav,
                &set_fav,
                &st_fav,
            );
        });

        let set_collapse = self.settings.clone();
        let st_collapse = self.state.clone();
        self.window.on_info_collapsed_toggled(move |collapsed| {
            tracing::info!(
                collapsed,
                "[UiBridge: InfoInspector] Toggled inspector collapsible panel state"
            );
            st_collapse.write().is_inspector_collapsed = collapsed;
            let mut app_settings = set_collapse.get();
            app_settings.view.is_inspector_collapsed = collapsed;
            set_collapse.save_quiet_async(&app_settings);
        });

        self.register_rename_callbacks();
        self.register_delete_callbacks();
        self.register_browse_folder_callbacks();
        self.register_keyboard_navigation_callbacks();
        self.register_type_ahead_callbacks();
        self.register_tab_grid_callbacks();
        self.register_dnd_callbacks();
        self.register_rubberband_callbacks();
        self.register_sidebar_resize_callbacks();
        self.register_tooltip_overlay_callbacks();
        self.register_modal_dismiss_callback();
    }

    fn register_modal_dismiss_callback(&self) {
        let fs_ctrl = self.fullscreen.clone();
        let ren_dlg = self.rename_dialog.clone();
        let fav_ren_dlg = self.favorite_rename_dialog.clone();
        let set_dlg = self.settings_dialog.clone();
        let miss_dlg = self.missing_file_dialog.clone();
        let del_dlg = self.delete_file_dialog.clone();
        let orph_dlg = self.remove_orphaned_dialog.clone();
        let conf_dlg = self.conflict_dialog.clone();
        let fld_dlg = self.folder_dialogs.clone();
        let tag_dlg = self.tag_dialogs.clone();
        let win = self.window.clone_strong();
        let st = self.state.clone();

        self.window.on_modal_dismiss_requested(move || {
            tracing::info!(
                "[UiBridge: ModalDismiss] Main UI modal dismiss requested (backdrop clicked or Escape pressed during modal lock)"
            );
            if fs_ctrl.is_isolated_open() {
                fs_ctrl.request_close();
            }
            FileRenameDialogController::close(&ren_dlg, &st, &win, &fs_ctrl);
            fav_ren_dlg.close(&win);
            set_dlg.borrow().close();
            miss_dlg.close(&win);
            del_dlg.close(&win);
            orph_dlg.close(&win);
            conf_dlg.close(&win);
            fld_dlg.close_rename(&win);
            fld_dlg.close_delete(&win);
            fld_dlg.close_missing_delete(&win);
            fld_dlg.close_move(&win);
            fld_dlg.close_remove_orphaned(&win);
            fld_dlg.close_reborn_warn(&win);
            tag_dlg.close_add(&win);
            tag_dlg.close_rename(&win);
            tag_dlg.close_delete(&win);
            win.set_is_modal_locked(false);
        });
    }

    fn register_tooltip_overlay_callbacks(&self) {
        let tm = self.tab_manager.clone();
        let win_weak = self.window.as_weak();

        self.window.on_global_tooltip_state_changed(move |is_visible, x, y, w, h| {
            if let Some(win) = win_weak.upgrade() {
                if is_visible && w > 2.0 && h > 2.0 {
                    let scale = win.window().scale_factor();
                    let cutouts = super::bridge_ctx_menu::make_rounded_cutout_rectangles(x, y, w, h, scale);
                    tm.set_tooltip_cutouts(cutouts);
                } else {
                    tm.clear_tooltip_cutouts();
                }
            }
        });
    }

    fn register_rename_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let tm = self.tab_manager.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let ren_dlg = self.rename_dialog.clone();
        let fav_ren_dlg = self.favorite_rename_dialog.clone();
        let favs = self.favorites.clone();
        let missing_dlg = self.missing_file_dialog.clone();
        let conflict_dlg = self.conflict_dialog.clone();
        let fld_dlg = self.folder_dialogs.clone();
        let fs_ctrl = self.fullscreen.clone();

        self.window.on_request_rename(move || {
            Self::trigger_rename_action(RenameActionParams {
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                tab_manager: &tm,
                settings: &set,
                state: &st,
                rename_dialog: &ren_dlg,
                favorite_rename_dialog: &fav_ren_dlg,
                favorites: &favs,
                missing_dialog: &missing_dlg,
                conflict_dialog: &conflict_dlg,
                folder_dialogs: &fld_dlg,
                fullscreen: &fs_ctrl,
            });
        });
    }

    /// The folder rows of the Browse view, the "Show Folders" toggle and the "New Folder" button of the toolbar.
    fn register_browse_folder_callbacks(&self) {
        BrowseFolders::register_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            self.state.clone(),
        );

        let (win, lib, set, st) = (self.window.clone_strong(), self.library.clone(), self.settings.clone(), self.state.clone());
        self.window.on_browse_go_up_requested(move || BrowseFolders::go_up(&win, &lib, &set, &st));
        let (win, lib, set, st) = (self.window.clone_strong(), self.library.clone(), self.settings.clone(), self.state.clone());
        self.window.on_browse_history_requested(move |direction| BrowseFolders::go_history(direction, &win, &lib, &set, &st));

        let win = self.window.clone_strong();
        let ctx = FolderDialogContext {
            library: self.library.clone(),
            thumbnails: self.thumbnails.clone(),
            tab_manager: self.tab_manager.clone(),
            settings: self.settings.clone(),
            state: self.state.clone(),
        };
        let dialogs = self.folder_dialogs.clone();
        self.window.on_new_folder_clicked(move || {
            let folder = ctx.state.read().current_folder_id;
            match folder {
                Some(folder_id) => dialogs.show_new(&win, folder_id, ctx.clone()),
                None => StatusBarBridge::info(&win, "Select a folder to create a new folder in."),
            }
        });
    }

    fn register_delete_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let tm = self.tab_manager.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let favs = self.favorites.clone();
        let del_file_dlg = self.delete_file_dialog.clone();
        let orph_file_dlg = self.remove_orphaned_dialog.clone();
        let fld_dlg = self.folder_dialogs.clone();
        let fs_ctrl = self.fullscreen.clone();

        self.window.on_request_delete(move || {
            Self::trigger_delete_action(DeleteActionParams {
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                tab_manager: &tm,
                settings: &set,
                state: &st,
                favorites: &favs,
                delete_file_dialog: &del_file_dlg,
                remove_orphaned_file_dialog: &orph_file_dlg,
                folder_dialogs: &fld_dlg,
                fullscreen: &fs_ctrl,
            });
        });
    }

    pub fn trigger_delete_action(params: DeleteActionParams<'_>) {
        let window = params.window;
        let active_mode = window.get_active_view_mode();
        // DEL or the delete button in the fullscreen window acts on the medium there; DEL in the main window always acts on
        // what the main window shows, even while a Multiview is open next to it.
        let fullscreen_target = super::multiview::handle::delete_requested_from_fullscreen();

        if fullscreen_target.is_none() && active_mode != 0 {
            tracing::debug!(
                active_mode,
                "[UiBridge: DeleteAction] DEL key pressed outside Browse View (mode != 0); operation safely bypassed"
            );
            return;
        }

        if BridgeMedia::is_system_busy(window, params.thumbnails, params.state) {
            StatusBarBridge::warn(
                window,
                "Cannot delete: an active operation is in progress.",
            );
            return;
        }

        if let Some(media_id) = fullscreen_target {
            tracing::info!(
                media_id,
                "[UiBridge: DeleteAction] Delete requested in the fullscreen: opening delete files dialog for its medium"
            );
            let fullscreen = params.fullscreen.clone();
            let followup: DeleteFollowup = Rc::new(move |ids| fullscreen.drop_deleted(ids));
            Self::open_media_delete_dialog(&params, vec![media_id], Some(followup));
            return;
        }

        let last_panel = params.state.read().last_active_panel;
        let active_sidebar_tab = window.get_active_sidebar_panel_tab();
        let is_folders_tab = active_sidebar_tab.as_str() == "folders";
        let is_favorites_tab = active_sidebar_tab.as_str() == "favorites";

        let browse_folders = params.state.read().focused_browse_folders();
        let effective_panel = match last_panel {
            _ if !browse_folders.is_empty() => LastActivePanel::Folder,
            LastActivePanel::Media => LastActivePanel::Media,
            LastActivePanel::Folder => {
                if is_favorites_tab {
                    LastActivePanel::Favorite
                } else {
                    LastActivePanel::Folder
                }
            }
            LastActivePanel::Favorite => {
                if is_folders_tab {
                    LastActivePanel::Folder
                } else {
                    LastActivePanel::Favorite
                }
            }
        };

        if effective_panel == LastActivePanel::Favorite {
            let selected_fav_path = params.state.read().selected_favorite_path.clone();
            let path_to_remove = selected_fav_path.or_else(|| {
                params.state.read().current_folder_path.clone()
            });

            // The entry is copied out first: a read guard in the `if let` would live through the block, where the favorite is
            // changed under the write lock.
            let found = path_to_remove.and_then(|path| {
                let entry = params.favorites.read().get_by_path(&path).cloned();
                entry.map(|entry| (path, entry))
            });
            if let Some((path, entry)) = found
            {
                tracing::info!(
                    path = %LogObscurer::path(&path),
                    alias = %entry.alias,
                    "[UiBridge: DeleteAction] DEL pressed on Favorite focus: Removing favorite"
                );
                BridgeFavorites::remove_favorite(window, params.favorites, &path);
                return;
            }

            StatusBarBridge::info(window, "Select a favorite to remove (Del).");
            return;
        }

        if effective_panel == LastActivePanel::Folder {
            // Folders highlighted in the Browse view: one folder alone goes through its own dialog, any larger selection through the
            // combined dialog for files and folders.
            let files = params.state.read().get_selected_ids_ordered();
            if browse_folders.len() > 1 || (!browse_folders.is_empty() && !files.is_empty()) {
                let ctx = FolderDialogContext {
                    library: params.library.clone(),
                    thumbnails: params.thumbnails.clone(),
                    tab_manager: params.tab_manager.clone(),
                    settings: params.settings.clone(),
                    state: params.state.clone(),
                };
                tracing::info!(
                    folders = browse_folders.len(),
                    files = files.len(),
                    "[UiBridge: DeleteAction] Delete requested on a selection of folders and files: opening the combined dialog"
                );
                params.folder_dialogs.show_delete_selection(window, files, browse_folders, ctx);
                return;
            }

            let current_folder_id = browse_folders.first().copied().or(params.state.read().current_folder_id);
            let Some(folder_id) = current_folder_id else {
                StatusBarBridge::info(window, "No folder selected to delete (Del).");
                return;
            };
            Self::open_folder_delete_dialog(&params, folder_id);
            return;
        }

        let selected_ids = params.state.read().get_selected_ids_ordered();
        if selected_ids.is_empty() {
            StatusBarBridge::info(window, "Select one or more files to delete (Del).");
            return;
        }

        Self::open_media_delete_dialog(&params, selected_ids, None);
    }

    /// Opens the dialog that deletes a folder (or, for a folder that is missing on disk, the one that removes it from the database).
    fn open_folder_delete_dialog(params: &DeleteActionParams<'_>, folder_id: FolderId) {
        let window = params.window;
        let is_root = {
            let app_settings = params.settings.get();
            if let Some(ref root_p) = app_settings.general.root_folder
                && let Ok(Some(folder_rec)) = params.library.folders().get_folder(folder_id)
            {
                normalize_path(Path::new(&folder_rec.path)) == normalize_path(root_p)
            } else {
                false
            }
        };

        if is_root {
            StatusBarBridge::info(window, "Cannot delete the library root directory.");
            return;
        }

        let folder_rec_opt = params.library.folders().get_folder(folder_id).ok().flatten();
        let Some(folder_rec) = folder_rec_opt else {
            StatusBarBridge::error(window, "Folder record not found in database.");
            return;
        };

        let folder_name = folder_rec.name.clone();

        let folder_ctx = FolderDialogContext {
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            tab_manager: params.tab_manager.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
        };

        if folder_rec.is_deleted {
            tracing::info!(
                folder = %LogObscurer::folder(Some(folder_id), Path::new(&folder_name)),
                "[UiBridge: DeleteAction] Delete requested on a missing folder: opening remove_orphaned folder dialog"
            );
            params.folder_dialogs.show_remove_orphaned(window, folder_id, &folder_name, folder_ctx);
        } else {
            tracing::info!(
                folder = %LogObscurer::folder(Some(folder_id), Path::new(&folder_name)),
                "[UiBridge: DeleteAction] Delete requested on an active folder: opening delete folder dialog"
            );
            params.folder_dialogs.show_delete(window, folder_id, &folder_name, folder_ctx);
        }
    }

    /// Opens the dialog that deletes `media_ids` (or, when every one of them is already missing on disk, the one that only
    /// removes them from the database). `followup` runs after a confirmed deletion.
    fn open_media_delete_dialog(
        params: &DeleteActionParams<'_>,
        media_ids: Vec<MediaId>,
        followup: Option<DeleteFollowup>,
    ) {
        let window = params.window;
        let all_selected_deleted = {
            let st = params.state.read();
            media_ids.iter().all(|&id| {
                st.all_folder_media
                    .find(id)
                    .map(|m| m.record.is_deleted)
                    .unwrap_or(false)
            })
        };

        let dialog_ctx = FileDialogContext {
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            tab_manager: params.tab_manager.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
        };

        if all_selected_deleted {
            tracing::info!(
                count = media_ids.len(),
                "[UiBridge: DeleteAction] DEL pressed on media selection (all missing): opening remove_orphaned files dialog"
            );
            params.remove_orphaned_file_dialog.show(window, media_ids, dialog_ctx);
        } else {
            tracing::info!(
                count = media_ids.len(),
                "[UiBridge: DeleteAction] DEL pressed on media selection: opening delete files dialog"
            );
            params.delete_file_dialog.show_with_followup(window, media_ids, dialog_ctx, followup);
        }
    }

    pub fn trigger_rename_action(params: RenameActionParams<'_>) {
        let window = params.window;
        let active_mode = window.get_active_view_mode();
        // F2 pressed in the fullscreen window acts on the medium there; F2 in the main window always acts on what the main
        // window shows, even while a Multiview is open next to it.
        let from_fullscreen = super::multiview::handle::rename_requested_from_fullscreen();

        if from_fullscreen {
            if let Some(media_id) = params.fullscreen.rename_target() {
                Self::open_media_rename(media_id, &params);
            }
            return;
        }

        if active_mode == 1 {
            if let Some(media_id) = tab_host::rename_target(params.tab_manager) {
                Self::open_media_rename(media_id, &params);
            }
            return;
        }

        let last_panel = params.state.read().last_active_panel;
        let active_sidebar_tab = window.get_active_sidebar_panel_tab();
        let is_folders_tab = active_sidebar_tab.as_str() == "folders";
        let is_favorites_tab = active_sidebar_tab.as_str() == "favorites";

        let browse_folders = params.state.read().focused_browse_folders();
        let effective_panel = match last_panel {
            _ if !browse_folders.is_empty() => LastActivePanel::Folder,
            LastActivePanel::Media => LastActivePanel::Media,
            LastActivePanel::Folder => {
                if is_favorites_tab {
                    LastActivePanel::Favorite
                } else {
                    LastActivePanel::Folder
                }
            }
            LastActivePanel::Favorite => {
                if is_folders_tab {
                    LastActivePanel::Folder
                } else {
                    LastActivePanel::Favorite
                }
            }
        };

        if effective_panel == LastActivePanel::Favorite {
            let selected_fav_path = params.state.read().selected_favorite_path.clone();
            let path_to_rename = selected_fav_path.or_else(|| {
                params.state.read().current_folder_path.clone()
            });

            // The entry is copied out first: a read guard in the `if let` would live through the block, where the favorite is
            // changed under the write lock.
            let found = path_to_rename.and_then(|path| {
                let entry = params.favorites.read().get_by_path(&path).cloned();
                entry.map(|entry| (path, entry))
            });
            if let Some((path, entry)) = found
            {
                tracing::info!(
                    path = %LogObscurer::path(&path),
                    alias = %entry.alias,
                    "[UiBridge: RenameAction] F2 pressed on Favorite focus: Opening Rename Favorite dialog"
                );
                params.favorite_rename_dialog.show(
                    params.window,
                    &entry.alias,
                    entry.path.clone(),
                    params.favorites.clone(),
                );
                return;
            }

            StatusBarBridge::info(window, "Select a favorite to rename (F2).");
            return;
        }

        if effective_panel == LastActivePanel::Folder {
            let file_count = params.state.read().file_selection.count();
            if browse_folders.len() > 1 || (!browse_folders.is_empty() && file_count > 0) {
                StatusBarBridge::warn(window, "Cannot rename several items at once. Select a single item.");
                return;
            }
            let current_folder_id = browse_folders.first().copied().or(params.state.read().current_folder_id);
            let Some(folder_id) = current_folder_id else {
                StatusBarBridge::info(window, "No folder selected to rename (F2).");
                return;
            };

            let is_root = {
                let app_settings = params.settings.get();
                if let Some(ref root_p) = app_settings.general.root_folder
                    && let Ok(Some(folder_rec)) = params.library.folders().get_folder(folder_id)
                {
                    normalize_path(Path::new(&folder_rec.path)) == normalize_path(root_p)
                } else {
                    false
                }
            };

            if is_root {
                StatusBarBridge::info(window, "Cannot rename the library root directory.");
                return;
            }

            let folder_rec_opt = params.library.folders().get_folder(folder_id).ok().flatten();
            let Some(folder_rec) = folder_rec_opt else {
                StatusBarBridge::error(window, "Folder record not found in database.");
                return;
            };

            let folder_name = folder_rec.name.clone();

            let folder_ctx = FolderDialogContext {
                library: params.library.clone(),
                thumbnails: params.thumbnails.clone(),
                tab_manager: params.tab_manager.clone(),
                settings: params.settings.clone(),
                state: params.state.clone(),
            };

            tracing::info!(
                folder = %LogObscurer::folder(Some(folder_id), Path::new(&folder_name)),
                "[UiBridge: RenameAction] F2 pressed on Folder Tree focus: Opening Rename Folder dialog"
            );

            params.folder_dialogs.show_rename(
                window,
                folder_id,
                &folder_name,
                folder_ctx,
            );
            return;
        }

        let sel_ids = params.state.read().file_selection.selected_ids().clone();
        if sel_ids.len() == 1 {
            let media_id = *sel_ids.iter().next().unwrap();
            Self::open_media_rename(media_id, &params);
        } else if sel_ids.is_empty() {
            StatusBarBridge::info(window, "Select a file to rename (F2).");
        } else {
            StatusBarBridge::warn(
                window,
                "Cannot rename multiple files at once. Select a single file.",
            );
        }
    }

    fn open_media_rename(media_id: MediaId, params: &RenameActionParams<'_>) {
        tracing::info!(
            media_id,
            "[UiBridge: RenameAction] Opening rename dialog for media file"
        );

        let dialog_ctx = FileDialogContext {
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            tab_manager: params.tab_manager.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
        };

        FileRenameDialogController::open(RenameDialogParams {
            dialog_ref: params.rename_dialog,
            window: params.window,
            media_id,
            ctx: &dialog_ctx,
            conflict_dialog: params.conflict_dialog,
            missing_dialog: params.missing_dialog,
            fullscreen: params.fullscreen,
        });
    }

    fn register_keyboard_navigation_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let fs = self.fullscreen.clone();

        self.window.on_media_navigate(move |direction_code, is_shift, page_rows| {
            handle_keyboard_navigate(KeyboardNavigateParams {
                direction_code,
                is_shift,
                page_rows,
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                settings: &set,
                state: &st,
                fullscreen: &fs,
            });
        });
    }

    fn register_type_ahead_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let search: TypeAheadState = TypeAheadState::default();

        self.window.on_type_ahead_key(move |text, command| {
            handle_type_ahead_key(TypeAheadParams {
                text: text.as_str(),
                command,
                search: &search,
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                settings: &set,
                state: &st,
            })
        });
    }

    fn register_tab_grid_callbacks(&self) {
        let win_grid = self.window.clone_strong();
        let tm_grid = self.tab_manager.clone();
        let lib_grid = self.library.clone();
        let set_grid = self.settings.clone();
        let st_grid = self.state.clone();
        let fs_grid = self.fullscreen.clone();

        self.window.on_tab_grid_clicked(move || {
            tracing::info!("[UiBridge: TabGrid] Tab-Grid button clicked. Switching to Masonry Tab Grid view");
            tm_grid.set_all_videos_view_active(false);
            win_grid.set_active_view_mode(2);
            win_grid.set_is_browse_active(false);
            win_grid.set_is_tab_grid_active(true);
            BridgeViewerTabs::sync_ui(&win_grid, &tm_grid, &lib_grid, &set_grid, &st_grid);
            BridgeViewerTabs::sync_video_embedding(&win_grid, &tm_grid, &fs_grid, &set_grid);
            BridgeTabGrid::sync_tab_grid_ui(&win_grid, &tm_grid, &lib_grid, &set_grid, &st_grid);
            StatusBarBridge::update_metrics(&win_grid, &st_grid);
        });

        let win_left = self.window.clone_strong();
        let tm_left = self.tab_manager.clone();
        let lib_left = self.library.clone();
        let set_left = self.settings.clone();
        let st_left = self.state.clone();
        let fs_left = self.fullscreen.clone();
        self.window.on_tab_grid_item_left_clicked(move |tab_id| {
            BridgeTabGrid::handle_item_left_clicked(
                tab_id as u64,
                &win_left,
                &tm_left,
                &lib_left,
                &set_left,
                &st_left,
                &fs_left,
            );
        });

        let win_mid = self.window.clone_strong();
        let tm_mid = self.tab_manager.clone();
        let set_mid = self.settings.clone();
        let fs_mid = self.fullscreen.clone();
        self.window.on_tab_grid_item_middle_clicked(move |tab_id| {
            BridgeTabGrid::handle_item_middle_clicked(
                tab_id as u64,
                &win_mid,
                &tm_mid,
                &set_mid,
                &fs_mid,
            );
        });

        let win_close = self.window.clone_strong();
        let tm_close = self.tab_manager.clone();
        let lib_close = self.library.clone();
        let set_close = self.settings.clone();
        let st_close = self.state.clone();
        self.window.on_tab_grid_item_close_clicked(move |tab_id| {
            BridgeTabGrid::handle_item_close_clicked(
                tab_id as u64,
                &win_close,
                &tm_close,
                &lib_close,
                &set_close,
                &st_close,
            );
        });

        let win_cols = self.window.clone_strong();
        let tm_cols = self.tab_manager.clone();
        let lib_cols = self.library.clone();
        let set_cols = self.settings.clone();
        let st_cols = self.state.clone();
        self.window.on_tab_grid_columns_changed(move |new_cols| {
            let cols = (new_cols as usize).clamp(1, 10);
            {
                let mut st = st_cols.write();
                st.tab_grid_columns = cols;
                st.tab_grid_auto_columns = false;
            }
            {
                let mut app_settings = set_cols.get();
                app_settings.view.tab_grid_columns = cols;
                app_settings.view.tab_grid_auto_columns = false;
                set_cols.save_quiet_async(&app_settings);
            }
            BridgeTabGrid::sync_tab_grid_ui(&win_cols, &tm_cols, &lib_cols, &set_cols, &st_cols);
        });

        let win_auto = self.window.clone_strong();
        let tm_auto = self.tab_manager.clone();
        let lib_auto = self.library.clone();
        let set_auto = self.settings.clone();
        let st_auto = self.state.clone();
        self.window.on_tab_grid_auto_columns_toggled(move |is_auto| {
            {
                let mut st = st_auto.write();
                st.tab_grid_auto_columns = is_auto;
            }
            {
                let mut app_settings = set_auto.get();
                app_settings.view.tab_grid_auto_columns = is_auto;
                set_auto.save_quiet_async(&app_settings);
            }
            BridgeTabGrid::sync_tab_grid_ui(&win_auto, &tm_auto, &lib_auto, &set_auto, &st_auto);
        });

        let win_close_all = self.window.clone_strong();
        let tm_close_all = self.tab_manager.clone();
        let lib_close_all = self.library.clone();
        let set_close_all = self.settings.clone();
        let st_close_all = self.state.clone();
        self.window.on_tab_grid_close_all_clicked(move || {
            tracing::info!("[UiBridge: TabGrid] Close all tabs clicked from TabGrid toolbar");
            tm_close_all.close_all_tabs();
            BridgeViewerTabs::sync_ui(&win_close_all, &tm_close_all, &lib_close_all, &set_close_all, &st_close_all);
            BridgeTabGrid::sync_tab_grid_ui(&win_close_all, &tm_close_all, &lib_close_all, &set_close_all, &st_close_all);
        });

        let win_geom = self.window.clone_strong();
        let tm_geom = self.tab_manager.clone();
        let lib_geom = self.library.clone();
        let set_geom = self.settings.clone();
        let st_geom = self.state.clone();
        self.window.on_tab_grid_viewport_geometry_changed(move |_vw, _vh| {
            if win_geom.get_active_view_mode() == 2 {
                BridgeTabGrid::sync_tab_grid_ui(&win_geom, &tm_geom, &lib_geom, &set_geom, &st_geom);
            }
        });

        let win_scroll = self.window.clone_strong();
        let tm_scroll = self.tab_manager.clone();
        let set_scroll = self.settings.clone();
        let st_scroll = self.state.clone();
        self.window.on_tab_grid_scrolled(move |_dx, _dy| {
            BridgeTabGrid::handle_scrolled(&win_scroll, &tm_scroll, &set_scroll, &st_scroll);
        });
    }

    fn register_rubberband_callbacks(&self) {
        let win_start = self.window.clone_strong();
        let st_start = self.state.clone();
        let scroller_start = self.rubberband_autoscroller.clone();
        self.window.on_rubberband_started(move |cx, cy, vx, vy, ctrl, shift| {
            BridgeRubberband::handle_started(RubberbandDragStartParams {
                start_content_x: cx,
                start_content_y: cy,
                view_x: vx,
                view_y: vy,
                is_ctrl: ctrl,
                is_shift: shift,
                window: &win_start,
                state: &st_start,
                autoscroller: &scroller_start,
            });
        });

        let win_move = self.window.clone_strong();
        let lib_move = self.library.clone();
        let st_move = self.state.clone();
        let scroller_move = self.rubberband_autoscroller.clone();
        self.window.on_rubberband_moved(move |cx, cy, vx, vy| {
            BridgeRubberband::handle_moved(RubberbandDragMoveParams {
                content_x: cx,
                content_y: cy,
                view_x: vx,
                view_y: vy,
                window: &win_move,
                library: &lib_move,
                state: &st_move,
                autoscroller: &scroller_move,
            });
        });

        let win_end = self.window.clone_strong();
        let lib_end = self.library.clone();
        let st_end = self.state.clone();
        let scroller_end = self.rubberband_autoscroller.clone();
        self.window.on_rubberband_ended(move || {
            BridgeRubberband::handle_ended(&win_end, &lib_end, &st_end, &scroller_end);
        });
    }

    fn register_dnd_callbacks(&self) {
        let win_start = self.window.clone_strong();
        let lib_start = self.library.clone();
        let thumb_start = self.thumbnails.clone();
        let st_start = self.state.clone();
        let autoscroll_start = self.dnd_foldertree_autoscroller.clone();
        let browse_scroll_start = self.dnd_browse_scroller.clone();
        let fs_start = self.fullscreen.clone();
        self.window.on_media_drag_started(move |id, x, y| {
            BridgeDnd::handle_drag_started(DragStartParams {
                media_id: id as u64,
                start_x: x,
                start_y: y,
                window: &win_start,
                library: &lib_start,
                thumbnails: &thumb_start,
                state: &st_start,
                autoscroller: &autoscroll_start,
                browse_scroller: &browse_scroll_start,
                fullscreen: &fs_start,
            });
        });

        let win_move = self.window.clone_strong();
        let lib_move = self.library.clone();
        let st_move = self.state.clone();
        let autoscroll_move = self.dnd_foldertree_autoscroller.clone();
        let browse_scroll_move = self.dnd_browse_scroller.clone();
        self.window.on_media_drag_moved(move |x, y| {
            BridgeDnd::handle_drag_moved(
                x,
                y,
                &win_move,
                &lib_move,
                &st_move,
                &autoscroll_move,
                &browse_scroll_move,
            );
        });

        let win_hover = self.window.clone_strong();
        let st_hover = self.state.clone();
        self.window.on_folder_row_hovered(move |folder_id| {
            BridgeDnd::handle_folder_hovered(folder_id as u64, &win_hover, &st_hover);
        });

        let win_unhover = self.window.clone_strong();
        self.window.on_folder_row_unhovered(move |folder_id| {
            BridgeDnd::handle_folder_unhovered(folder_id as u64, &win_unhover);
        });

        let win_end = self.window.clone_strong();
        let lib_end = self.library.clone();
        let st_end = self.state.clone();
        let autoscroll_end = self.dnd_foldertree_autoscroller.clone();
        let browse_scroll_end = self.dnd_browse_scroller.clone();
        self.window.on_media_drag_ended(move || {
            BridgeDnd::handle_drag_ended(
                &win_end,
                &lib_end,
                &st_end,
                &autoscroll_end,
                &browse_scroll_end,
            );
        });

        let win_copy = self.window.clone_strong();
        let lib_copy = self.library.clone();
        let thumb_copy = self.thumbnails.clone();
        let set_copy = self.settings.clone();
        let st_copy = self.state.clone();
        let conflict_copy = self.conflict_dialog.clone();
        self.window.on_choice_popup_copy(move |target_fid| {
            win_copy.set_is_choice_popup_open(false);
            BridgeDnd::start_batch_transfer(BatchTransferParams {
                window: &win_copy,
                library: &lib_copy,
                thumbnails: &thumb_copy,
                settings: &set_copy,
                state: &st_copy,
                conflict_dialog: &conflict_copy,
                target_folder_id: target_fid as u64,
                transfer_kind: FileTransferKind::Copy,
            });
        });

        let win_mv = self.window.clone_strong();
        let lib_mv = self.library.clone();
        let thumb_mv = self.thumbnails.clone();
        let set_mv = self.settings.clone();
        let st_mv = self.state.clone();
        let conflict_mv = self.conflict_dialog.clone();
        self.window.on_choice_popup_move(move |target_fid| {
            win_mv.set_is_choice_popup_open(false);
            BridgeDnd::start_batch_transfer(BatchTransferParams {
                window: &win_mv,
                library: &lib_mv,
                thumbnails: &thumb_mv,
                settings: &set_mv,
                state: &st_mv,
                conflict_dialog: &conflict_mv,
                target_folder_id: target_fid as u64,
                transfer_kind: FileTransferKind::Move,
            });
        });

        let win_cancel = self.window.clone_strong();
        self.window.on_choice_popup_cancel(move || {
            tracing::info!("[UiBridge: ChoicePopup] Floating choice popup cancelled via Cancel button");
            win_cancel.set_is_choice_popup_open(false);
        });

        let win_dism = self.window.clone_strong();
        self.window.on_choice_popup_dismissed(move || {
            tracing::info!("[UiBridge: ChoicePopup] Floating choice popup dismissed via Escape key or backdrop click");
            win_dism.set_is_choice_popup_open(false);
        });
    }

    fn register_sidebar_resize_callbacks(&self) {
        let last_left_resize = Arc::new(Mutex::new(Instant::now()));
        let left_events_count = Arc::new(AtomicUsize::new(0));

        let last_left_clone = last_left_resize;
        let left_count_clone = left_events_count;

        self.window.on_sidebar_left_resized(move |new_width| {
            let t0 = Instant::now();
            let count = left_count_clone.fetch_add(1, Ordering::Relaxed) + 1;

            let mut last_log = last_left_clone.lock();
            let elapsed = last_log.elapsed();
            if elapsed.as_millis() >= 100 {
                let hz = if elapsed.as_secs_f64() > 0.0 {
                    (count as f64) / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                let elapsed_us = t0.elapsed().as_micros();
                tracing::debug!(
                    sidebar = "left",
                    width = new_width,
                    event_rate_hz = format!("{:.1}", hz),
                    total_events = count,
                    processing_us = elapsed_us,
                    "[Benchmark: Sidebar Drag] Smooth high-FPS left sidebar resize active"
                );
                *last_log = Instant::now();
                left_count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_left = self.settings.clone();
        self.window.on_sidebar_left_resize_finished(move |final_width| {
            let width_f32 = final_width;
            let mut app_settings = set_left.get();
            app_settings.view.sidebar_left_width = width_f32;
            set_left.save_quiet(&app_settings);
        });

        let last_right_resize = Arc::new(Mutex::new(Instant::now()));
        let right_events_count = Arc::new(AtomicUsize::new(0));

        let last_right_clone = last_right_resize;
        let right_count_clone = right_events_count;

        let tm_right = self.tab_manager.clone();
        let fs_right = self.fullscreen.clone();
        let set_right = self.settings.clone();
        let win_right = self.window.clone_strong();
        let lib_right = self.library.clone();
        let st_right = self.state.clone();

        self.window.on_sidebar_right_resized(move |new_width| {
            let t0 = Instant::now();
            let count = right_count_clone.fetch_add(1, Ordering::Relaxed) + 1;

            if win_right.get_active_view_mode() == 1 {
                BridgeViewerTabs::sync_video_embedding(&win_right, &tm_right, &fs_right, &set_right);
            } else if win_right.get_active_view_mode() == 2 {
                BridgeTabGrid::sync_tab_grid_ui(&win_right, &tm_right, &lib_right, &set_right, &st_right);
            }

            let mut last_log = last_right_clone.lock();
            let elapsed = last_log.elapsed();
            if elapsed.as_millis() >= 100 {
                let hz = if elapsed.as_secs_f64() > 0.0 {
                    (count as f64) / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                let elapsed_us = t0.elapsed().as_micros();
                tracing::debug!(
                    sidebar = "right",
                    width = new_width,
                    event_rate_hz = format!("{:.1}", hz),
                    total_events = count,
                    processing_us = elapsed_us,
                    "[Benchmark: Sidebar Drag] Smooth high-FPS right sidebar resize active"
                );
                *last_log = Instant::now();
                right_count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_right_fin = self.settings.clone();
        let tm_right_fin = self.tab_manager.clone();
        let fs_right_fin = self.fullscreen.clone();
        let win_right_fin = self.window.clone_strong();
        let lib_right_fin = self.library.clone();
        let st_right_fin = self.state.clone();

        self.window.on_sidebar_right_resize_finished(move |final_width| {
            let width_f32 = final_width;
            let mut app_settings = set_right_fin.get();
            app_settings.view.sidebar_right_width = width_f32;
            set_right_fin.save_quiet(&app_settings);

            if win_right_fin.get_active_view_mode() == 1 {
                BridgeViewerTabs::sync_video_embedding(&win_right_fin, &tm_right_fin, &fs_right_fin, &set_right_fin);
            } else if win_right_fin.get_active_view_mode() == 2 {
                BridgeTabGrid::sync_tab_grid_ui(&win_right_fin, &tm_right_fin, &lib_right_fin, &set_right_fin, &st_right_fin);
            }
        });
    }

    pub fn sync_root_folder(&self) {
        self.state.write().invalidate_folder_hierarchy();
        BridgeFolders::sync_root(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }

    pub fn select_folder(&self, folder_id: FolderId, path: PathBuf) {
        BridgeFolders::select_folder(
            folder_id,
            path,
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }

    pub fn refresh_folder_explicit(&self, folder_id: FolderId, path: PathBuf) {
        BridgeFolders::refresh_folder_explicit(
            folder_id,
            path,
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }

    /// Closes every window and releases the native resources, see `ui::shutdown`. The caller has claimed the shutdown with
    /// `shutdown::begin`. UI thread only.
    pub fn shutdown(&self) {
        super::shutdown::run(&ShutdownTargets {
            window: &self.window,
            settings: &self.settings,
            thumbnails: &self.thumbnails,
            state: &self.state,
            settings_dialog: &self.settings_dialog,
            fullscreen: &self.fullscreen,
            tab_manager: &self.tab_manager,
            file_rename_dialog: &self.rename_dialog,
            favorite_rename_dialog: &self.favorite_rename_dialog,
            missing_file_dialog: &self.missing_file_dialog,
            delete_file_dialog: &self.delete_file_dialog,
            remove_orphaned_dialog: &self.remove_orphaned_dialog,
            conflict_dialog: &self.conflict_dialog,
            tag_dialogs: &self.tag_dialogs,
            folder_dialogs: &self.folder_dialogs,
        });
    }

    pub fn reload_folders(&self) {
        BridgeFolders::reload_tree(&self.window, &self.library, &self.settings, &self.state);
    }

    pub fn reload_favorites(&self) {
        BridgeFavorites::reload(&self.window, &self.favorites);
    }

    pub fn reload_media_view(&self) {
        self.reload_media_view_filtered();
    }

    pub fn reload_media_view_full(&self, start_pipeline: bool) {
        BridgeMedia::reload(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
            start_pipeline,
            true,
        );
    }

    pub fn reload_media_view_filtered(&self) {
        BridgeMedia::reload(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
            false,
            false,
        );
    }

    pub fn handle_media_click(&self, media_id: MediaId, is_ctrl: bool, is_shift: bool) {
        BridgeFileSelection::handle_click(
            super::bridge_file_selection::FileClickEvent {
                media_id,
                is_ctrl,
                is_shift,
            },
            FileClickContext {
                window: &self.window,
                library: &self.library,
                settings: &self.settings,
                tab_manager: &self.tab_manager,
                state: &self.state,
                last_click: &self.last_click,
                fullscreen: &self.fullscreen,
            },
        );
    }

    pub fn select_all(&self) {
        BridgeFileSelection::select_all(&self.window, &self.library, &self.state);
    }

    pub fn clear_selection(&self) {
        BridgeFileSelection::clear_selection(&self.window, &self.library, &self.state);
    }

    pub fn update_search_query(&self, query: String) {
        self.state.write().filter.search_query = query;
        self.reload_media_view_filtered();
    }

    pub fn update_inspector(&self) {
        BridgeInspector::update(&self.window, &self.state);
    }

    pub fn show_media_context_menu(&self, media_id: MediaId, x: f32, y: f32) {
        BridgeContextMenu::show_media_menu(
            MediaMenuContext {
                window: &self.window,
                library: &self.library,
                thumbnails: &self.thumbnails,
                tab_manager: &self.tab_manager,
                state: &self.state,
                settings: &self.settings,
            },
            media_id,
            x,
            y,
        );
    }

    pub fn reload_tags(&self) {
        BridgeTags::reload_tags(&self.window, &self.library, &self.state);
    }

    pub fn filter_tags_view(&self, filter_text: &str) {
        BridgeTags::filter_tags(&self.window, &self.library, &self.state, filter_text);
    }

    pub fn toggle_tag(&self, tag_id: TagId) {
        BridgeTags::toggle_tag(
            tag_id,
            &TagActionContext {
                window: &self.window,
                library: &self.library,
                thumbnails: &self.thumbnails,
                tab_manager: &self.tab_manager,
                settings: &self.settings,
                state: &self.state,
            },
        );
    }

    pub fn apply_rating(&self, rating: Option<u8>) {
        BridgeTags::apply_rating(
            rating,
            &TagActionContext {
                window: &self.window,
                library: &self.library,
                thumbnails: &self.thumbnails,
                tab_manager: &self.tab_manager,
                settings: &self.settings,
                state: &self.state,
            },
        );
    }

    pub fn apply_rating_to_ids(&self, ids: &std::collections::HashSet<MediaId>, rating: u8) {
        BridgeTags::apply_rating_to_ids(
            ids,
            rating,
            &TagActionContext {
                window: &self.window,
                library: &self.library,
                thumbnails: &self.thumbnails,
                tab_manager: &self.tab_manager,
                settings: &self.settings,
                state: &self.state,
            },
        );
    }
}
