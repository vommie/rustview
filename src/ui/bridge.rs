use super::favorites::FavoriteManager;
use super::models::{format_filesize, rgb8_to_slint_image};
use crate::library::Library;
use crate::library::models::{FolderId, MediaId, MediaRecord, TagId};
use crate::player::events::format_playback_time;
use crate::player::{Player, PlayerController, PlayerEvent};
use crate::query::{MatchMode, MediaSorter, QueryFilter, ScopeMode, SortColumn, SortOrder};
use crate::thumbnails::{THUMBNAIL_SIZE, ThumbnailManager};
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

slint::include_modules!();

pub struct UiState {
    pub current_folder_id: Option<FolderId>,
    pub current_folder_path: Option<PathBuf>,
    pub active_media: Vec<MediaRecord>,
    pub selected_media_ids: HashSet<MediaId>,
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
    favorites: Arc<RwLock<FavoriteManager>>,
    state: Arc<RwLock<UiState>>,
}

impl UiBridge {
    pub fn new(
        window: MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        player: Arc<PlayerController>,
        config_dir: &Path,
    ) -> Self {
        let favorites = Arc::new(RwLock::new(FavoriteManager::new(config_dir)));
        let state = Arc::new(RwLock::new(UiState {
            current_folder_id: None,
            current_folder_path: None,
            active_media: Vec::new(),
            selected_media_ids: HashSet::new(),
            filter: QueryFilter::default(),
            sort_column: SortColumn::Filename,
            sort_order: SortOrder::Ascending,
            is_assign_mode: false,
        }));

        let bridge = Self {
            window,
            library,
            thumbnails,
            player,
            favorites,
            state,
        };

        bridge.register_callbacks();
        bridge.start_player_event_timer();
        bridge.reload_folders();
        bridge.reload_favorites();
        bridge.reload_tags();
        bridge
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    fn register_callbacks(&self) {
        let library = self.library.clone();
        let state = self.state.clone();
        let window_weak = self.window.as_weak();

        // REFRESH ACTION
        let lib_clone = library.clone();
        let state_clone = state.clone();
        let w_weak = window_weak;
        self.window.on_refresh_folder(move || {
            let folder_id = state_clone.read().current_folder_id;
            if let Some(id) = folder_id {
                let _ = lib_clone.refresher().refresh_folder(id);
                if let Some(w) = w_weak.upgrade() {
                    w.set_status_text("Folder refreshed.".into());
                }
            }
        });

        // FOLDER SELECTION
        let bridge_self = self.clone_handle();
        self.window.on_folder_selected(move |id, path| {
            bridge_self.select_folder(id as u64, PathBuf::from(path.as_str()));
        });

        // FAVORITE SELECTION
        let bridge_self = self.clone_handle();
        self.window.on_favorite_selected(move |path_str| {
            let path = PathBuf::from(path_str.as_str());
            if let Ok(Some(folder)) = bridge_self.library.folders().get_folder_by_path(&path) {
                bridge_self.select_folder(folder.id, path);
            }
        });

        // MEDIA SELECTION
        let bridge_self = self.clone_handle();
        self.window.on_media_clicked(move |id| {
            bridge_self.toggle_media_selection(id as u64);
        });

        // MEDIA DOUBLE CLICK
        let bridge_self = self.clone_handle();
        self.window.on_media_double_clicked(move |id| {
            bridge_self.open_media_item(id as u64);
        });

        // SEARCH QUERY
        let bridge_self = self.clone_handle();
        self.window.on_search_query_changed(move |query| {
            bridge_self.update_search_query(query.to_string());
        });

        // REGEX MODE TOGGLE
        let bridge_self = self.clone_handle();
        self.window.on_regex_mode_toggled(move |enabled| {
            bridge_self.state.write().filter.is_regex = enabled;
            bridge_self.reload_media_view();
        });

        // RATING APPLICATION
        let bridge_self = self.clone_handle();
        self.window.on_rating_applied(move |rating| {
            bridge_self.apply_rating(if rating < 0 { None } else { Some(rating as u8) });
        });

        // TAG TOGGLE
        let bridge_self = self.clone_handle();
        self.window.on_tag_toggled(move |tag_id| {
            bridge_self.toggle_tag(tag_id as u32);
        });

        // TAG MODE TOGGLE
        let bridge_self = self.clone_handle();
        self.window.on_tag_mode_toggled(move |is_assign| {
            bridge_self.state.write().is_assign_mode = is_assign;
            bridge_self.reload_tags();
        });

        // MATCH MODE
        let bridge_self = self.clone_handle();
        self.window.on_match_mode_changed(move |idx| {
            let mode = if idx == 0 {
                MatchMode::All
            } else {
                MatchMode::Any
            };
            bridge_self.state.write().filter.match_mode = mode;
            bridge_self.reload_media_view();
        });

        // SCOPE MODE
        let bridge_self = self.clone_handle();
        self.window.on_scope_mode_changed(move |idx| {
            let scope = if idx == 0 {
                ScopeMode::Folder
            } else {
                ScopeMode::Global
            };
            bridge_self.state.write().filter.scope_mode = scope;
            bridge_self.reload_media_view();
        });

        // TAG FILTER
        let bridge_self = self.clone_handle();
        self.window.on_tag_filter_changed(move |query| {
            bridge_self.filter_tags_view(&query);
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

        let bridge_self = self.clone_handle();
        self.window.on_player_close(move || {
            let _ = bridge_self.player.stop();
            bridge_self.window.set_is_player_open(false);
        });
    }

    fn start_player_event_timer(&self) {
        let player = self.player.clone();
        let window_weak = self.window.as_weak();

        let timer = slint::Timer::default();
        timer.start(
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
            favorites: self.favorites.clone(),
            state: self.state.clone(),
        }
    }

    pub fn select_folder(&self, folder_id: FolderId, path: PathBuf) {
        {
            let mut state = self.state.write();
            state.current_folder_id = Some(folder_id);
            state.current_folder_path = Some(path);
            state.selected_media_ids.clear();
        }
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
        let folders = self.library.folders().list_folders().unwrap_or_default();
        let folder_views: Vec<FolderNodeView> = folders
            .into_iter()
            .map(|f| FolderNodeView {
                id: f.id as i32,
                name: Path::new(&f.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| f.path.clone())
                    .into(),
                path: f.path.into(),
                depth: 0,
                is_expanded: false,
                is_selected: false,
            })
            .collect();

        self.window
            .set_folders(ModelRc::new(VecModel::from(folder_views)));
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
        let (folder_id, filter, sort_col, sort_ord, selected_ids) = {
            let s = self.state.read();
            (
                s.current_folder_id,
                s.filter.clone(),
                s.sort_column,
                s.sort_order,
                s.selected_media_ids.clone(),
            )
        };

        if let Some(f_id) = folder_id {
            let raw_media = self.library.media().get_by_folder(f_id).unwrap_or_default();

            // 1. Filter
            let filtered = filter.execute(&self.library, &raw_media);

            // 2. Sort
            let mut sorted = filtered;
            MediaSorter::sort(&mut sorted, sort_col, sort_ord);

            // 3. Build Slint Views with Thumbnails
            let mut item_views = Vec::with_capacity(sorted.len());
            for media in &sorted {
                let is_selected = selected_ids.contains(&media.id);
                let full_path = self
                    .state
                    .read()
                    .current_folder_path
                    .as_ref()
                    .map(|p| p.join(&media.filename))
                    .unwrap_or_default();

                // Retrieve or generate thumbnail
                let thumbnail_img =
                    if let Ok(handle) = self.thumbnails.get_or_load_sync(media.id, &full_path) {
                        let arena = self.thumbnails.cache().arena();
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
                    dimensions_str: format!("{}x{}", media.width, media.height).into(),
                    rating: media.rating as i32,
                    has_tags: !self
                        .library
                        .tags()
                        .get_tags_for_media(media.id)
                        .unwrap_or_default()
                        .is_empty(),
                    thumbnail: thumbnail_img,
                    is_selected,
                    is_video: matches!(media.media_type, crate::media::MediaType::Video),
                });
            }

            self.window
                .set_media_items(ModelRc::new(VecModel::from(item_views)));
            self.window.set_items_metric_text(
                format!("Items: {} / {}", sorted.len(), raw_media.len()).into(),
            );
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
