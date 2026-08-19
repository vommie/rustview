use super::bridge_ctx_menu::{BridgeContextMenu, ContextMenuRegistration};
use super::bridge_dnd::{BatchTransferParams, BridgeDnd, DragStartParams};
use super::bridge_favorites::BridgeFavorites;
use super::bridge_file_dialog_conflict::FileOpsConflictDialogController;
use super::bridge_file_dialog_missing::MissingFileDialogController;
use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_folder_dialogs::FolderDialogController;
use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_tag_dialogs::TagDialogController;
use super::bridge_tags::BridgeTags;
use super::bridge_thumbnails::BridgeThumbnails;
use super::dnd_foldertree_autoscroller::DndFolderTreeAutoScroller;
use super::favorites::FavoriteManager;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::CtxMenuDispatcher;
use crate::library::models::{FileTransferKind, FolderId, MediaId, TagId};
use crate::library::Library;
use crate::query::{ScopeMode, SortColumn, SortOrder};
use crate::settings::{
    AppSettings, GridAspectRatio, LogObscurer, SettingsDialogController, SettingsDiff,
    SettingsManager,
};
use crate::thumbnails::ThumbnailManager;
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub struct UiBridge {
    window: MainWindow,
    library: Arc<Library>,
    thumbnails: Arc<ThumbnailManager>,
    settings: Arc<SettingsManager>,
    settings_dialog: Rc<RefCell<SettingsDialogController>>,
    rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    missing_file_dialog: MissingFileDialogController,
    conflict_dialog: FileOpsConflictDialogController,
    dnd_foldertree_autoscroller: DndFolderTreeAutoScroller,
    tag_dialogs: TagDialogController,
    folder_dialogs: FolderDialogController,
    favorites: Arc<RwLock<FavoriteManager>>,
    ctx_menu_dispatcher: Arc<CtxMenuDispatcher>,
    state: Arc<RwLock<UiState>>,
    last_click: Arc<Mutex<Option<(MediaId, Instant)>>>,
    _thumbnail_timer: Timer,
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

        let mut controller = SettingsDialogController::new(settings.clone());
        controller.set_main_window(window.as_weak());
        let settings_dialog = Rc::new(RefCell::new(controller));
        let rename_dialog = Rc::new(RefCell::new(None));
        let missing_file_dialog = MissingFileDialogController::new();
        let conflict_dialog = FileOpsConflictDialogController::new();
        let dnd_foldertree_autoscroller = DndFolderTreeAutoScroller::new();
        let tag_dialogs = TagDialogController::new();
        let folder_dialogs = FolderDialogController::new();

        let mut initial_state = UiState::default();
        let restored_expanded_tags = settings.get().tags.expanded_tag_ids;
        initial_state.expanded_tag_ids = restored_expanded_tags.into_iter().collect();

        let restored_expanded_folders = if settings.get().folders.persist_folder_tree_state {
            settings.get().folders.expanded_folder_ids
        } else {
            Vec::new()
        };
        initial_state.expanded_folder_ids = restored_expanded_folders.into_iter().collect();

        let v_settings = settings.get().view;
        window.set_is_table_view(v_settings.is_table_view);

        // Restore Sidebar Widths from Persistent Settings
        window.set_sidebar_left_width(v_settings.sidebar_left_width);
        window.set_sidebar_right_width(v_settings.sidebar_right_width);

        let initial_sort_col = SortColumn::from_name(&v_settings.sort_column);
        let initial_sort_ord = SortOrder::from_name(&v_settings.sort_order);
        initial_state.sort_column = initial_sort_col;
        initial_state.sort_order = initial_sort_ord;

        window.set_sort_column_idx(initial_sort_col.to_index());
        window.set_sort_order_idx(initial_sort_ord.to_index());

        window.set_col_filename_width(v_settings.col_filename_width);
        window.set_col_dimensions_width(v_settings.col_dimensions_width);
        window.set_col_size_width(v_settings.col_size_width);
        window.set_col_rating_width(v_settings.col_rating_width);

        // Apply centralized appearance & tooltip settings
        let app_settings = settings.get();
        window.set_thumb_size(app_settings.appearance.default_thumbnail_size as i32);
        let ratio = GridAspectRatio::from_str_name(&app_settings.appearance.thumbnail_aspect_ratio);
        window.set_thumb_aspect_idx(ratio.to_index());
        window.set_enable_tooltips(app_settings.appearance.enable_tooltips);
        window.set_tooltip_delay_ms(app_settings.appearance.tooltip_delay_ms as i32);
        window.set_show_hidden_folders(app_settings.folders.show_hidden_folders);
        window.set_status_text("Initializing RustView...".into());

        // Initialize Tag & Rate panel properties (Default Scope: Global / Index 2)
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

        let thumbnail_timer = BridgeThumbnails::start_drain_timer(
            thumbnails.clone(),
            state.clone(),
            window.as_weak(),
        );

        let bridge = Self {
            window,
            library,
            thumbnails,
            settings,
            settings_dialog,
            rename_dialog,
            missing_file_dialog,
            conflict_dialog,
            dnd_foldertree_autoscroller,
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

        bridge.reload_favorites();
        bridge.reload_tags();
        BridgeFolders::reload_tree(
            &bridge.window,
            &bridge.library,
            &bridge.settings,
            &bridge.state,
        );

        // Defer root folder synchronization until Slint has entered the event loop and painted the initial window
        let w_weak = bridge.window.as_weak();
        let lib = bridge.library.clone();
        let thumb = bridge.thumbnails.clone();
        let set = bridge.settings.clone();
        let st = bridge.state.clone();

        Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(win) = w_weak.upgrade() {
                tracing::info!(
                    "[UiBridge: PostMount] Slint UI event loop ready. Mounting root folder..."
                );
                BridgeFolders::sync_root(&win, &lib, &thumb, &set, &st);
            }
        });

        tracing::info!(
            is_table = v_settings.is_table_view,
            sidebar_left_width = v_settings.sidebar_left_width,
            sidebar_right_width = v_settings.sidebar_right_width,
            sort_col = ?initial_sort_col,
            sort_ord = ?initial_sort_ord,
            thumb_size = app_settings.appearance.default_thumbnail_size,
            aspect_ratio = %app_settings.appearance.thumbnail_aspect_ratio,
            show_hidden_folders = app_settings.folders.show_hidden_folders,
            support_dot_hidden = app_settings.folders.support_dot_hidden_files,
            enable_tooltips = app_settings.appearance.enable_tooltips,
            tooltip_delay_ms = app_settings.appearance.tooltip_delay_ms,
            "[UiBridge] Initialized all UI bridges, scheduled deferred initial mount"
        );

        bridge
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    /// Registers the intelligent delta-based settings subscriber.
    fn register_settings_subscriber(&self) {
        let w_weak = self.window.as_weak();
        let library = self.library.clone();
        let thumbnails = self.thumbnails.clone();
        let settings = self.settings.clone();
        let state = self.state.clone();

        self.settings.subscribe(move |_old_settings: &AppSettings, new_settings: &AppSettings, diff: &SettingsDiff| {
            if !diff.has_changes {
                tracing::debug!("[UiBridge: SettingsSubscriber] No functional changes in settings diff. Bypassing UI updates.");
                return;
            }

            tracing::info!(
                root_changed = diff.root_folder_changed,
                tree_presentation_changed = diff.folder_tree_presentation_changed,
                grid_appearance_changed = diff.appearance_grid_changed,
                tooltips_changed = diff.tooltip_settings_changed,
                pipeline_changed = diff.performance_pipeline_changed,
                debug_changed = diff.debug_settings_changed,
                "[UiBridge: SettingsSubscriber] Granular settings diff received. Dispatching targeted updates..."
            );

            let library = library.clone();
            let thumbnails = thumbnails.clone();
            let settings = settings.clone();
            let state = state.clone();
            let diff_clone = diff.clone();
            let new_clone = new_settings.clone();

            let _ = w_weak.upgrade_in_event_loop(move |window| {
                let t_start = Instant::now();

                // 1. Tooltips update
                if diff_clone.tooltip_settings_changed {
                    window.set_enable_tooltips(new_clone.appearance.enable_tooltips);
                    window.set_tooltip_delay_ms(new_clone.appearance.tooltip_delay_ms as i32);
                }

                // 2. Appearance & Grid Presentation
                if diff_clone.appearance_grid_changed {
                    window.set_thumb_size(new_clone.appearance.default_thumbnail_size as i32);
                    let ratio = GridAspectRatio::from_str_name(&new_clone.appearance.thumbnail_aspect_ratio);
                    window.set_thumb_aspect_idx(ratio.to_index());
                }

                // 3. Debug & Privacy update
                if diff_clone.debug_settings_changed {
                    LogObscurer::set_obfuscate(new_clone.debug.obfuscate_logs);
                }

                // 4. Folder Tree Presentation
                if diff_clone.folder_tree_presentation_changed && !diff_clone.root_folder_changed {
                    window.set_show_hidden_folders(new_clone.folders.show_hidden_folders);
                    BridgeFolders::reload_tree(&window, &library, &settings, &state);
                }

                // 5. Root Folder Changed
                if diff_clone.root_folder_changed {
                    state.write().invalidate_folder_tree_cache();
                    BridgeFolders::sync_root(&window, &library, &thumbnails, &settings, &state);
                }

                tracing::debug!(
                    elapsed_us = t_start.elapsed().as_micros(),
                    "[UiBridge: SettingsUpdate] Targeted UI settings update completed successfully"
                );
            });
        });
    }

    fn register_subsystem_callbacks(&self) {
        BridgeFolders::register_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            self.state.clone(),
            self.settings_dialog.clone(),
        );

        BridgeFileSelection::register_callbacks(
            &self.window,
            self.library.clone(),
            self.state.clone(),
            self.last_click.clone(),
        );

        BridgeContextMenu::register_callbacks(
            &self.window,
            ContextMenuRegistration {
                library: self.library.clone(),
                thumbnails: self.thumbnails.clone(),
                state: self.state.clone(),
                dispatcher: self.ctx_menu_dispatcher.clone(),
                rename_dialog: self.rename_dialog.clone(),
                missing_file_dialog: self.missing_file_dialog.clone(),
                tag_dialogs: self.tag_dialogs.clone(),
                folder_dialogs: self.folder_dialogs.clone(),
                settings: self.settings.clone(),
            },
        );

        BridgeTags::register_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            self.state.clone(),
        );

        BridgeMedia::register_query_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            self.state.clone(),
        );

        self.register_dnd_callbacks();
        self.register_sidebar_resize_callbacks();
    }

    fn register_dnd_callbacks(&self) {
        let win_start = self.window.clone_strong();
        let lib_start = self.library.clone();
        let thumb_start = self.thumbnails.clone();
        let st_start = self.state.clone();
        let autoscroll_start = self.dnd_foldertree_autoscroller.clone();
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
            });
        });

        let win_move = self.window.clone_strong();
        let lib_move = self.library.clone();
        let st_move = self.state.clone();
        let autoscroll_move = self.dnd_foldertree_autoscroller.clone();
        self.window.on_media_drag_moved(move |x, y| {
            BridgeDnd::handle_drag_moved(
                x,
                y,
                &win_move,
                &lib_move,
                &st_move,
                &autoscroll_move,
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
        self.window.on_media_drag_ended(move || {
            BridgeDnd::handle_drag_ended(
                &win_end,
                &lib_end,
                &st_end,
                &autoscroll_end,
            );
        });

        // Floating Choice Popup Callbacks: Copy, Move, Cancel, Dismissed
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
            tracing::info!("[DndChoicePopup] User clicked Cancel. Drop aborted, selection preserved.");
            win_cancel.set_is_choice_popup_open(false);
        });

        let win_dism = self.window.clone_strong();
        self.window.on_choice_popup_dismissed(move || {
            tracing::info!("[DndChoicePopup] Popup dismissed. Drop aborted, selection preserved.");
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
            let t0 = Instant::now();
            let width_f32 = final_width;
            let mut app_settings = set_left.get();
            app_settings.view.sidebar_left_width = width_f32;
            set_left.save_quiet(&app_settings);
            let elapsed_us = t0.elapsed().as_micros();

            tracing::info!(
                sidebar = "left",
                final_width = width_f32,
                save_time_us = elapsed_us,
                "[Benchmark: SidebarResizeCommit] Left sidebar width persisted to disk settings"
            );
        });

        let last_right_resize = Arc::new(Mutex::new(Instant::now()));
        let right_events_count = Arc::new(AtomicUsize::new(0));

        let last_right_clone = last_right_resize;
        let right_count_clone = right_events_count;

        self.window.on_sidebar_right_resized(move |new_width| {
            let t0 = Instant::now();
            let count = right_count_clone.fetch_add(1, Ordering::Relaxed) + 1;

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

        let set_right = self.settings.clone();
        self.window.on_sidebar_right_resize_finished(move |final_width| {
            let t0 = Instant::now();
            let width_f32 = final_width;
            let mut app_settings = set_right.get();
            app_settings.view.sidebar_right_width = width_f32;
            set_right.save_quiet(&app_settings);
            let elapsed_us = t0.elapsed().as_micros();

            tracing::info!(
                sidebar = "right",
                final_width = width_f32,
                save_time_us = elapsed_us,
                "[Benchmark: SidebarResizeCommit] Right sidebar width persisted to disk settings"
            );
        });
    }

    pub fn sync_root_folder(&self) {
        self.state.write().invalidate_folder_tree_cache();
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
            &self.window,
            &self.library,
            &self.state,
            &self.last_click,
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
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.state,
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
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }

    pub fn apply_rating(&self, rating: Option<u8>) {
        BridgeTags::apply_rating(
            rating,
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }

    pub fn apply_rating_to_ids(&self, ids: &std::collections::HashSet<MediaId>, rating: u8) {
        BridgeTags::apply_rating_to_ids(
            ids,
            rating,
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }
}
