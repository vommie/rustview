pub use super::bridge_ctx_menu_folder::FolderContextMenu;
pub use super::bridge_ctx_menu_media::MediaContextMenu;
pub use super::bridge_ctx_menu_presenter::CtxMenuPresenter;
pub use super::bridge_ctx_menu_tag::TagContextMenu;
pub use super::bridge_file_dialogs::{FileRenameContext, FileRenameDialogController};

// Backward-compatibility re-exports for existing modules & integration tests
pub use super::bridge_ctx_menu_folder::FolderContextMenu as FolderContextMenuBuilder;
pub use super::bridge_ctx_menu_media::MediaContextMenu as MediaContextMenuBuilder;
pub use super::bridge_ctx_menu_presenter::CtxMenuPresenter as SlintContextMenuPresenter;
pub use super::bridge_file_dialogs::FileRenameDialogController as FileRenameController;

use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::bridge_state::*;
use super::bridge_tag_dialogs::TagDialogController;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{CtxMenu, CtxMenuDispatcher};
use crate::library::Library;
use crate::library::models::{FolderId, MediaId};
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// Registration dependency container for context menu subsystems.
pub struct ContextMenuRegistration {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub state: Arc<RwLock<UiState>>,
    pub dispatcher: Arc<CtxMenuDispatcher>,
    pub rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    pub tag_dialogs: TagDialogController,
    pub folder_dialogs: FolderDialogController,
    pub settings: Arc<SettingsManager>,
}

/// Central Context Menu Bridge Facade coordinating Slint UI callbacks and domain routers.
pub struct BridgeContextMenu;

impl BridgeContextMenu {
    /// Registers all context menu and submenu flyout callbacks onto MainWindow.
    pub fn register_callbacks(window: &MainWindow, reg: ContextMenuRegistration) {
        Self::register_media_menu(window, &reg.state);
        Self::register_folder_menu(window, &reg.library, &reg.settings, &reg.state);
        Self::register_popup_events(window, &reg.state);
        Self::register_action_dispatcher(window, reg);
    }

    fn register_media_menu(window: &MainWindow, state: &Arc<RwLock<UiState>>) {
        let win = window.clone_strong();
        let st = state.clone();
        window.on_media_context_menu(move |id, x, y| {
            Self::show_media_menu(&win, &st, id as u64, x, y);
        });
    }

    fn register_folder_menu(
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_folder_context_menu(move |id, x, y| {
            Self::show_folder_menu(&win, &lib, &set, &st, id as u64, x, y);
        });
    }

    fn register_popup_events(window: &MainWindow, state: &Arc<RwLock<UiState>>) {
        let win = window.clone_strong();
        let st = state.clone();
        window.on_context_menu_request_submenu(move |depth, m_idx| {
            let active = st.read().active_context_menu.clone();
            CtxMenuPresenter::handle_submenu_request(&win, active.as_ref(), depth, m_idx as usize);
        });

        let win = window.clone_strong();
        window.on_context_menu_dismissed(move || {
            CtxMenuPresenter::dismiss(&win);
        });
    }

    fn register_action_dispatcher(window: &MainWindow, reg: ContextMenuRegistration) {
        let win = window.clone_strong();
        let lib = reg.library;
        let thumb = reg.thumbnails;
        let st = reg.state;
        let set = reg.settings;
        let disp = reg.dispatcher;
        let ren_dlg = reg.rename_dialog;
        let tag_dlg = reg.tag_dialogs;
        let fld_dlg = reg.folder_dialogs;

        window.on_context_menu_item_clicked(move |action_id| {
            // Instantly dismiss popup before routing action
            CtxMenuPresenter::dismiss(&win);
            let action_u64 = action_id as u64;

            // 1. Folder Actions (Move-to / Rename / Delete)
            if action_u64 >= CTX_FOLDER_MOVE_BASE || (3000..4000).contains(&action_u64) {
                let target_folder_id = st.read().active_folder_dialog_target;
                let ctx = FolderDialogContext {
                    library: lib.clone(),
                    thumbnails: thumb.clone(),
                    settings: set.clone(),
                    state: st.clone(),
                };
                FolderContextMenu::handle_action(action_u64, &win, &fld_dlg, ctx, target_folder_id);
                return;
            }

            // 2. Tag Actions (CRUD / Move / Tree Navigation)
            if action_u64 >= CTX_TAG_MOVE_TARGET_BASE || (2000..3000).contains(&action_u64) {
                let bundle = tag_dlg.create_action_bundle(&win, &lib, &thumb, &set, &st);
                TagContextMenu::handle_action(action_u64, &win, &lib, &set, &st, bundle);
                return;
            }

            // 3. Media File Actions (Rename / Ratings / Selections)
            MediaContextMenu::handle_action(action_u64, &win, &lib, &thumb, &st, &disp, &ren_dlg);
        });
    }

    /// Displays the media tile context menu.
    pub fn show_media_menu(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
        x: f32,
        y: f32,
    ) {
        let (selected_count, _) = MediaContextMenu::prepare_selection(window, state, media_id);
        let menu = MediaContextMenu::build_menu(selected_count);
        Self::open_custom_menu(window, state, menu, x, y);
    }

    /// Displays the folder tree item context menu.
    pub fn show_folder_menu(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &Arc<RwLock<UiState>>,
        folder_id: FolderId,
        x: f32,
        y: f32,
    ) {
        let Ok(Some(folder_rec)) = library.folders().get_folder(folder_id) else {
            return;
        };

        state.write().active_folder_dialog_target = Some(folder_id);

        let all_folders = library.folders().list_folders().unwrap_or_default();
        let root_setting = settings.get().general.root_folder;
        let is_root = root_setting
            .as_ref()
            .map(|r| Path::new(&folder_rec.path) == r)
            .unwrap_or(false);

        let menu = FolderContextMenu::build_menu(
            &folder_rec,
            &all_folders,
            root_setting.as_deref(),
            is_root,
        );
        Self::open_custom_menu(window, state, menu, x, y);
    }

    /// Renders any custom declarative CtxMenu into the multi-level popup presenter.
    #[inline]
    pub fn open_custom_menu(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        menu: CtxMenu,
        x: f32,
        y: f32,
    ) {
        let mut st = state.write();
        CtxMenuPresenter::open(window, &mut st.active_context_menu, menu, x, y);
    }
}
