use super::bridge_context_menu::BridgeContextMenu;
use super::bridge_favorites::BridgeFavorites;
use super::bridge_file_selection::{BridgeFileSelection, FileClickEvent};
use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_player::BridgePlayer;
use super::bridge_state::*;
use super::bridge_tags::BridgeTags;
use super::bridge_thumbnails::BridgeThumbnails;
use super::favorites::FavoriteManager;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{CtxMenuActionId, CtxMenuDispatcher};
use crate::library::Library;
use crate::library::models::{FolderId, MediaId, MediaRecord, TagId};
use crate::player::{Player, PlayerController};
use crate::query::{MatchMode, ScopeMode};
use crate::settings::{AppSettings, SettingsDialogController, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle, Timer};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Central UI Registry and Facade coordinating specialized UI bridge subsystems.
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
    last_click: Arc<Mutex<Option<(MediaId, Instant)>>>,
    _thumbnail_timer: Timer,
    _player_timer: Timer,
}

impl UiBridge {
    /// Initializes all bridge subsystems, registers event subscribers, and restores view state.
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

        let state = Arc::new(RwLock::new(UiState::default()));
        let ctx_menu_dispatcher = Arc::new(CtxMenuDispatcher::new());
        let last_click = Arc::new(Mutex::new(None));

        // 1. Initialize Background Timers
        let thumbnail_timer = BridgeThumbnails::start_drain_timer(
            thumbnails.clone(),
            state.clone(),
            window.as_weak(),
        );

        let player_timer = BridgePlayer::start_event_timer(player.clone(), window.as_weak());

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
            last_click,
            _thumbnail_timer: thumbnail_timer,
            _player_timer: player_timer,
        };

        // 2. Wire Callbacks and Observers
        bridge.register_callbacks();
        bridge.register_settings_subscriber();

        // 3. Initial View Synchronization
        bridge.sync_root_folder();
        bridge.reload_favorites();
        bridge.reload_tags();

        bridge
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
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
                BridgeFolders::sync_root(&window, &library, &thumbnails, &settings, &state);
            });
        });
    }

    fn register_callbacks(&self) {
        self.register_folder_callbacks();
        self.register_media_callbacks();
        self.register_context_menu_callbacks();
        self.register_tag_callbacks();
        self.register_query_callbacks();
        BridgePlayer::register_callbacks(&self.window, self.player.clone());
    }

    fn register_folder_callbacks(&self) {
        let bridge = self.clone_handle();
        self.window.on_refresh_folder(move || {
            let folder_id = bridge.state.read().current_folder_id;
            if let Some(id) = folder_id {
                let _ = bridge.library.refresher().refresh_folder(id);
                bridge.reload_media_view_full(true);
                bridge.window.set_status_text("Folder refreshed.".into());
            }
        });

        let dialog = self.settings_dialog.clone();
        self.window.on_open_settings(move || {
            dialog.borrow_mut().show();
        });

        let bridge = self.clone_handle();
        self.window.on_folder_selected(move |id, path| {
            bridge.select_folder(id as u64, PathBuf::from(path.as_str()));
        });

        let bridge = self.clone_handle();
        self.window.on_favorite_selected(move |path_str| {
            let path = PathBuf::from(path_str.as_str());
            if let Ok(Some(folder)) = bridge.library.folders().get_folder_by_path(&path) {
                bridge.select_folder(folder.id, path);
            }
        });
    }

    fn register_media_callbacks(&self) {
        let bridge = self.clone_handle();
        self.window.on_media_clicked(move |id, is_ctrl, is_shift| {
            bridge.handle_media_click(id as u64, is_ctrl, is_shift);
        });

        let bridge = self.clone_handle();
        self.window.on_player_close(move || {
            let _ = bridge.player.stop();
            bridge.window.set_is_player_open(false);
        });
    }

    fn register_context_menu_callbacks(&self) {
        let bridge = self.clone_handle();
        self.window.on_media_context_menu(move |id, x, y| {
            bridge.show_media_context_menu(id as u64, x, y);
        });

        let bridge = self.clone_handle();
        self.window.on_context_menu_item_clicked(move |action_id| {
            bridge.handle_context_menu_action(action_id as u64);
        });

        let bridge = self.clone_handle();
        self.window.on_context_menu_dismissed(move || {
            bridge.window.set_is_context_menu_open(false);
        });
    }

    fn register_tag_callbacks(&self) {
        let bridge = self.clone_handle();
        self.window.on_rating_applied(move |rating| {
            bridge.apply_rating(if rating < 0 { None } else { Some(rating as u8) });
        });

        let bridge = self.clone_handle();
        self.window.on_tag_toggled(move |tag_id| {
            bridge.toggle_tag(tag_id as u32);
        });

        let bridge = self.clone_handle();
        self.window.on_tag_mode_toggled(move |is_assign| {
            bridge.state.write().is_assign_mode = is_assign;
            bridge.reload_tags();
        });

        let bridge = self.clone_handle();
        self.window.on_tag_filter_changed(move |query| {
            bridge.filter_tags_view(&query);
        });
    }

    fn register_query_callbacks(&self) {
        let bridge = self.clone_handle();
        self.window.on_search_query_changed(move |query| {
            bridge.update_search_query(query.to_string());
        });

        let bridge = self.clone_handle();
        self.window.on_regex_mode_toggled(move |enabled| {
            bridge.state.write().filter.is_regex = enabled;
            bridge.reload_media_view_filtered();
        });

        let bridge = self.clone_handle();
        self.window.on_match_mode_changed(move |idx| {
            let mode = if idx == 0 {
                MatchMode::All
            } else {
                MatchMode::Any
            };
            bridge.state.write().filter.match_mode = mode;
            bridge.reload_media_view_filtered();
        });

        let bridge = self.clone_handle();
        self.window.on_scope_mode_changed(move |idx| {
            let scope = if idx == 0 {
                ScopeMode::Folder
            } else {
                ScopeMode::Global
            };
            bridge.state.write().filter.scope_mode = scope;
            bridge.reload_media_view_filtered();
        });
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
            last_click: self.last_click.clone(),
            _thumbnail_timer: Timer::default(),
            _player_timer: Timer::default(),
        }
    }

    // =========================================================================
    // Delegated Public Facade APIs
    // =========================================================================

    pub fn sync_root_folder(&self) {
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
            &self.state,
            false,
            false,
        );
    }

    pub fn handle_media_click(&self, media_id: MediaId, is_ctrl: bool, is_shift: bool) {
        BridgeFileSelection::handle_click(
            FileClickEvent {
                media_id,
                is_ctrl,
                is_shift,
            },
            &self.window,
            &self.library,
            &self.player,
            &self.state,
            &self.last_click,
        );
    }

    pub fn handle_media_double_click(&self, media_id: MediaId) {
        BridgeFileSelection::handle_double_click(
            media_id,
            &self.state,
            &self.library,
            &self.player,
            &self.window,
        );
    }

    pub fn open_media_item(&self, media_id: MediaId) {
        BridgeFileSelection::open_media(
            media_id,
            &self.state,
            &self.library,
            &self.player,
            &self.window,
        );
    }

    pub fn select_all(&self) {
        BridgeFileSelection::select_all(&self.window, &self.state);
    }

    pub fn clear_selection(&self) {
        BridgeFileSelection::clear_selection(&self.window, &self.state);
    }

    pub fn update_search_query(&self, query: String) {
        self.state.write().filter.search_query = query;
        self.reload_media_view_filtered();
    }

    pub fn update_inspector(&self) {
        BridgeInspector::update(&self.window, &self.state);
    }

    pub fn show_media_context_menu(&self, media_id: MediaId, x: f32, y: f32) {
        BridgeContextMenu::show(&self.window, &self.state, media_id, x, y);
    }

    pub fn handle_context_menu_action(&self, action_id: CtxMenuActionId) {
        let bridge_rename = self.clone_handle();
        let bridge_rating = self.clone_handle();

        BridgeContextMenu::handle_action(
            action_id,
            &self.window,
            &self.library,
            &self.state,
            &self.ctx_menu_dispatcher,
            move |current_name| bridge_rename.show_rename_dialog(current_name),
            move |ids, rating| bridge_rating.apply_rating_to_ids(ids, rating),
        );
    }

    pub fn show_rename_dialog(&self, current_name: &str) {
        let bridge_confirm = self.clone_handle();
        let bridge_cancel = self.clone_handle();
        let bridge_close = self.clone_handle();

        BridgeContextMenu::show_rename_dialog(
            &self.rename_dialog,
            &self.window,
            current_name,
            move |new_name| bridge_confirm.execute_file_rename(&new_name),
            move || bridge_cancel.close_rename_dialog(),
            move || bridge_close.close_rename_dialog(),
        );
    }

    pub fn close_rename_dialog(&self) {
        BridgeContextMenu::close_rename_dialog(&self.rename_dialog, &self.state, &self.window);
    }

    pub fn execute_file_rename(&self, new_name: &str) {
        BridgeContextMenu::execute_file_rename(
            new_name,
            &self.library,
            &self.thumbnails,
            &self.state,
            &self.window,
            &self.rename_dialog,
        );
    }

    pub fn reload_tags(&self) {
        BridgeTags::reload_tags(&self.window, &self.library);
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
            &self.state,
        );
    }

    pub fn apply_rating(&self, rating: Option<u8>) {
        BridgeTags::apply_rating(
            rating,
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.state,
        );
    }

    fn apply_rating_to_ids(&self, ids: &std::collections::HashSet<MediaId>, rating: u8) {
        BridgeTags::apply_rating_to_ids(
            ids,
            rating,
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.state,
        );
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
}
