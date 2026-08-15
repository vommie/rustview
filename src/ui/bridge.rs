use super::favorites::FavoriteManager;
use super::file_selection::{FileSelectionModel, FileSelectionSummary};
use super::models::{format_filesize, rgb8_to_shared_buffer, rgb8_to_slint_image};
use super::{
    FavoriteItemView, FileRenameDialog, FolderNodeView, MainWindow, MediaItemView, TagNodeView,
};
use crate::ctx_menu::{CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher};
use crate::library::Library;
use crate::library::models::{FolderId, MediaId, MediaRecord, TagId};
use crate::player::events::format_playback_time;
use crate::player::{Player, PlayerController, PlayerEvent};
use crate::query::filters::IndexedMediaRecord;
use crate::query::{MatchMode, MediaSorter, QueryFilter, ScopeMode, SortColumn, SortOrder};
use crate::settings::{AppSettings, SettingsDialogController, SettingsManager};
use crate::thumbnails::{PreparedTarget, THUMBNAIL_SIZE, ThumbnailManager};
use parking_lot::RwLock;
use slint::{
    CloseRequestResponse, ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, SharedPixelBuffer,
    Timer, VecModel,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

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

pub struct UiBridge {
    window: MainWindow,
    library: Arc<Library>,
    thumbnails: Arc<ThumbnailManager>,
    player: Arc<PlayerController>,
    settings: Arc<SettingsManager>,
    settings_dialog: Rc<RefCell<SettingsDialogController>>,
    rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    favorites: Arc<RwLock<FavoriteManager>>,
    ctx_menu_dispatcher: Arc<CtxMenuDispatcher>,
    state: Arc<RwLock<UiState>>,
    _thumbnail_timer: Timer,
    _player_timer: Timer,
}

impl UiBridge {
    pub fn new(
        window: MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        player: Arc<PlayerController>,
        settings: Arc<SettingsManager>,
        config_dir: &Path,
    ) -> Self {
        let favorites = Arc::new(RwLock::new(FavoriteManager::new(config_dir)));

        let mut controller = SettingsDialogController::new(settings.clone());
        controller.set_main_window(window.as_weak());
        let settings_dialog = Rc::new(RefCell::new(controller));
        let rename_dialog = Rc::new(RefCell::new(None));

        let state = Arc::new(RwLock::new(UiState {
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
        }));

        let ctx_menu_dispatcher = Arc::new(CtxMenuDispatcher::new());
        let thumbnail_timer = Timer::default();
        let player_timer = Timer::default();

        let bridge = Self {
            window,
            library,
            thumbnails,
            player,
            settings,
            settings_dialog,
            rename_dialog,
            favorites,
            ctx_menu_dispatcher,
            state,
            _thumbnail_timer: thumbnail_timer,
            _player_timer: player_timer,
        };

        bridge.register_callbacks();
        bridge.register_settings_subscriber();
        bridge.start_thumbnail_drain_timer();
        bridge.start_player_event_timer();
        bridge.sync_root_folder();
        bridge.reload_favorites();
        bridge.reload_tags();
        bridge
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    fn start_thumbnail_drain_timer(&self) {
        let w_weak = self.window.as_weak();
        let state = self.state.clone();
        let thumbnails = self.thumbnails.clone();

        self._thumbnail_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(30),
            move || {
                let Some(window) = w_weak.upgrade() else {
                    return;
                };

                let batch = thumbnails.drain_ready_batch(64);
                if !batch.is_empty() {
                    let mut st = state.write();
                    let media_model = window.get_media_items();

                    for item_data in batch {
                        let shared_buf = rgb8_to_shared_buffer(
                            &item_data.rgb8_pixels,
                            THUMBNAIL_SIZE,
                            THUMBNAIL_SIZE,
                        );

                        st.folder_thumbnails
                            .insert(item_data.media_id, shared_buf.clone());

                        if let Some(&row_idx) = st.media_row_map.get(&item_data.media_id)
                            && let Some(mut item) = media_model.row_data(row_idx)
                        {
                            item.thumbnail = Image::from_rgb8(shared_buf);
                            media_model.set_row_data(row_idx, item);
                        }
                    }
                }

                let progress = thumbnails.progress();
                window.set_is_preloading(progress.is_preloading);
                window.set_preload_current(progress.preload_current as i32);
                window.set_preload_total(progress.preload_total as i32);

                window.set_is_generating(progress.is_generating);
                window.set_generate_current(progress.generate_current as i32);
                window.set_generate_total(progress.generate_total as i32);
                window.set_active_workers(progress.active_workers as i32);
            },
        );
    }

    pub fn sync_root_folder(&self) {
        Self::sync_root_folder_internal(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }

    fn sync_root_folder_internal(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let s = settings.get();
        window.set_thumb_size(s.appearance.default_thumbnail_size as i32);

        if let Some(ref root_path) = s.general.root_folder {
            if root_path.is_dir() {
                tracing::info!(root = ?root_path, "Synchronizing root directory...");

                let folder_record = match library.folders().get_folder_by_path(root_path) {
                    Ok(Some(f)) => f,
                    _ => match library.folders().add_folder(root_path) {
                        Ok(f) => f,
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to register root directory");
                            return;
                        }
                    },
                };

                if let Ok(entries) = std::fs::read_dir(root_path) {
                    for entry in entries.flatten() {
                        if let Ok(file_type) = entry.file_type()
                            && file_type.is_dir()
                        {
                            let sub_path = entry.path();
                            if library
                                .folders()
                                .get_folder_by_path(&sub_path)
                                .unwrap_or(None)
                                .is_none()
                            {
                                let _ = library.folders().add_folder(&sub_path);
                            }
                        }
                    }
                }

                let target_folder = if s.general.remember_last_folder
                    && let Some(ref last_path) = s.general.last_folder_path
                    && last_path.is_dir()
                    && last_path.starts_with(root_path)
                    && let Ok(Some(f)) = library.folders().get_folder_by_path(last_path)
                {
                    (f.id, last_path.clone())
                } else {
                    (folder_record.id, root_path.clone())
                };

                let _ = library.refresher().refresh_folder(target_folder.0);

                let raw_media = library
                    .media()
                    .get_by_folder(target_folder.0)
                    .unwrap_or_default();
                let indexed_media =
                    Self::preindex_media_records(library, raw_media, &target_folder.1);

                {
                    let mut st = state.write();
                    st.current_folder_id = Some(target_folder.0);
                    st.current_folder_path = Some(target_folder.1);
                    st.all_folder_media = indexed_media;
                    st.folder_thumbnails.clear();
                    st.active_indices.clear();
                    st.file_selection.clear();
                    st.last_executed_query.clear();
                }

                window.set_status_text("Ready.".into());

                Self::reload_folders_internal(window, library, settings, state);
                Self::reload_media_view_internal(window, library, thumbnails, state, true, false);
                Self::update_inspector_internal(window, state);
            } else {
                Self::clear_library_view_internal(
                    window,
                    state,
                    "Configured Root Directory does not exist on disk.",
                );
            }
        } else {
            Self::clear_library_view_internal(
                window,
                state,
                "No Root Directory configured. Open Settings to select a root folder.",
            );
        }
    }

    fn preindex_media_records(
        library: &Library,
        raw_records: Vec<MediaRecord>,
        folder_path: &Path,
    ) -> Vec<IndexedMediaRecord> {
        raw_records
            .into_iter()
            .enumerate()
            .map(|(index, r)| {
                let filesize_str = format_filesize(r.file_size).into();
                let dimensions_str = format!("{} × {}", r.width, r.height).into();
                let filename_slint = r.filename.clone().into();
                let filename_lower = r.filename.to_lowercase();
                let full_path = folder_path.join(&r.filename);
                let has_tags = !library
                    .tags()
                    .get_tags_for_media(r.id)
                    .unwrap_or_default()
                    .is_empty();

                IndexedMediaRecord {
                    index,
                    record: r,
                    full_path,
                    filename_lower,
                    filename_slint,
                    filesize_str,
                    dimensions_str,
                    has_tags,
                }
            })
            .collect()
    }

    fn clear_library_view_internal(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        status_msg: &str,
    ) {
        {
            let mut st = state.write();
            st.current_folder_id = None;
            st.current_folder_path = None;
            st.all_folder_media.clear();
            st.folder_thumbnails.clear();
            st.active_indices.clear();
            st.file_selection.clear();
            st.media_row_map.clear();
            st.last_executed_query.clear();
        }
        window.set_folders(ModelRc::new(VecModel::from(vec![])));
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_items_metric_text("Items: 0".into());
        window.set_status_text(status_msg.into());
        Self::update_inspector_internal(window, state);
    }

    fn register_settings_subscriber(&self) {
        let w_weak = self.window.as_weak();
        let library = self.library.clone();
        let thumbnails = self.thumbnails.clone();
        let settings = self.settings.clone();
        let state = self.state.clone();

        self.settings.subscribe(move |_new_settings: &AppSettings| {
            let library = library.clone();
            let thumbnails = thumbnails.clone();
            let settings = settings.clone();
            let state = state.clone();

            let _ = w_weak.upgrade_in_event_loop(move |window| {
                Self::sync_root_folder_internal(&window, &library, &thumbnails, &settings, &state);
            });
        });
    }

    fn register_callbacks(&self) {
        let library = self.library.clone();
        let state = self.state.clone();
        let window_weak = self.window.as_weak();
        let bridge_self = self.clone_handle();

        // REFRESH ACTION
        let lib_clone = library.clone();
        let state_clone = state.clone();
        let w_weak = window_weak;
        let bridge_refresh = self.clone_handle();
        self.window.on_refresh_folder(move || {
            let folder_id = state_clone.read().current_folder_id;
            if let Some(id) = folder_id {
                let _ = lib_clone.refresher().refresh_folder(id);
                bridge_refresh.reload_media_view_full(true);
                if let Some(w) = w_weak.upgrade() {
                    w.set_status_text("Folder refreshed.".into());
                }
            }
        });

        // OPEN SETTINGS WINDOW
        let dialog = self.settings_dialog.clone();
        self.window.on_open_settings(move || {
            dialog.borrow_mut().show();
        });

        // FOLDER SELECTION
        let bridge_folder = self.clone_handle();
        self.window.on_folder_selected(move |id, path| {
            bridge_folder.select_folder(id as u64, PathBuf::from(path.as_str()));
        });

        // FAVORITE SELECTION
        let bridge_fav = self.clone_handle();
        self.window.on_favorite_selected(move |path_str| {
            let path = PathBuf::from(path_str.as_str());
            if let Ok(Some(folder)) = bridge_fav.library.folders().get_folder_by_path(&path) {
                bridge_fav.select_folder(folder.id, path);
            }
        });

        // FILE SELECTION CLICK
        let bridge_media = self.clone_handle();
        self.window.on_media_clicked(move |id, is_ctrl, is_shift| {
            bridge_media.handle_media_click(id as u64, is_ctrl, is_shift);
        });

        // MEDIA DOUBLE CLICK
        let bridge_dbl = self.clone_handle();
        self.window.on_media_double_clicked(move |id| {
            bridge_dbl.open_media_item(id as u64);
        });

        // CONTEXT MENU TRIGGER (RIGHT-CLICK)
        let bridge_ctx = self.clone_handle();
        self.window.on_media_context_menu(move |id, x, y| {
            bridge_ctx.show_media_context_menu(id as u64, x, y);
        });

        // CONTEXT MENU ITEM CLICKED
        let bridge_ctx_click = self.clone_handle();
        self.window.on_context_menu_item_clicked(move |action_id| {
            bridge_ctx_click.handle_context_menu_action(action_id as u64);
        });

        // CONTEXT MENU DISMISSED
        let bridge_ctx_dismiss = self.clone_handle();
        self.window.on_context_menu_dismissed(move || {
            bridge_ctx_dismiss.window.set_is_context_menu_open(false);
        });

        // SEARCH QUERY
        let bridge_search = self.clone_handle();
        self.window.on_search_query_changed(move |query| {
            bridge_search.update_search_query(query.to_string());
        });

        // REGEX MODE TOGGLE
        let bridge_regex = self.clone_handle();
        self.window.on_regex_mode_toggled(move |enabled| {
            bridge_regex.state.write().filter.is_regex = enabled;
            bridge_regex.reload_media_view_filtered();
        });

        // RATING APPLICATION
        let bridge_rate = self.clone_handle();
        self.window.on_rating_applied(move |rating| {
            bridge_rate.apply_rating(if rating < 0 { None } else { Some(rating as u8) });
        });

        // TAG TOGGLE
        let bridge_tag = self.clone_handle();
        self.window.on_tag_toggled(move |tag_id| {
            bridge_tag.toggle_tag(tag_id as u32);
        });

        // TAG MODE TOGGLE
        let bridge_tag_mode = self.clone_handle();
        self.window.on_tag_mode_toggled(move |is_assign| {
            bridge_tag_mode.state.write().is_assign_mode = is_assign;
            bridge_tag_mode.reload_tags();
        });

        // MATCH MODE
        let bridge_match = self.clone_handle();
        self.window.on_match_mode_changed(move |idx| {
            let mode = if idx == 0 {
                MatchMode::All
            } else {
                MatchMode::Any
            };
            bridge_match.state.write().filter.match_mode = mode;
            bridge_match.reload_media_view_filtered();
        });

        // SCOPE MODE
        let bridge_scope = self.clone_handle();
        self.window.on_scope_mode_changed(move |idx| {
            let scope = if idx == 0 {
                ScopeMode::Folder
            } else {
                ScopeMode::Global
            };
            bridge_scope.state.write().filter.scope_mode = scope;
            bridge_scope.reload_media_view_filtered();
        });

        // TAG FILTER
        let bridge_filter = self.clone_handle();
        self.window.on_tag_filter_changed(move |query| {
            bridge_filter.filter_tags_view(&query);
        });

        // PLAYER CONTROLS
        let p_clone = self.player.clone();
        self.window.on_player_toggle_play(move || {
            let is_paused = p_clone.raw_mpv().is_paused();
            if is_paused {
                let _ = p_clone.play();
            } else {
                let _ = p_clone.pause();
            }
        });

        let p_clone = self.player.clone();
        self.window.on_player_seek(move |pos| {
            let _ = p_clone.seek(pos as f64);
        });

        let p_clone = self.player.clone();
        self.window.on_player_set_volume(move |vol| {
            let _ = p_clone.set_volume(vol as f64);
        });

        let p_clone = self.player.clone();
        self.window.on_player_stop(move || {
            let _ = p_clone.stop();
        });

        let bridge_close = bridge_self;
        self.window.on_player_close(move || {
            let _ = bridge_close.player.stop();
            bridge_close.window.set_is_player_open(false);
        });
    }

    fn start_player_event_timer(&self) {
        let player = self.player.clone();
        let window_weak = self.window.as_weak();

        self._player_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(50),
            move || {
                for event in player.poll_events() {
                    if let Some(w) = window_weak.upgrade() {
                        match event {
                            PlayerEvent::PositionChanged {
                                current_time,
                                duration,
                            } => {
                                w.set_player_current_time(current_time as f32);
                                w.set_player_duration(duration as f32);
                                w.set_player_time_str(
                                    format!(
                                        "{} / {}",
                                        format_playback_time(current_time),
                                        format_playback_time(duration)
                                    )
                                    .into(),
                                );
                            }
                            PlayerEvent::PlaybackStateChanged { is_paused } => {
                                w.set_player_is_playing(!is_paused);
                            }
                            PlayerEvent::PlaybackEnded => {
                                w.set_player_is_playing(false);
                            }
                            _ => {}
                        }
                    }
                }
            },
        );
    }

    fn clone_handle(&self) -> Self {
        Self {
            window: self.window.clone_strong(),
            library: self.library.clone(),
            thumbnails: self.thumbnails.clone(),
            player: self.player.clone(),
            settings: self.settings.clone(),
            settings_dialog: self.settings_dialog.clone(),
            rename_dialog: self.rename_dialog.clone(),
            favorites: self.favorites.clone(),
            ctx_menu_dispatcher: self.ctx_menu_dispatcher.clone(),
            state: self.state.clone(),
            _thumbnail_timer: Timer::default(),
            _player_timer: Timer::default(),
        }
    }

    pub fn select_folder(&self, folder_id: FolderId, path: PathBuf) {
        tracing::info!(folder_id = folder_id, path = ?path, "User selected folder in sidebar");

        match self.library.refresher().refresh_folder(folder_id) {
            Ok(report) => {
                tracing::info!(
                    folder_id = folder_id,
                    scanned = report.total_scanned,
                    new = report.new_media,
                    updated = report.updated_media,
                    unchanged = report.unchanged_media,
                    deleted = report.deleted_media,
                    "Folder scan complete"
                );
            }
            Err(e) => {
                tracing::error!(folder_id = folder_id, error = %e, "Failed to scan folder");
            }
        }

        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type()
                    && file_type.is_dir()
                {
                    let sub_path = entry.path();
                    if self
                        .library
                        .folders()
                        .get_folder_by_path(&sub_path)
                        .unwrap_or(None)
                        .is_none()
                    {
                        let _ = self.library.folders().add_folder(&sub_path);
                    }
                }
            }
        }

        let raw_media = self
            .library
            .media()
            .get_by_folder(folder_id)
            .unwrap_or_default();
        let indexed_media = Self::preindex_media_records(&self.library, raw_media, &path);

        tracing::info!(
            folder_id = folder_id,
            count = indexed_media.len(),
            "Cached preformatted folder media records into RAM."
        );

        {
            let mut state = self.state.write();
            state.current_folder_id = Some(folder_id);
            state.current_folder_path = Some(path.clone());
            state.all_folder_media = indexed_media;
            state.folder_thumbnails.clear();
            state.active_indices.clear();
            state.file_selection.clear();
            state.last_executed_query.clear();
        }

        {
            let mut s = self.settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(path);
                self.settings.save_quiet(&s);
            }
        }

        self.reload_folders();
        self.reload_media_view_full(true);
        self.update_inspector();
    }

    pub fn open_media_item(&self, media_id: MediaId) {
        if let Ok(Some(media)) = self.library.media().get(media_id) {
            let full_path = self
                .state
                .read()
                .current_folder_path
                .as_ref()
                .map(|p| p.join(&media.filename))
                .unwrap_or_default();

            tracing::info!(media_id = media_id, file = ?full_path, "Opening media item");

            if matches!(media.media_type, crate::media::MediaType::Video)
                && self.player.load(&full_path, true).is_ok()
            {
                self.window.set_player_title(media.filename.into());
                self.window.set_is_player_open(true);
                self.window.set_player_is_playing(true);
            }
        }
    }

    /// Prepares and opens the dynamic context menu at the specified cursor position.
    pub fn show_media_context_menu(&self, media_id: MediaId, x: f32, y: f32) {
        let (selected_count, is_single, can_rename) = {
            let mut st = self.state.write();
            if !st.file_selection.is_selected(media_id) {
                st.file_selection.select_single(media_id);
            }
            let count = st.file_selection.count();
            (count, count == 1, count == 1)
        };

        self.update_inspector();
        self.reload_media_view_filtered();

        let menu = CtxMenuBuilder::new()
            .header(if is_single {
                "Selected File"
            } else {
                "Selected Files"
            })
            .action(
                CTX_ACTION_RENAME,
                if can_rename {
                    "Rename...".to_string()
                } else {
                    format!("Rename (Single File Only, {} selected)", selected_count)
                },
                Some("F2"),
                Some("edit"),
                can_rename,
            )
            .separator()
            .submenu("Rate Selection", Some("star"), selected_count > 0, |sub| {
                sub.action(CTX_ACTION_RATE_5, "5 Stars (★★★★★)", None, None, true)
                    .action(CTX_ACTION_RATE_4, "4 Stars (★★★★☆)", None, None, true)
                    .action(CTX_ACTION_RATE_3, "3 Stars (★★★☆☆)", None, None, true)
                    .action(CTX_ACTION_RATE_2, "2 Stars (★★☆☆☆)", None, None, true)
                    .action(CTX_ACTION_RATE_1, "1 Star  (★☆☆☆☆)", None, None, true)
                    .separator()
                    .action(CTX_ACTION_RATE_0, "Clear Rating", None, None, true)
            })
            .separator()
            .action(
                CTX_ACTION_CLEAR_SELECTION,
                "Clear Selection",
                Some("Esc"),
                None,
                selected_count > 0,
            )
            .action(
                CTX_ACTION_SELECT_ALL,
                "Select All",
                Some("Ctrl+A"),
                None,
                true,
            )
            .build();

        let (top_model, sub_model) = CtxMenuDispatcher::to_slint_models(&menu);

        self.window.set_context_menu_items(top_model);
        self.window.set_context_submenu_items(sub_model);
        self.window.set_context_menu_x(x);
        self.window.set_context_menu_y(y);
        self.window.set_is_context_menu_open(true);

        tracing::debug!(
            target_media_id = media_id,
            pos_x = x,
            pos_y = y,
            selected_items = selected_count,
            "Context menu opened"
        );
    }

    /// Dispatches and executes the invoked context menu action.
    pub fn handle_context_menu_action(&self, action_id: CtxMenuActionId) {
        self.window.set_is_context_menu_open(false);

        let selected_ids = self.state.read().file_selection.selected_ids().clone();
        let context = CtxMenuContext {
            position_x: 0.0,
            position_y: 0.0,
            selected_item_count: selected_ids.len(),
            target_ids: selected_ids.iter().copied().collect(),
        };

        tracing::info!(ctx_action_id = action_id, "Executing context menu action");

        match action_id {
            CTX_ACTION_RENAME => {
                if let Some(&target_id) = selected_ids.iter().next()
                    && selected_ids.len() == 1
                    && let Ok(Some(record)) = self.library.media().get(target_id)
                {
                    self.state.write().active_file_rename_target = Some(target_id);
                    self.show_rename_dialog(&record.filename);
                }
            }
            CTX_ACTION_SELECT_ALL => {
                self.select_all();
            }
            CTX_ACTION_CLEAR_SELECTION => {
                self.clear_selection();
            }
            CTX_ACTION_RATE_5 => self.apply_rating_to_ids(&selected_ids, 5),
            CTX_ACTION_RATE_4 => self.apply_rating_to_ids(&selected_ids, 4),
            CTX_ACTION_RATE_3 => self.apply_rating_to_ids(&selected_ids, 3),
            CTX_ACTION_RATE_2 => self.apply_rating_to_ids(&selected_ids, 2),
            CTX_ACTION_RATE_1 => self.apply_rating_to_ids(&selected_ids, 1),
            CTX_ACTION_RATE_0 => self.apply_rating_to_ids(&selected_ids, 0),
            _ => {
                self.ctx_menu_dispatcher.dispatch(action_id, &context);
            }
        }
    }

    /// Prepares and displays the standalone modal FileRenameDialog window.
    pub fn show_rename_dialog(&self, current_name: &str) {
        let mut dialog_ref = self.rename_dialog.borrow_mut();
        if dialog_ref.is_none() {
            let win = FileRenameDialog::new().expect("Failed to instantiate FileRenameDialog");

            let bridge_confirm = self.clone_handle();
            win.on_confirmed(move |new_name| {
                bridge_confirm.execute_file_rename(&new_name);
            });

            let bridge_cancel = self.clone_handle();
            win.on_cancelled(move || {
                bridge_cancel.close_rename_dialog();
            });

            let bridge_close = self.clone_handle();
            win.window().on_close_requested(move || {
                bridge_close.close_rename_dialog();
                CloseRequestResponse::HideWindow
            });

            *dialog_ref = Some(win);
        }

        if let Some(ref win) = *dialog_ref {
            win.set_initial_name(current_name.into());
            win.set_new_name(current_name.into());
            win.set_error_message("".into());
            self.window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the FileRenameDialog window and unlocks the main window.
    pub fn close_rename_dialog(&self) {
        if let Some(ref win) = *self.rename_dialog.borrow() {
            let _ = win.hide();
        }
        self.state.write().active_file_rename_target = None;
        self.window.set_is_modal_locked(false);
    }

    /// Applies a rating directly to a specific set of media IDs.
    fn apply_rating_to_ids(&self, ids: &std::collections::HashSet<MediaId>, rating: u8) {
        for &id in ids {
            let _ = self.library.media().set_rating(id, rating);
        }
        let current_folder = self.state.read().current_folder_id;
        let current_path = self.state.read().current_folder_path.clone();
        if let (Some(f_id), Some(path)) = (current_folder, current_path) {
            let raw = self.library.media().get_by_folder(f_id).unwrap_or_default();
            self.state.write().all_folder_media =
                Self::preindex_media_records(&self.library, raw, &path);
        }
        self.update_inspector();
        self.reload_media_view_filtered();
    }

    /// Executes the physical and database file rename operation.
    pub fn execute_file_rename(&self, new_name: &str) {
        let target_id = self.state.read().active_file_rename_target;

        if let Some(media_id) = target_id {
            match self
                .library
                .file_operations()
                .rename_media(media_id, new_name)
            {
                Ok(updated_record) => {
                    tracing::info!(
                        media_id = media_id,
                        new_name = %new_name,
                        "File successfully renamed via FileOperations subsystem"
                    );

                    // Update in-memory preindexed media cache
                    {
                        let mut st = self.state.write();
                        let folder_path_opt = st.current_folder_path.clone();
                        if let Some(item) = st
                            .all_folder_media
                            .iter_mut()
                            .find(|m| m.record.id == media_id)
                        {
                            item.filename_slint = updated_record.filename.clone().into();
                            item.filename_lower = updated_record.filename.to_lowercase();
                            if let Some(ref folder_path) = folder_path_opt {
                                item.full_path = folder_path.join(&updated_record.filename);
                            }
                            item.record = updated_record;
                        }
                    }

                    self.close_rename_dialog();
                    self.window
                        .set_status_text(format!("Renamed file to '{}'", new_name).into());

                    self.update_inspector();
                    self.reload_media_view_filtered();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to rename media file");
                    if let Some(ref win) = *self.rename_dialog.borrow() {
                        win.set_error_message(e.to_string().into());
                    }
                }
            }
        }
    }

    pub fn reload_folders(&self) {
        Self::reload_folders_internal(&self.window, &self.library, &self.settings, &self.state);
    }

    fn reload_folders_internal(
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let root_setting = settings.get().general.root_folder;
        if let Some(ref root_dir) = root_setting {
            let folders = library.folders().list_folders().unwrap_or_default();
            let current_sel = state.read().current_folder_id;

            let mut matching_folders: Vec<_> = folders
                .into_iter()
                .filter(|f| Path::new(&f.path).starts_with(root_dir))
                .collect();

            matching_folders.sort_by(|a, b| a.path.cmp(&b.path));

            let folder_views: Vec<FolderNodeView> = matching_folders
                .into_iter()
                .map(|f| {
                    let p = Path::new(&f.path);
                    let depth = p
                        .strip_prefix(root_dir)
                        .map(|rel| rel.components().count())
                        .unwrap_or(0);

                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| f.path.clone());

                    FolderNodeView {
                        id: f.id as i32,
                        name: name.into(),
                        path: f.path.into(),
                        depth: depth as i32,
                        is_expanded: true,
                        is_selected: current_sel == Some(f.id),
                    }
                })
                .collect();
            window.set_folders(ModelRc::new(VecModel::from(folder_views)));
        } else {
            window.set_folders(ModelRc::new(VecModel::from(vec![])));
        }
    }

    pub fn reload_favorites(&self) {
        let favs = self.favorites.read().list();
        let views: Vec<FavoriteItemView> = favs
            .into_iter()
            .map(|(alias, path, valid)| FavoriteItemView {
                alias: alias.into(),
                path: path.to_string_lossy().to_string().into(),
                is_valid: valid,
                is_selected: false,
            })
            .collect();
        self.window
            .set_favorites(ModelRc::new(VecModel::from(views)));
    }

    pub fn reload_tags(&self) {
        let tags = self.library.tags().list_all_tags().unwrap_or_default();
        let views: Vec<TagNodeView> = tags
            .into_iter()
            .map(|t| TagNodeView {
                id: t.id as i32,
                name: t.name.into(),
                depth: if t.parent_id.is_some() { 1 } else { 0 },
                is_selected: false,
                is_partial: false,
                is_match: false,
            })
            .collect();
        self.window.set_tags(ModelRc::new(VecModel::from(views)));
    }

    pub fn filter_tags_view(&self, filter_text: &str) {
        let filter_lower = filter_text.to_lowercase();
        let tags = self.library.tags().list_all_tags().unwrap_or_default();
        let selected_tags = self.state.read().filter.tag_ids.clone();

        let views: Vec<TagNodeView> = tags
            .into_iter()
            .map(|t| {
                let is_match =
                    !filter_lower.is_empty() && t.name.to_lowercase().contains(&filter_lower);
                let is_selected = selected_tags.contains(&t.id);
                TagNodeView {
                    id: t.id as i32,
                    name: t.name.into(),
                    depth: if t.parent_id.is_some() { 1 } else { 0 },
                    is_selected,
                    is_partial: false,
                    is_match,
                }
            })
            .collect();
        self.window.set_tags(ModelRc::new(VecModel::from(views)));
    }

    pub fn reload_media_view(&self) {
        self.reload_media_view_filtered();
    }

    pub fn reload_media_view_full(&self, start_pipeline: bool) {
        Self::reload_media_view_internal(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.state,
            start_pipeline,
            true,
        );
    }

    pub fn reload_media_view_filtered(&self) {
        Self::reload_media_view_internal(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.state,
            false,
            false,
        );
    }

    fn reload_media_view_internal(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
        start_pipeline: bool,
        reload_lmdb: bool,
    ) {
        let t_total_start = Instant::now();

        let (folder_id, filter, sort_col, sort_ord, last_query) = {
            let s = state.read();
            (
                s.current_folder_id,
                s.filter.clone(),
                s.sort_column,
                s.sort_order,
                s.last_executed_query.clone(),
            )
        };

        if let Some(f_id) = folder_id {
            if reload_lmdb {
                let path_opt = state.read().current_folder_path.clone();
                if let Some(ref p) = path_opt {
                    let raw_media = library.media().get_by_folder(f_id).unwrap_or_default();
                    let indexed = Self::preindex_media_records(library, raw_media, p);
                    state.write().all_folder_media = indexed;
                }
            }

            if start_pipeline {
                thumbnails.cancel_pipeline();
            }

            let current_query = filter.search_query.trim().to_string();
            let is_full_reset = filter.is_empty();

            let is_refinement = !current_query.is_empty()
                && !last_query.is_empty()
                && current_query.starts_with(&last_query);

            let st = state.read();
            let total_folder_count = st.all_folder_media.len();

            let subset = if is_refinement && !st.active_indices.is_empty() {
                Some(st.active_indices.as_slice())
            } else {
                None
            };

            let t_filter_start = Instant::now();
            let filtered_refs = filter.execute_indexed_refs(library, &st.all_folder_media, subset);
            let filter_us = t_filter_start.elapsed().as_micros();

            let t_sort_start = Instant::now();
            let mut sorted_refs = filtered_refs;

            if !is_full_reset || reload_lmdb {
                MediaSorter::sort_indexed_refs(&mut sorted_refs, sort_col, sort_ord);
            }
            let sort_us = t_sort_start.elapsed().as_micros();

            let t_map_start = Instant::now();
            let matched_len = sorted_refs.len();
            let mut item_views = Vec::with_capacity(matched_len);
            let mut row_map = HashMap::with_capacity(matched_len);
            let mut pipeline_targets = if start_pipeline {
                Vec::with_capacity(matched_len)
            } else {
                Vec::new()
            };

            let arena = thumbnails.cache().arena();
            let mut matched_indices = Vec::with_capacity(matched_len);

            for (row_idx, &item) in sorted_refs.iter().enumerate() {
                matched_indices.push(item.index);
                let is_selected = st.file_selection.is_selected(item.record.id);
                row_map.insert(item.record.id, row_idx);

                if start_pipeline {
                    pipeline_targets.push(PreparedTarget {
                        media_id: item.record.id,
                        file_path: item.full_path.clone(),
                    });
                }

                let thumbnail_img = if let Some(buf) = st.folder_thumbnails.get(&item.record.id) {
                    Image::from_rgb8(buf.clone())
                } else if let Some(handle) = thumbnails.cache().get(item.record.id) {
                    if let Some(rgb_data) = arena.read_slot(handle) {
                        rgb8_to_slint_image(rgb_data, THUMBNAIL_SIZE, THUMBNAIL_SIZE)
                    } else {
                        Image::default()
                    }
                } else {
                    Image::default()
                };

                item_views.push(MediaItemView {
                    id: item.record.id as i32,
                    filename: item.filename_slint.clone(),
                    filesize_str: item.filesize_str.clone(),
                    dimensions_str: item.dimensions_str.clone(),
                    rating: item.record.rating as i32,
                    has_tags: item.has_tags,
                    thumbnail: thumbnail_img,
                    is_selected,
                    is_video: matches!(item.record.media_type, crate::media::MediaType::Video),
                });
            }

            drop(st);

            let map_us = t_map_start.elapsed().as_micros();

            {
                let mut st_write = state.write();
                st_write.media_row_map = row_map;
                st_write.active_indices = matched_indices;
                st_write.last_executed_query = current_query.clone();
            }

            let t_slint_start = Instant::now();
            let matched_count = item_views.len();
            window.set_media_items(ModelRc::new(VecModel::from(item_views)));
            window.set_items_metric_text(
                format!("Items: {} / {}", matched_count, total_folder_count).into(),
            );
            let slint_us = t_slint_start.elapsed().as_micros();

            let total_us = t_total_start.elapsed().as_micros();

            tracing::debug!(
                query = %current_query,
                is_reset = is_full_reset,
                matched = matched_count,
                filter_us = filter_us,
                sort_us = sort_us,
                model_mapping_us = map_us,
                slint_commit_us = slint_us,
                total_pipeline_us = total_us,
                "[ViewBenchmark: Zero-Copy] View update pipeline completed"
            );

            if start_pipeline {
                thumbnails.start_pipeline(f_id, pipeline_targets);
            }
        } else {
            window.set_media_items(ModelRc::new(VecModel::from(vec![])));
            window.set_items_metric_text("Items: 0".into());
        }
    }

    /// Centralized modifier-aware click handler for single and range file selection.
    pub fn handle_media_click(&self, media_id: MediaId, is_ctrl: bool, is_shift: bool) {
        let t0 = Instant::now();
        {
            let mut state = self.state.write();

            let visible_ids: Vec<MediaId> = if !state.active_indices.is_empty() {
                state
                    .active_indices
                    .iter()
                    .filter_map(|&idx| state.all_folder_media.get(idx))
                    .map(|item| item.record.id)
                    .collect()
            } else {
                state
                    .all_folder_media
                    .iter()
                    .map(|item| item.record.id)
                    .collect()
            };

            if is_shift {
                let additive = is_ctrl;
                state
                    .file_selection
                    .select_range(media_id, &visible_ids, additive);
                tracing::info!(
                    target_media_id = media_id,
                    is_ctrl = is_ctrl,
                    is_shift = is_shift,
                    selected_count = state.file_selection.count(),
                    elapsed_us = t0.elapsed().as_micros(),
                    "[FileSelection: Shift-Range] Contiguous range selection executed"
                );
            } else if is_ctrl {
                state.file_selection.toggle(media_id);
                tracing::info!(
                    target_media_id = media_id,
                    is_ctrl = is_ctrl,
                    selected_count = state.file_selection.count(),
                    is_now_selected = state.file_selection.is_selected(media_id),
                    elapsed_us = t0.elapsed().as_micros(),
                    "[FileSelection: Ctrl-Toggle] Individual item selection toggled"
                );
            } else {
                state.file_selection.select_single(media_id);
                tracing::info!(
                    target_media_id = media_id,
                    selected_count = 1,
                    elapsed_us = t0.elapsed().as_micros(),
                    "[FileSelection: Single] Single item selected (previous selection cleared)"
                );
            }
        }

        self.update_inspector();
        self.reload_media_view_filtered();
    }

    pub fn update_search_query(&self, query: String) {
        self.state.write().filter.search_query = query;
        self.reload_media_view_filtered();
    }

    pub fn apply_rating(&self, rating: Option<u8>) {
        let (selected_ids, is_assign) = {
            let s = self.state.read();
            (s.file_selection.selected_ids().clone(), s.is_assign_mode)
        };

        if is_assign {
            if let Some(r) = rating {
                for id in selected_ids {
                    let _ = self.library.media().set_rating(id, r);
                }
                let current_folder = self.state.read().current_folder_id;
                let current_path = self.state.read().current_folder_path.clone();
                if let (Some(f_id), Some(path)) = (current_folder, current_path) {
                    let raw = self.library.media().get_by_folder(f_id).unwrap_or_default();
                    self.state.write().all_folder_media =
                        Self::preindex_media_records(&self.library, raw, &path);
                }
            }
        } else {
            self.state.write().filter.rating = rating;
        }
        self.update_inspector();
        self.reload_media_view_filtered();
    }

    pub fn toggle_tag(&self, tag_id: TagId) {
        let (selected_ids, is_assign) = {
            let s = self.state.read();
            (s.file_selection.selected_ids().clone(), s.is_assign_mode)
        };

        if is_assign {
            for id in selected_ids {
                let _ = self.library.tags().assign_tag(id, tag_id);
            }
            let current_folder = self.state.read().current_folder_id;
            let current_path = self.state.read().current_folder_path.clone();
            if let (Some(f_id), Some(path)) = (current_folder, current_path) {
                let raw = self.library.media().get_by_folder(f_id).unwrap_or_default();
                self.state.write().all_folder_media =
                    Self::preindex_media_records(&self.library, raw, &path);
            }
        } else {
            let mut state = self.state.write();
            if state.filter.tag_ids.contains(&tag_id) {
                state.filter.tag_ids.remove(&tag_id);
            } else {
                state.filter.tag_ids.insert(tag_id);
            }
        }
        self.update_inspector();
        self.reload_media_view_filtered();
    }

    pub fn update_inspector(&self) {
        Self::update_inspector_internal(&self.window, &self.state);
    }

    fn update_inspector_internal(window: &MainWindow, state: &Arc<RwLock<UiState>>) {
        let st = state.read();
        let selected_ids = st.file_selection.selected_ids();

        if selected_ids.is_empty() {
            window.set_info_filename("- (No selection)".into());
            window.set_info_size("-".into());
            window.set_info_dimensions("-".into());
            window.set_info_format("-".into());
            window.set_info_hash("-".into());
            window.set_info_selection_count(0);
            return;
        }

        let selected_records: Vec<MediaRecord> = st
            .all_folder_media
            .iter()
            .filter(|m| selected_ids.contains(&m.record.id))
            .map(|m| m.record.clone())
            .collect();

        let summary = FileSelectionSummary::from_records(&selected_records);
        window.set_info_selection_count(summary.count as i32);

        if summary.count == 1 {
            if let Some(single) = summary.single_record {
                window.set_info_filename(single.filename.clone().into());
                window.set_info_size(format_filesize(single.file_size).into());
                window.set_info_dimensions(format!("{} × {}", single.width, single.height).into());
                window.set_info_format(format!("{:?}", single.format).into());
                window.set_info_hash(format!("{:x?}", &single.file_hash[..8]).into());
            }
        } else {
            window.set_info_filename(format!("{} multiple files selected", summary.count).into());
            window.set_info_size(
                format!("{} (total)", format_filesize(summary.total_size_bytes)).into(),
            );

            let dim_str = if summary.is_uniform_dimensions {
                if let Some((w, h)) = summary.min_dimensions {
                    format!("{} × {} (Uniform)", w, h)
                } else {
                    "-".to_string()
                }
            } else if let (Some(min), Some(max)) = (summary.min_dimensions, summary.max_dimensions)
            {
                format!("{} × {} – {} × {}", min.0, min.1, max.0, max.1)
            } else {
                "-".to_string()
            };
            window.set_info_dimensions(dim_str.into());

            let format_str = summary
                .format_breakdown
                .iter()
                .map(|(fmt, count)| format!("{:?} ({})", fmt, count))
                .collect::<Vec<_>>()
                .join(", ");
            window.set_info_format(format_str.into());

            let detail_str = if summary.rated_count > 0 {
                format!(
                    "Avg Rating: {:.1} ★ ({} rated)",
                    summary.avg_rating, summary.rated_count
                )
            } else {
                "— (Multi-selection)".to_string()
            };
            window.set_info_hash(detail_str.into());
        }
    }

    pub fn selected_media_ids(&self) -> Vec<MediaId> {
        self.state
            .read()
            .file_selection
            .selected_ids()
            .iter()
            .copied()
            .collect()
    }

    pub fn selected_media_records(&self) -> Vec<MediaRecord> {
        let state = self.state.read();
        let sel = state.file_selection.selected_ids();
        state
            .all_folder_media
            .iter()
            .filter(|m| sel.contains(&m.record.id))
            .map(|m| m.record.clone())
            .collect()
    }

    pub fn select_all(&self) {
        {
            let mut state = self.state.write();
            let visible_ids: Vec<MediaId> = if !state.active_indices.is_empty() {
                state
                    .active_indices
                    .iter()
                    .filter_map(|&idx| state.all_folder_media.get(idx))
                    .map(|m| m.record.id)
                    .collect()
            } else {
                state.all_folder_media.iter().map(|m| m.record.id).collect()
            };
            state.file_selection.select_all(&visible_ids);
        }
        self.update_inspector();
        self.reload_media_view_filtered();
    }

    pub fn clear_selection(&self) {
        self.state.write().file_selection.clear();
        self.update_inspector();
        self.reload_media_view_filtered();
    }
}
