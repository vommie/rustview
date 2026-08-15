use super::bridge_context_menu::BridgeContextMenu;
use super::bridge_favorites::BridgeFavorites;
use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_player::BridgePlayer;
use super::bridge_state::*;
use super::bridge_tag_dialogs::TagDialogController;
use super::bridge_tags::BridgeTags;
use super::bridge_thumbnails::BridgeThumbnails;
use super::favorites::FavoriteManager;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::CtxMenuDispatcher;
use crate::library::Library;
use crate::library::models::{FolderId, MediaId, TagId};
use crate::player::PlayerController;
use crate::settings::{AppSettings, SettingsDialogController, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle, Timer};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Central UI Registry and Bootstrapper coordinating specialized UI subsystems.
pub struct UiBridge {
    window: MainWindow,
    library: Arc<Library>,
    thumbnails: Arc<ThumbnailManager>,
    player: Arc<PlayerController>,
    settings: Arc<SettingsManager>,
    settings_dialog: Rc<RefCell<SettingsDialogController>>,
    rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    tag_dialogs: TagDialogController,
    favorites: Arc<RwLock<FavoriteManager>>,
    ctx_menu_dispatcher: Arc<CtxMenuDispatcher>,
    state: Arc<RwLock<UiState>>,
    last_click: Arc<Mutex<Option<(MediaId, Instant)>>>,
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
        let tag_dialogs = TagDialogController::new();

        // Restore persistent UI state (expanded tag IDs from settings)
        let mut initial_state = UiState::default();
        let restored_expanded = settings.get().tags.expanded_tag_ids;
        tracing::info!(
            restored_count = restored_expanded.len(),
            "[TagTree: Session Restore] Restoring persistent expanded tag IDs"
        );
        initial_state.expanded_tag_ids = restored_expanded.into_iter().collect();

        let state = Arc::new(RwLock::new(initial_state));
        let ctx_menu_dispatcher = Arc::new(CtxMenuDispatcher::new());
        let last_click = Arc::new(Mutex::new(None));

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
            tag_dialogs,
            favorites,
            ctx_menu_dispatcher,
            state,
            last_click,
            _thumbnail_timer: thumbnail_timer,
            _player_timer: player_timer,
        };

        bridge.register_subsystem_callbacks();
        bridge.register_settings_subscriber();

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

    /// Clean, modular callback dispatch to each specialized bridge subsystem (SRP).
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
            self.player.clone(),
            self.state.clone(),
            self.last_click.clone(),
        );

        BridgeContextMenu::register_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.state.clone(),
            self.ctx_menu_dispatcher.clone(),
            self.rename_dialog.clone(),
            self.tag_dialogs.clone(),
            self.settings.clone(),
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
            self.state.clone(),
        );

        BridgePlayer::register_callbacks(&self.window, self.player.clone());
    }

    // =========================================================================
    // Public Facade APIs
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
            super::bridge_file_selection::FileClickEvent {
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
        BridgeContextMenu::show(&self.window, &self.state, media_id, x, y);
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

    pub fn apply_rating_to_ids(&self, ids: &std::collections::HashSet<MediaId>, rating: u8) {
        BridgeTags::apply_rating_to_ids(
            ids,
            rating,
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.state,
        );
    }
}
