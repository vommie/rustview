use crate::ctx_menu::CtxMenuDispatcher;
use crate::ui::browse_folders::BrowseDragScroller;
use crate::ui::context_menu::{BridgeContextMenu, MediaMenuContext};
use crate::ui::favorites::BridgeFavorites;
use crate::ui::favorites::manager::FavoriteManager;
use crate::ui::favorites::rename_dialog::FavoriteRenameDialogController;
use crate::ui::file_dialogs::conflict::FileOpsConflictDialogController;
use crate::ui::file_dialogs::delete::DeleteFileDialogController;
use crate::ui::file_dialogs::missing::MissingFileDialogController;
use crate::ui::file_dialogs::remove_orphaned::RemoveOrphanedFileDialogController;
use crate::ui::folder_dialogs::FolderDialogController;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::folder_tree::dnd_autoscroller::DndFolderTreeAutoScroller;
use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::media::thumbnails::BridgeThumbnails;
use crate::ui::multiview::handle::{AppContext, KEEP_VIEW};
use crate::ui::multiview::tab_host;
use crate::ui::multiview::{MultiviewHandle, bar_overlay};
use crate::ui::selection::rubberband::RubberbandAutoScroller;
use crate::ui::selection::{BridgeFileSelection, FileClickContext, LastClickRecord};
use crate::ui::settings_dialog::SettingsDialogController;
use crate::ui::shutdown::ShutdownTargets;
use crate::ui::sidebar::BridgeSidebarPanelTabs;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tags::dialogs::TagDialogController;
use crate::ui::tags::history_bridge::BridgeTagHistory;
use crate::ui::tags::{BridgeTags, TagActionContext};
use crate::ui::thumbnail_badges::ThumbnailBadgeBridge;
use crate::ui::type_ahead::{self};
use crate::ui::{FileRenameDialog, MainWindow};
use crate::viewer::ViewerTabManager;
use crate::viewer::multiview::prefs;
use crate::viewer::zoom::prefs as zoom_prefs;
use parking_lot::{Mutex, RwLock};
use rustview_core::library::Library;
use rustview_core::library::models::{FolderId, MediaId, TagId};
use rustview_core::query::{MediaTypeFilter, ScopeMode, SortColumn, SortOrder};
use rustview_core::settings::{GridAspectRatio, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{ComponentHandle, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

mod actions;
mod pointer_callbacks;
mod settings_subscriber;
mod subsystem_callbacks;
mod tab_grid_callbacks;

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
        zoom_prefs::apply(&app_settings.fullscreen);
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
        tab_manager.set_main_viewport_info(0, 64, initial_win_sz.width.max(800), initial_win_sz.height.max(600));

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
        tab_manager.set_event_sink(crate::ui::multiview::event_sink(fullscreen.clone()));

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

        let thumbnail_timer = BridgeThumbnails::start_drain_timer(thumbnails.clone(), state.clone(), window.as_weak());

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
        BridgeFolders::reload_tree(&bridge.window, &bridge.library, &bridge.settings, &bridge.state);

        BridgeTagHistory::init(&bridge.window, &bridge.library, &bridge.settings, &bridge.state);

        let w_weak = bridge.window.as_weak();
        let lib = bridge.library.clone();
        let thumb = bridge.thumbnails.clone();
        let set = bridge.settings.clone();
        let st = bridge.state.clone();
        let tm_post = bridge.tab_manager.clone();

        Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(win) = w_weak.upgrade() {
                let sz = win.window().size();
                tm_post.set_main_viewport_info(0, 64, sz.width.max(800), sz.height.max(600));
                BridgeFolders::sync_root(&win, &lib, &thumb, &set, &st);
                BridgeMedia::update_virtual_folder_button_state(&win, &lib, &st);
                StatusBarBridge::update_metrics(&win, &st);
            }
        });

        bridge
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    pub fn tab_manager(&self) -> &Arc<ViewerTabManager> {
        &self.tab_manager
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
        crate::ui::shutdown::run(&ShutdownTargets {
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
            crate::ui::selection::FileClickEvent {
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
