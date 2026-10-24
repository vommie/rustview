pub mod browse;
pub mod favorite;
pub mod folder;
pub mod media;
pub mod presenter;
pub mod tab;
pub mod tag;

pub use crate::ui::context_menu::favorite::FavoriteContextMenu;
pub use crate::ui::context_menu::folder::FolderContextMenu;
pub use crate::ui::context_menu::media::{MediaActionContext, MediaContextMenu, MediaSelectionMeta};
pub use crate::ui::context_menu::presenter::CtxMenuPresenter;
pub use crate::ui::context_menu::tab::{TabActionContext, TabContextMenu};
pub use crate::ui::context_menu::tag::TagContextMenu;
pub use crate::ui::file_dialogs::conflict::FileOpsConflictDialogController;
pub use crate::ui::file_dialogs::delete::DeleteFileDialogController;
pub use crate::ui::file_dialogs::missing::{MissingFileDialogController, MissingFileItem};
pub use crate::ui::file_dialogs::remove_orphaned::RemoveOrphanedFileDialogController;
pub use crate::ui::file_dialogs::{FileDialogContext, FileRenameContext, FileRenameDialogController};

use crate::ctx_menu::CtxMenuDispatcher;
use crate::ui::favorites::manager::FavoriteManager;
use crate::ui::favorites::rename_dialog::FavoriteRenameDialogController;
use crate::ui::folder_dialogs::FolderDialogController;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::state::UiState;
use crate::ui::tags::dialogs::TagDialogController;
use crate::ui::{FileRenameDialog, MainWindow};
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub mod cutouts;
mod dispatcher;
mod outside_click;
mod registration;
mod show;
pub use cutouts::{calculate_all_active_menu_cutouts, make_rounded_cutout_rectangles};

pub struct MediaMenuContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Library,
    pub thumbnails: &'a ThumbnailManager,
    pub tab_manager: &'a ViewerTabManager,
    pub state: &'a Arc<RwLock<UiState>>,
    pub settings: &'a SettingsManager,
}

pub struct FolderMenuContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Library,
    pub thumbnails: &'a ThumbnailManager,
    pub settings: &'a SettingsManager,
    pub favorites: &'a Arc<RwLock<FavoriteManager>>,
    pub tab_manager: &'a ViewerTabManager,
    pub state: &'a Arc<RwLock<UiState>>,
}

pub struct ContextMenuRegistration {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub tab_manager: Arc<ViewerTabManager>,
    pub favorites: Arc<RwLock<FavoriteManager>>,
    pub state: Arc<RwLock<UiState>>,
    pub dispatcher: Arc<CtxMenuDispatcher>,
    pub rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    pub favorite_rename_dialog: FavoriteRenameDialogController,
    pub missing_file_dialog: MissingFileDialogController,
    pub delete_file_dialog: DeleteFileDialogController,
    pub remove_orphaned_dialog: RemoveOrphanedFileDialogController,
    pub conflict_dialog: FileOpsConflictDialogController,
    pub tag_dialogs: TagDialogController,
    pub folder_dialogs: FolderDialogController,
    pub fullscreen: MultiviewHandle,
    pub settings: Arc<SettingsManager>,
}

pub struct BridgeContextMenu;

impl BridgeContextMenu {
    pub fn register_callbacks(window: &MainWindow, reg: ContextMenuRegistration) {
        Self::register_media_menu(
            window,
            &reg.library,
            &reg.thumbnails,
            &reg.tab_manager,
            &reg.settings,
            &reg.state,
        );
        Self::register_folder_menu(
            window,
            &reg.library,
            &reg.thumbnails,
            &reg.settings,
            &reg.favorites,
            &reg.tab_manager,
            &reg.state,
        );
        Self::register_tab_menu(window, &reg.tab_manager, &reg.thumbnails, &reg.state);
        Self::register_favorite_menu(window, &reg.tab_manager, &reg.state);
        Self::register_popup_events(window, &reg.tab_manager, &reg.state);
        Self::register_action_dispatcher(window, reg);
    }
}
