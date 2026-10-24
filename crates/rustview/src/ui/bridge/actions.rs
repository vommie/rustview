//! Rename and delete actions: their callbacks and the entry points shared by the keyboard, the menus and the Multiview.

use super::{DeleteActionParams, RenameActionParams, UiBridge};
use crate::ui::browse_folders::BrowseFolders;
use crate::ui::favorites::BridgeFavorites;
use crate::ui::file_dialogs::delete::DeleteFollowup;
use crate::ui::file_dialogs::{FileDialogContext, FileRenameDialogController, RenameDialogParams};
use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::media::BridgeMedia;
use crate::ui::multiview::tab_host;
use crate::ui::state::LastActivePanel;
use crate::ui::status_bar::StatusBarBridge;
use rustview_core::library::folders::normalize_path;
use rustview_core::library::models::{FolderId, MediaId};
use rustview_core::settings::LogObscurer;
use slint::ComponentHandle;
use std::path::Path;
use std::rc::Rc;

impl UiBridge {
    pub(super) fn register_rename_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let tm = self.tab_manager.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let ren_dlg = self.rename_dialog.clone();
        let fav_ren_dlg = self.favorite_rename_dialog.clone();
        let favs = self.favorites.clone();
        let missing_dlg = self.missing_file_dialog.clone();
        let conflict_dlg = self.conflict_dialog.clone();
        let fld_dlg = self.folder_dialogs.clone();
        let fs_ctrl = self.fullscreen.clone();

        self.window.on_request_rename(move || {
            Self::trigger_rename_action(&RenameActionParams {
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                tab_manager: &tm,
                settings: &set,
                state: &st,
                rename_dialog: &ren_dlg,
                favorite_rename_dialog: &fav_ren_dlg,
                favorites: &favs,
                missing_dialog: &missing_dlg,
                conflict_dialog: &conflict_dlg,
                folder_dialogs: &fld_dlg,
                fullscreen: &fs_ctrl,
            });
        });
    }

    /// The folder rows of the Browse view, the "Show Folders" toggle and the "New Folder" button of the toolbar.
    pub(super) fn register_browse_folder_callbacks(&self) {
        BrowseFolders::register_callbacks(
            &self.window,
            self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            self.state.clone(),
        );

        let (win, lib, set, st) = (
            self.window.clone_strong(),
            self.library.clone(),
            self.settings.clone(),
            self.state.clone(),
        );
        self.window
            .on_browse_go_up_requested(move || BrowseFolders::go_up(&win, &lib, &set, &st));
        let (win, lib, set, st) = (
            self.window.clone_strong(),
            self.library.clone(),
            self.settings.clone(),
            self.state.clone(),
        );
        self.window
            .on_browse_history_requested(move |direction| BrowseFolders::go_history(direction, &win, &lib, &set, &st));

        let win = self.window.clone_strong();
        let ctx = FolderDialogContext {
            library: self.library.clone(),
            thumbnails: self.thumbnails.clone(),
            tab_manager: self.tab_manager.clone(),
            settings: self.settings.clone(),
            state: self.state.clone(),
        };
        let dialogs = self.folder_dialogs.clone();
        self.window.on_new_folder_clicked(move || {
            let folder = ctx.state.read().current_folder_id;
            match folder {
                Some(folder_id) => dialogs.show_new(&win, folder_id, ctx.clone()),
                None => StatusBarBridge::info(&win, "Select a folder to create a new folder in."),
            }
        });
    }

    pub(super) fn register_delete_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let tm = self.tab_manager.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let favs = self.favorites.clone();
        let del_file_dlg = self.delete_file_dialog.clone();
        let orph_file_dlg = self.remove_orphaned_dialog.clone();
        let fld_dlg = self.folder_dialogs.clone();
        let fs_ctrl = self.fullscreen.clone();

        self.window.on_request_delete(move || {
            Self::trigger_delete_action(&DeleteActionParams {
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                tab_manager: &tm,
                settings: &set,
                state: &st,
                favorites: &favs,
                delete_file_dialog: &del_file_dlg,
                remove_orphaned_file_dialog: &orph_file_dlg,
                folder_dialogs: &fld_dlg,
                fullscreen: &fs_ctrl,
            });
        });
    }

    pub fn trigger_delete_action(params: &DeleteActionParams<'_>) {
        let window = params.window;
        let active_mode = window.get_active_view_mode();
        // DEL or the delete button in the fullscreen window acts on the medium there; DEL in the main window always acts on
        // what the main window shows, even while a Multiview is open next to it.
        let fullscreen_target = crate::ui::multiview::handle::delete_requested_from_fullscreen();

        if fullscreen_target.is_none() && active_mode != 0 {
            tracing::debug!(
                active_mode,
                "[UiBridge: DeleteAction] DEL key pressed outside Browse View (mode != 0); operation safely bypassed"
            );
            return;
        }

        if BridgeMedia::is_system_busy(window, params.thumbnails, params.state) {
            StatusBarBridge::warn(window, "Cannot delete: an active operation is in progress.");
            return;
        }

        if let Some(media_id) = fullscreen_target {
            tracing::info!(
                media_id,
                "[UiBridge: DeleteAction] Delete requested in a Multiview: opening delete files dialog for its selected medium"
            );
            let fullscreen = params.fullscreen.clone();
            let followup: DeleteFollowup = Rc::new(move |ids| fullscreen.drop_deleted(ids));
            Self::open_media_delete_dialog(params, vec![media_id], Some(followup));
            return;
        }

        let last_panel = params.state.read().last_active_panel;
        let active_sidebar_tab = window.get_active_sidebar_panel_tab();
        let is_folders_tab = active_sidebar_tab.as_str() == "folders";
        let is_favorites_tab = active_sidebar_tab.as_str() == "favorites";

        let browse_folders = params.state.read().focused_browse_folders();
        let effective_panel = match last_panel {
            _ if !browse_folders.is_empty() => LastActivePanel::Folder,
            LastActivePanel::Media => LastActivePanel::Media,
            LastActivePanel::Folder => {
                if is_favorites_tab {
                    LastActivePanel::Favorite
                } else {
                    LastActivePanel::Folder
                }
            }
            LastActivePanel::Favorite => {
                if is_folders_tab {
                    LastActivePanel::Folder
                } else {
                    LastActivePanel::Favorite
                }
            }
        };

        if effective_panel == LastActivePanel::Favorite {
            let selected_fav_path = params.state.read().selected_favorite_path.clone();
            let path_to_remove = selected_fav_path.or_else(|| params.state.read().current_folder_path.clone());

            // The entry is copied out first: a read guard in the `if let` would live through the block, where the favorite is
            // changed under the write lock.
            let found = path_to_remove.and_then(|path| {
                let entry = params.favorites.read().get_by_path(&path).cloned();
                entry.map(|entry| (path, entry))
            });
            if let Some((path, entry)) = found {
                tracing::info!(
                    path = %LogObscurer::path(&path),
                    alias = %entry.alias,
                    "[UiBridge: DeleteAction] DEL pressed on Favorite focus: Removing favorite"
                );
                BridgeFavorites::remove_favorite(window, params.favorites, &path);
                return;
            }

            StatusBarBridge::info(window, "Select a favorite to remove (Del).");
            return;
        }

        if effective_panel == LastActivePanel::Folder {
            // Folders highlighted in the Browse view: one folder alone goes through its own dialog, any larger selection through the
            // combined dialog for files and folders.
            let files = params.state.read().get_selected_ids_ordered();
            if browse_folders.len() > 1 || (!browse_folders.is_empty() && !files.is_empty()) {
                let ctx = FolderDialogContext {
                    library: params.library.clone(),
                    thumbnails: params.thumbnails.clone(),
                    tab_manager: params.tab_manager.clone(),
                    settings: params.settings.clone(),
                    state: params.state.clone(),
                };
                tracing::info!(
                    folders = browse_folders.len(),
                    files = files.len(),
                    "[UiBridge: DeleteAction] Delete requested on a selection of folders and files: opening the combined dialog"
                );
                params
                    .folder_dialogs
                    .show_delete_selection(window, &files, &browse_folders, &ctx);
                return;
            }

            let current_folder_id = browse_folders
                .first()
                .copied()
                .or(params.state.read().current_folder_id);
            let Some(folder_id) = current_folder_id else {
                StatusBarBridge::info(window, "No folder selected to delete (Del).");
                return;
            };
            Self::open_folder_delete_dialog(params, folder_id);
            return;
        }

        let selected_ids = params.state.read().get_selected_ids_ordered();
        if selected_ids.is_empty() {
            StatusBarBridge::info(window, "Select one or more files to delete (Del).");
            return;
        }

        Self::open_media_delete_dialog(params, selected_ids, None);
    }

    /// Opens the dialog that deletes a folder (or, for a folder that is missing on disk, the one that removes it from the database).
    pub(super) fn open_folder_delete_dialog(params: &DeleteActionParams<'_>, folder_id: FolderId) {
        let window = params.window;
        let is_root = {
            let app_settings = params.settings.get();
            if let Some(ref root_p) = app_settings.general.root_folder
                && let Ok(Some(folder_rec)) = params.library.folders().get_folder(folder_id)
            {
                normalize_path(Path::new(&folder_rec.path)) == normalize_path(root_p)
            } else {
                false
            }
        };

        if is_root {
            StatusBarBridge::info(window, "Cannot delete the library root directory.");
            return;
        }

        let folder_rec_opt = params.library.folders().get_folder(folder_id).ok().flatten();
        let Some(folder_rec) = folder_rec_opt else {
            StatusBarBridge::error(window, "Folder record not found in database.");
            return;
        };

        let folder_name = folder_rec.name.clone();

        let folder_ctx = FolderDialogContext {
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            tab_manager: params.tab_manager.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
        };

        if folder_rec.is_deleted {
            tracing::info!(
                folder = %LogObscurer::folder(Some(folder_id), Path::new(&folder_name)),
                "[UiBridge: DeleteAction] Delete requested on a missing folder: opening remove_orphaned folder dialog"
            );
            params
                .folder_dialogs
                .show_remove_orphaned(window, folder_id, &folder_name, folder_ctx);
        } else {
            tracing::info!(
                folder = %LogObscurer::folder(Some(folder_id), Path::new(&folder_name)),
                "[UiBridge: DeleteAction] Delete requested on an active folder: opening delete folder dialog"
            );
            params
                .folder_dialogs
                .show_delete(window, folder_id, &folder_name, folder_ctx);
        }
    }

    /// Opens the dialog that deletes `media_ids` (or, when every one of them is already missing on disk, the one that only
    /// removes them from the database). `followup` runs after a confirmed deletion.
    pub(super) fn open_media_delete_dialog(
        params: &DeleteActionParams<'_>,
        media_ids: Vec<MediaId>,
        followup: Option<DeleteFollowup>,
    ) {
        let window = params.window;
        let all_selected_deleted = {
            let st = params.state.read();
            media_ids
                .iter()
                .all(|&id| st.all_folder_media.find(id).is_some_and(|m| m.record.is_deleted))
        };

        let dialog_ctx = FileDialogContext {
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            tab_manager: params.tab_manager.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
        };

        if all_selected_deleted {
            tracing::info!(
                count = media_ids.len(),
                "[UiBridge: DeleteAction] DEL pressed on media selection (all missing): opening remove_orphaned files dialog"
            );
            params.remove_orphaned_file_dialog.show(window, media_ids, dialog_ctx);
        } else {
            tracing::info!(
                count = media_ids.len(),
                "[UiBridge: DeleteAction] DEL pressed on media selection: opening delete files dialog"
            );
            params
                .delete_file_dialog
                .show_with_followup(window, media_ids, dialog_ctx, followup);
        }
    }

    pub fn trigger_rename_action(params: &RenameActionParams<'_>) {
        let window = params.window;
        let active_mode = window.get_active_view_mode();
        // F2 pressed in the fullscreen window acts on the medium there; F2 in the main window always acts on what the main
        // window shows, even while a Multiview is open next to it.
        let from_fullscreen = crate::ui::multiview::handle::rename_requested_from_fullscreen();

        if from_fullscreen {
            if let Some(media_id) = params.fullscreen.rename_target() {
                Self::open_media_rename(media_id, params);
            }
            return;
        }

        if active_mode == 1 {
            if let Some(media_id) = tab_host::rename_target(params.tab_manager) {
                Self::open_media_rename(media_id, params);
            }
            return;
        }

        let last_panel = params.state.read().last_active_panel;
        let active_sidebar_tab = window.get_active_sidebar_panel_tab();
        let is_folders_tab = active_sidebar_tab.as_str() == "folders";
        let is_favorites_tab = active_sidebar_tab.as_str() == "favorites";

        let browse_folders = params.state.read().focused_browse_folders();
        let effective_panel = match last_panel {
            _ if !browse_folders.is_empty() => LastActivePanel::Folder,
            LastActivePanel::Media => LastActivePanel::Media,
            LastActivePanel::Folder => {
                if is_favorites_tab {
                    LastActivePanel::Favorite
                } else {
                    LastActivePanel::Folder
                }
            }
            LastActivePanel::Favorite => {
                if is_folders_tab {
                    LastActivePanel::Folder
                } else {
                    LastActivePanel::Favorite
                }
            }
        };

        if effective_panel == LastActivePanel::Favorite {
            let selected_fav_path = params.state.read().selected_favorite_path.clone();
            let path_to_rename = selected_fav_path.or_else(|| params.state.read().current_folder_path.clone());

            // The entry is copied out first: a read guard in the `if let` would live through the block, where the favorite is
            // changed under the write lock.
            let found = path_to_rename.and_then(|path| {
                let entry = params.favorites.read().get_by_path(&path).cloned();
                entry.map(|entry| (path, entry))
            });
            if let Some((path, entry)) = found {
                tracing::info!(
                    path = %LogObscurer::path(&path),
                    alias = %entry.alias,
                    "[UiBridge: RenameAction] F2 pressed on Favorite focus: Opening Rename Favorite dialog"
                );
                params.favorite_rename_dialog.show(
                    params.window,
                    &entry.alias,
                    &entry.path.clone(),
                    params.favorites.clone(),
                );
                return;
            }

            StatusBarBridge::info(window, "Select a favorite to rename (F2).");
            return;
        }

        if effective_panel == LastActivePanel::Folder {
            let file_count = params.state.read().file_selection.count();
            if browse_folders.len() > 1 || (!browse_folders.is_empty() && file_count > 0) {
                StatusBarBridge::warn(window, "Cannot rename several items at once. Select a single item.");
                return;
            }
            let current_folder_id = browse_folders
                .first()
                .copied()
                .or(params.state.read().current_folder_id);
            let Some(folder_id) = current_folder_id else {
                StatusBarBridge::info(window, "No folder selected to rename (F2).");
                return;
            };

            let is_root = {
                let app_settings = params.settings.get();
                if let Some(ref root_p) = app_settings.general.root_folder
                    && let Ok(Some(folder_rec)) = params.library.folders().get_folder(folder_id)
                {
                    normalize_path(Path::new(&folder_rec.path)) == normalize_path(root_p)
                } else {
                    false
                }
            };

            if is_root {
                StatusBarBridge::info(window, "Cannot rename the library root directory.");
                return;
            }

            let folder_rec_opt = params.library.folders().get_folder(folder_id).ok().flatten();
            let Some(folder_rec) = folder_rec_opt else {
                StatusBarBridge::error(window, "Folder record not found in database.");
                return;
            };

            let folder_name = folder_rec.name.clone();

            let folder_ctx = FolderDialogContext {
                library: params.library.clone(),
                thumbnails: params.thumbnails.clone(),
                tab_manager: params.tab_manager.clone(),
                settings: params.settings.clone(),
                state: params.state.clone(),
            };

            tracing::info!(
                folder = %LogObscurer::folder(Some(folder_id), Path::new(&folder_name)),
                "[UiBridge: RenameAction] F2 pressed on Folder Tree focus: Opening Rename Folder dialog"
            );

            params
                .folder_dialogs
                .show_rename(window, folder_id, &folder_name, folder_ctx);
            return;
        }

        let sel_ids = params.state.read().file_selection.selected_ids().clone();
        if sel_ids.len() == 1 {
            let media_id = *sel_ids.iter().next().unwrap();
            Self::open_media_rename(media_id, params);
        } else if sel_ids.is_empty() {
            StatusBarBridge::info(window, "Select a file to rename (F2).");
        } else {
            StatusBarBridge::warn(window, "Cannot rename multiple files at once. Select a single file.");
        }
    }

    pub(super) fn open_media_rename(media_id: MediaId, params: &RenameActionParams<'_>) {
        tracing::info!(
            media_id,
            "[UiBridge: RenameAction] Opening rename dialog for media file"
        );

        let dialog_ctx = FileDialogContext {
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            tab_manager: params.tab_manager.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
        };

        FileRenameDialogController::open(&RenameDialogParams {
            dialog_ref: params.rename_dialog,
            window: params.window,
            media_id,
            ctx: &dialog_ctx,
            conflict_dialog: params.conflict_dialog,
            fullscreen: params.fullscreen,
        });
    }
}
