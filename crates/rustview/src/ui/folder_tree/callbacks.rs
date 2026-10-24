//! The callbacks of the folder tree panel.

use super::BridgeFolders;
use crate::ui::MainWindow;
use crate::ui::SettingsDialogController;
use crate::ui::media::BridgeMedia;
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::status_bar::StatusBarBridge;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::WrappingAs;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

impl BridgeFolders {
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        settings_dialog: Rc<RefCell<SettingsDialogController>>,
    ) {
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();

        window.on_refresh_folder(move || {
            if BridgeMedia::is_system_busy(&win, &thumb, &st) {
                StatusBarBridge::warn(&win, "Cannot refresh: another operation is already in progress.");
                return;
            }

            let (folder_id_opt, path_opt) = {
                let s = st.read();
                (s.current_folder_id, s.current_folder_path.clone())
            };
            if let (Some(id), Some(path)) = (folder_id_opt, path_opt) {
                tracing::info!(
                    folder_id = id,
                    path = %LogObscurer::path(&path),
                    "[FolderRefresh: Toolbar] Executing explicit full media refresh."
                );
                Self::refresh_folder_explicit(id, &path, &win, &lib, &thumb, &set, &st);
            }
        });

        let dialog = settings_dialog.clone();
        window.on_open_settings(move || {
            dialog.borrow_mut().show();
        });
        let dialog = settings_dialog;
        window.on_open_hotkey_settings(move || {
            dialog.borrow_mut().show_page(crate::ui::settings_dialog::HOTKEYS_PAGE);
        });

        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_selected(move |id, path| {
            st.write().last_active_panel = LastActivePanel::Folder;
            Self::select_folder(
                id.wrapping_as::<u64>(),
                &PathBuf::from(path.as_str()),
                &win,
                &lib,
                &thumb,
                &set,
                &st,
            );
        });

        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_toggle_expand(move |id| {
            st.write().last_active_panel = LastActivePanel::Folder;
            Self::toggle_expand(id.wrapping_as::<u64>(), &win, &lib, &set, &st);
        });

        let lib_all = library.clone();
        let thumb_all = thumbnails.clone();
        let set_all = settings.clone();
        let st_all = state.clone();
        let win_all = window.clone_strong();
        window.on_folder_refresh_all_clicked(move || {
            st_all.write().last_active_panel = LastActivePanel::Folder;
            if BridgeMedia::is_system_busy(&win_all, &thumb_all, &st_all) {
                StatusBarBridge::warn(
                    &win_all,
                    "Cannot refresh folder tree: another operation is already in progress.",
                );
                return;
            }
            tracing::info!("[FolderRefresh: Manual] Toolbar 'Refresh Tree (Full)' triggered by user");
            Self::refresh_folder_tree(&win_all, &lib_all, &thumb_all, &set_all, &st_all);
        });

        let lib_rel = library.clone();
        let thumb_rel = thumbnails.clone();
        let set_rel = settings.clone();
        let st_rel = state.clone();
        let win_rel = window.clone_strong();
        window.on_folder_refresh_relative_clicked(move || {
            st_rel.write().last_active_panel = LastActivePanel::Folder;
            if BridgeMedia::is_system_busy(&win_rel, &thumb_rel, &st_rel) {
                StatusBarBridge::warn(
                    &win_rel,
                    "Cannot refresh folder tree: another operation is already in progress.",
                );
                return;
            }
            tracing::info!("[FolderRefresh: Manual] Toolbar 'Refresh Tree (Relative)' triggered by user");
            Self::refresh_selected_folder_subtree(&win_rel, &lib_rel, &thumb_rel, &set_rel, &st_rel);
        });

        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_collapse_all_clicked(move || {
            st.write().last_active_panel = LastActivePanel::Folder;
            Self::collapse_all_except_active(&win, &lib, &set, &st);
        });

        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        let lib = library;
        window.on_toggle_hidden_folders(move |show_hidden| {
            st.write().last_active_panel = LastActivePanel::Folder;
            let mut app_settings = set.get();
            app_settings.folders.show_hidden_folders = show_hidden;
            set.save_quiet_async(&app_settings);
            Self::reload_tree(&win, &lib, &set, &st);
            Self::scroll_to_selected(&win);
        });
    }
}
