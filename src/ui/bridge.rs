use super::favorites::FavoriteManager;
use super::models::{format_filesize, rgb8_to_slint_image};
use super::{FavoriteItemView, FolderNodeView, MainWindow, MediaItemView, TagNodeView};
use crate::library::Library;
use crate::library::models::{FolderId, MediaId, MediaRecord, TagId};
use crate::player::events::format_playback_time;
use crate::player::{Player, PlayerController, PlayerEvent};
use crate::query::{MatchMode, MediaSorter, QueryFilter, ScopeMode, SortColumn, SortOrder};
use crate::settings::{AppSettings, SettingsDialogController, SettingsManager};
use crate::thumbnails::{PreparedTarget, THUMBNAIL_SIZE, ThumbnailManager};
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, Model, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

pub struct UiState {
    pub current_folder_id: Option<FolderId>,
    pub current_folder_path: Option<PathBuf>,
    pub active_media: Vec<MediaRecord>,
    pub selected_media_ids: HashSet<MediaId>,
    pub media_row_map: HashMap<MediaId, usize>,
    pub filter: QueryFilter,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    pub is_assign_mode: bool,
}

pub struct UiBridge {
    window: MainWindow,
    library: Arc<Library>,
    thumbnails: Arc<ThumbnailManager>,
    player: Arc<PlayerController>,
    settings: Arc<SettingsManager>,
    settings_dialog: Rc<RefCell<SettingsDialogController>>,
    favorites: Arc<RwLock<FavoriteManager>>,
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

        let state = Arc::new(RwLock::new(UiState {
            current_folder_id: None,
            current_folder_path: None,
            active_media: Vec::new(),
            selected_media_ids: HashSet::new(),
            media_row_map: HashMap::new(),
            filter: QueryFilter::default(),
            sort_column: SortColumn::Filename,
            sort_order: SortOrder::Ascending,
            is_assign_mode: false,
        }));

        let thumbnail_timer = Timer::default();
        let player_timer = Timer::default();

        let bridge = Self {
            window,
            library,
            thumbnails,
            player,
            settings,
            settings_dialog,
            favorites,
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

    /// High-frequency batched UI updater running on the Slint main thread (zero lag, 60fps guaranteed)
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

                // 1. Drain ready thumbnail batch (up to 64 items per frame tick)
                let batch = thumbnails.drain_ready_batch(64);
                if !batch.is_empty() {
                    let st = state.read();
                    let media_model = window.get_media_items();

                    for item_data in batch {
                        if let Some(&row_idx) = st.media_row_map.get(&item_data.media_id)
                            && let Some(mut item) = media_model.row_data(row_idx)
                        {
                            item.thumbnail = rgb8_to_slint_image(
                                &item_data.rgb8_pixels,
                                THUMBNAIL_SIZE,
                                THUMBNAIL_SIZE,
                            );
                            media_model.set_row_data(row_idx, item);
                        }
                    }
                }

                // 2. Sync Progress Snapshot with UI Status Bar
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

                {
                    let mut st = state.write();
                    st.current_folder_id = Some(target_folder.0);
                    st.current_folder_path = Some(target_folder.1);
                    st.selected_media_ids.clear();
                }

                window.set_status_text("Ready.".into());

                Self::reload_folders_internal(window, library, settings, state);
                Self::reload_media_view_internal(window, library, thumbnails, settings, state);
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

    fn clear_library_view_internal(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        status_msg: &str,
    ) {
        {
            let mut st = state.write();
            st.current_folder_id = None;
            st.current_folder_path = None;
            st.active_media.clear();
            st.selected_media_ids.clear();
            st.media_row_map.clear();
        }
        window.set_folders(ModelRc::new(VecModel::from(vec![])));
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_items_metric_text("Items: 0".into());
        window.set_status_text(status_msg.into());
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
                bridge_refresh.reload_media_view();
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

        // MEDIA SELECTION
        let bridge_media = self.clone_handle();
        self.window.on_media_clicked(move |id| {
            bridge_media.toggle_media_selection(id as u64);
        });

        // MEDIA DOUBLE CLICK
        let bridge_dbl = self.clone_handle();
        self.window.on_media_double_clicked(move |id| {
            bridge_dbl.open_media_item(id as u64);
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
            bridge_regex.reload_media_view();
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
            bridge_match.reload_media_view();
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
            bridge_scope.reload_media_view();
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
            favorites: self.favorites.clone(),
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

        {
            let mut state = self.state.write();
            state.current_folder_id = Some(folder_id);
            state.current_folder_path = Some(path.clone());
            state.selected_media_ids.clear();
        }

        {
            let mut s = self.settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(path);
                self.settings.save_quiet(&s);
            }
        }

        self.reload_folders();
        self.reload_media_view();
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
        Self::reload_media_view_internal(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
        );
    }

    fn reload_media_view_internal(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let root_setting = settings.get().general.root_folder;
        if root_setting.is_none() {
            window.set_media_items(ModelRc::new(VecModel::from(vec![])));
            window.set_items_metric_text("Items: 0".into());
            return;
        }

        let (folder_id, filter, sort_col, sort_ord, selected_ids, folder_path) = {
            let s = state.read();
            (
                s.current_folder_id,
                s.filter.clone(),
                s.sort_column,
                s.sort_order,
                s.selected_media_ids.clone(),
                s.current_folder_path.clone(),
            )
        };

        if let Some(f_id) = folder_id {
            thumbnails.cancel_pipeline();

            let raw_media = library.media().get_by_folder(f_id).unwrap_or_default();
            tracing::info!(
                folder_id = f_id,
                folder_path = ?folder_path,
                count = raw_media.len(),
                "Loaded media records from LMDB."
            );

            let filtered = filter.execute(library, &raw_media);
            let mut sorted = filtered;
            MediaSorter::sort(&mut sorted, sort_col, sort_ord);

            let mut item_views = Vec::with_capacity(sorted.len());
            let mut row_map = HashMap::with_capacity(sorted.len());
            let mut pipeline_targets = Vec::with_capacity(sorted.len());

            for (row_idx, media) in sorted.iter().enumerate() {
                let is_selected = selected_ids.contains(&media.id);
                let full_path = folder_path
                    .as_ref()
                    .map(|p| p.join(&media.filename))
                    .unwrap_or_default();

                row_map.insert(media.id, row_idx);
                pipeline_targets.push(PreparedTarget {
                    media_id: media.id,
                    file_path: full_path,
                });

                let thumbnail_img = if let Some(handle) = thumbnails.cache().get(media.id) {
                    let arena = thumbnails.cache().arena();
                    if let Some(rgb_data) = arena.read_slot(handle) {
                        rgb8_to_slint_image(rgb_data, THUMBNAIL_SIZE, THUMBNAIL_SIZE)
                    } else {
                        Image::default()
                    }
                } else {
                    Image::default()
                };

                item_views.push(MediaItemView {
                    id: media.id as i32,
                    filename: media.filename.clone().into(),
                    filesize_str: format_filesize(media.file_size).into(),
                    dimensions_str: format!("{} × {}", media.width, media.height).into(),
                    rating: media.rating as i32,
                    has_tags: !library
                        .tags()
                        .get_tags_for_media(media.id)
                        .unwrap_or_default()
                        .is_empty(),
                    thumbnail: thumbnail_img,
                    is_selected,
                    is_video: matches!(media.media_type, crate::media::MediaType::Video),
                });
            }

            {
                let mut st = state.write();
                st.media_row_map = row_map;
            }

            window.set_media_items(ModelRc::new(VecModel::from(item_views)));
            window.set_items_metric_text(
                format!("Items: {} / {}", sorted.len(), raw_media.len()).into(),
            );
            window.set_status_text(format!("Loaded {} media items.", sorted.len()).into());

            // Launch the 2-Phase Pipeline with Auto-Recovery
            thumbnails.start_pipeline(f_id, pipeline_targets);
        } else {
            window.set_media_items(ModelRc::new(VecModel::from(vec![])));
            window.set_items_metric_text("Items: 0".into());
        }
    }

    pub fn toggle_media_selection(&self, media_id: MediaId) {
        {
            let mut state = self.state.write();
            if state.selected_media_ids.contains(&media_id) {
                state.selected_media_ids.remove(&media_id);
            } else {
                state.selected_media_ids.insert(media_id);
            }
        }
        self.update_inspector();
        self.reload_media_view();
    }

    pub fn update_search_query(&self, query: String) {
        self.state.write().filter.search_query = query;
        self.reload_media_view();
    }

    pub fn apply_rating(&self, rating: Option<u8>) {
        let (selected_ids, is_assign) = {
            let s = self.state.read();
            (s.selected_media_ids.clone(), s.is_assign_mode)
        };

        if is_assign {
            if let Some(r) = rating {
                for id in selected_ids {
                    let _ = self.library.media().set_rating(id, r);
                }
            }
        } else {
            self.state.write().filter.rating = rating;
        }
        self.reload_media_view();
    }

    pub fn toggle_tag(&self, tag_id: TagId) {
        let (selected_ids, is_assign) = {
            let s = self.state.read();
            (s.selected_media_ids.clone(), s.is_assign_mode)
        };

        if is_assign {
            for id in selected_ids {
                let _ = self.library.tags().assign_tag(id, tag_id);
            }
        } else {
            let mut state = self.state.write();
            if state.filter.tag_ids.contains(&tag_id) {
                state.filter.tag_ids.remove(&tag_id);
            } else {
                state.filter.tag_ids.insert(tag_id);
            }
        }
        self.reload_media_view();
    }

    fn update_inspector(&self) {
        let selected_id = self.state.read().selected_media_ids.iter().next().copied();
        if let Some(id) = selected_id
            && let Ok(Some(media)) = self.library.media().get(id)
        {
            self.window.set_info_filename(media.filename.into());
            self.window
                .set_info_size(format_filesize(media.file_size).into());
            self.window
                .set_info_dimensions(format!("{} × {}", media.width, media.height).into());
            self.window
                .set_info_format(format!("{:?}", media.format).into());
            self.window
                .set_info_hash(format!("{:x?}", &media.file_hash[..8]).into());
        }
    }
}
