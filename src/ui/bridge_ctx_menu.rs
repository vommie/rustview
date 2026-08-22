pub use super::bridge_ctx_menu_folder::FolderContextMenu;
pub use super::bridge_ctx_menu_media::{MediaActionContext, MediaContextMenu};
pub use super::bridge_ctx_menu_presenter::CtxMenuPresenter;
pub use super::bridge_ctx_menu_tag::TagContextMenu;
pub use super::bridge_file_dialog_delete::DeleteFileDialogController;
pub use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
pub use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
pub use super::bridge_file_dialogs::{FileDialogContext, FileRenameContext, FileRenameDialogController};

pub use super::bridge_ctx_menu_folder::FolderContextMenu as FolderContextMenuBuilder;
pub use super::bridge_ctx_menu_media::MediaContextMenu as MediaContextMenuBuilder;
pub use super::bridge_ctx_menu_presenter::CtxMenuPresenter as SlintContextMenuPresenter;
pub use super::bridge_file_dialogs::FileRenameDialogController as FileRenameController;

use super::bridge_folder_dialogs::{FolderDialogContext, FolderDialogController};
use super::bridge_folders::{FolderTreeBuilder, FolderTreeNode};
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_tag_dialogs::TagDialogController;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{CtxMenu, CtxMenuDispatcher};
use crate::library::Library;
use crate::library::models::{FolderId, FolderRecord, MediaId};
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

pub struct ContextMenuRegistration {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub state: Arc<RwLock<UiState>>,
    pub dispatcher: Arc<CtxMenuDispatcher>,
    pub rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    pub missing_file_dialog: MissingFileDialogController,
    pub delete_file_dialog: DeleteFileDialogController,
    pub remove_orphaned_dialog: RemoveOrphanedFileDialogController,
    pub tag_dialogs: TagDialogController,
    pub folder_dialogs: FolderDialogController,
    pub settings: Arc<SettingsManager>,
}

pub struct BridgeContextMenu;

impl BridgeContextMenu {
    pub fn register_callbacks(window: &MainWindow, reg: ContextMenuRegistration) {
        Self::register_media_menu(window, &reg.library, &reg.thumbnails, &reg.state);
        Self::register_folder_menu(window, &reg.library, &reg.settings, &reg.state);
        Self::register_popup_events(window, &reg.state);
        Self::register_action_dispatcher(window, reg);
    }

    fn register_media_menu(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let th = thumbnails.clone();
        let st = state.clone();
        window.on_media_context_menu(move |id, x, y| {
            Self::show_media_menu(&win, &lib, &th, &st, id as u64, x, y);
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
        let missing_file_dlg = reg.missing_file_dialog;
        let delete_file_dlg = reg.delete_file_dialog;
        let remove_orphaned_dlg = reg.remove_orphaned_dialog;
        let tag_dlg = reg.tag_dialogs;
        let fld_dlg = reg.folder_dialogs;

        window.on_context_menu_item_clicked(move |action_id| {
            CtxMenuPresenter::dismiss(&win);
            let action_u64 = action_id as u64;

            // 1. Folder Actions
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

            // 2. Tag Actions
            if action_u64 >= CTX_TAG_MOVE_TARGET_BASE || (2000..3000).contains(&action_u64) {
                let bundle = tag_dlg.create_action_bundle(&win, &lib, &thumb, &set, &st);
                TagContextMenu::handle_action(action_u64, &win, &lib, &set, &st, bundle);
                return;
            }

            // 3. Media File Actions
            MediaContextMenu::handle_action(
                action_u64,
                &win,
                &MediaActionContext {
                    library: &lib,
                    thumbnails: &thumb,
                    settings: &set,
                    state: &st,
                    dispatcher: &disp,
                    rename_dialog: &ren_dlg,
                    missing_file_dialog: &missing_file_dlg,
                    delete_file_dialog: &delete_file_dlg,
                    remove_orphaned_dialog: &remove_orphaned_dlg,
                },
            );
        });
    }

    pub fn show_media_menu(
        window: &MainWindow,
        library: &Library,
        thumbnails: &ThumbnailManager,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
        x: f32,
        y: f32,
    ) {
        let (all_selected_deleted, selected_count, is_single_image, associated_apps) =
            MediaContextMenu::prepare_selection(window, library, state, media_id);
        let is_busy = BridgeMedia::is_system_busy(window, thumbnails, state);
        let menu = MediaContextMenu::build_menu(selected_count, is_single_image, all_selected_deleted, is_busy, &associated_apps);
        Self::open_custom_menu(window, state, menu, x, y);
    }

    pub fn show_folder_menu(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &Arc<RwLock<UiState>>,
        folder_id: FolderId,
        x: f32,
        y: f32,
    ) {
        let app_settings = settings.get();
        let root_setting = app_settings.general.root_folder;
        let show_hidden_folders = app_settings.folders.show_hidden_folders;
        let support_dot_hidden_files = app_settings.folders.support_dot_hidden_files;

        let mut st = state.write();
        st.active_folder_dialog_target = Some(folder_id);

        if st.cached_folder_tree.is_none()
            && let Some(ref root_dir) = root_setting
        {
            let folders = library.folders().list_folders().unwrap_or_default();
            let media_counts = library.folders().get_all_media_counts().unwrap_or_default();
            let built_tree = FolderTreeBuilder::build_hierarchy(root_dir, &folders, &media_counts);
            st.cached_folder_tree = Some(built_tree);
            st.cached_media_counts = media_counts;
        }

        let UiState {
            cached_folder_tree: ref tree_opt,
            ref mut dot_hidden_cache,
            ref mut active_context_menu,
            ..
        } = *st;

        let Some(tree) = tree_opt.as_ref() else {
            return;
        };

        let target_record = Self::find_record_in_tree(tree, folder_id);
        let Some(folder_rec) = target_record else {
            return;
        };

        let is_root = root_setting
            .as_ref()
            .map(|r| Path::new(&folder_rec.path) == r)
            .unwrap_or(false);

        let menu = FolderContextMenu::build_menu_from_cached_tree(
            &folder_rec,
            tree,
            is_root,
            show_hidden_folders,
            support_dot_hidden_files,
            dot_hidden_cache,
        );

        CtxMenuPresenter::open(window, active_context_menu, menu, x, y);
    }

    fn find_record_in_tree(nodes: &[FolderTreeNode], target_id: FolderId) -> Option<FolderRecord> {
        for node in nodes {
            if node.record.id == target_id {
                return Some(node.record.clone());
            }
            if let Some(rec) = Self::find_record_in_tree(&node.children, target_id) {
                return Some(rec);
            }
        }
        None
    }

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
