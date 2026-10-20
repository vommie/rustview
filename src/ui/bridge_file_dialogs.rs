pub use super::bridge_file_dialog_conflict::{
    ConflictDialogParams, ConflictThumbInfo, FileOpsConflictDialogController,
};
pub use super::bridge_file_dialog_delete::DeleteFileDialogController;
pub use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
pub use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
pub use super::multiview::MultiviewHandle;
pub use super::bridge_inspector::BridgeInspector;
pub use super::bridge_media::BridgeMedia;
pub use super::bridge_state::UiState;
pub use super::bridge_status_bar::StatusBarBridge;
pub use super::bridge_tab_grid::BridgeTabGrid;
pub use super::bridge_viewer_tabs::BridgeViewerTabs;
pub use super::models::format_timestamp;
pub use super::{FileRenameDialog, MainWindow};
use crate::library::file_operations::FileOperations;
use crate::library::models::{FileConflictResolution, FileTransferKind, MediaId, MediaRecord};
use crate::library::Library;
use crate::media::FormatPolicy;
use crate::settings::{LogObscurer, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// Unified context bundle grouping storage, thumbnails, tabs, settings, and UI state dependencies for file dialogs.
#[derive(Clone)]
pub struct FileDialogContext {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub tab_manager: Arc<ViewerTabManager>,
    pub settings: Arc<SettingsManager>,
    pub state: Arc<RwLock<UiState>>,
}

// Backward-compatible alias for existing file rename flows
pub type FileRenameContext = FileDialogContext;

/// Structured parameters for opening the Rename Media File dialog.
pub struct RenameDialogParams<'a> {
    pub dialog_ref: &'a Rc<RefCell<Option<FileRenameDialog>>>,
    pub window: &'a MainWindow,
    pub media_id: MediaId,
    pub ctx: &'a FileDialogContext,
    pub conflict_dialog: &'a FileOpsConflictDialogController,
    pub missing_dialog: &'a MissingFileDialogController,
    pub fullscreen: &'a MultiviewHandle,
}

pub struct FileRenameDialogController;

impl FileRenameDialogController {
    /// Opens the modern smart rename dialog with real-time conflict checking, automatic suggestion filling, and conflict delegation.
    /// Supports pending stubs even if their physical file is not yet ready on disk.
    pub fn open(params: RenameDialogParams<'_>) {
        let dialog_ref = params.dialog_ref;
        let window = params.window;
        let media_id = params.media_id;
        let ctx = params.ctx;
        let conflict_dialog = params.conflict_dialog;
        let _missing_dialog = params.missing_dialog;
        let fullscreen = params.fullscreen;

        let current_target = {
            let st = ctx.state.read();
            st.all_folder_media
                .iter()
                .find(|m| m.record.id == media_id)
                .map(|m| (m.record.filename.clone(), m.full_path()))
                .or_else(|| {
                    let path = ctx.library.resolve_media_path(media_id).ok()?;
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string())?;
                    Some((name, path))
                })
                .or_else(|| {
                    let tab = ctx.tab_manager.get_tab_by_media_id(media_id)?;
                    Some((tab.filename, tab.file_path))
                })
        };

        let Some((current_name, full_path)) = current_target else {
            tracing::warn!(media_id, "[FileRenameController: Open] Failed to resolve target media path");
            return;
        };

        window.set_is_context_menu_open(false);
        let mut d_ref = dialog_ref.borrow_mut();
        if d_ref.is_none() {
            let win = FileRenameDialog::new().expect("Failed to instantiate FileRenameDialog");
            *d_ref = Some(win);
        }

        let Some(ref win) = *d_ref else { return; };

        win.set_initial_name(current_name.clone().into());
        win.set_new_name(current_name.clone().into());
        win.set_has_conflict(false);
        win.set_suggested_name("".into());
        win.set_conflict_message("".into());
        win.set_error_message("".into());

        let folder_path = full_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();

        // 1. Real-time conflict inspection as user edits new name
        let win_weak_edit = win.as_weak();
        let lib_edit = ctx.library.clone();
        let folder_edit = folder_path.clone();
        let orig_name_edit = current_name.clone();

        win.on_name_edited(move |typed| {
            let trimmed = typed.trim().to_string();
            if let Some(w) = win_weak_edit.upgrade() {
                if trimmed.is_empty() || trimmed == orig_name_edit {
                    w.set_has_conflict(false);
                    w.set_suggested_name("".into());
                    w.set_conflict_message("".into());
                    w.set_error_message("".into());
                    return;
                }

                match lib_edit.file_operations().check_rename_conflict(media_id, &trimmed) {
                    Ok(Some(conflict)) => {
                        let suggestion = FileOperations::generate_unique_filename(&folder_edit, &trimmed);
                        w.set_has_conflict(true);
                        w.set_suggested_name(suggestion.into());
                        w.set_conflict_message(format!("'{}' already exists in this folder.", conflict.destination.filename).into());
                        w.set_error_message("".into());
                    }
                    Ok(None) => {
                        w.set_has_conflict(false);
                        w.set_suggested_name("".into());
                        w.set_conflict_message("".into());
                        w.set_error_message("".into());
                    }
                    Err(e) => {
                        w.set_has_conflict(false);
                        w.set_error_message(e.to_string().into());
                    }
                }
            }
        });

        // 2. Immediate adoption of suggested unique name
        let win_weak_sug = win.as_weak();
        win.on_use_suggested_name(move || {
            if let Some(w) = win_weak_sug.upgrade() {
                let sug = w.get_suggested_name();
                if !sug.is_empty() {
                    w.set_new_name(sug);
                    w.set_has_conflict(false);
                    w.set_conflict_message("".into());
                    w.set_suggested_name("".into());
                    w.set_error_message("".into());
                }
            }
        });

        // 3. User confirmation handler (either direct rename or conflict delegation)
        let win_weak_conf = win.as_weak();
        let ctx_conf = ctx.clone();
        let win_main_conf = window.clone_strong();
        let conflict_dlg_conf = conflict_dialog.clone();
        let dlg_ref_conf = dialog_ref.clone();
        let fs_conf = fullscreen.clone();
        let target_id_conf = media_id;
        let orig_name_conf = current_name;

        win.on_confirmed(move |confirmed_name| {
            let trimmed = confirmed_name.trim().to_string();
            if trimmed.is_empty() || trimmed == orig_name_conf {
                Self::close(&dlg_ref_conf, &ctx_conf.state, &win_main_conf, &fs_conf);
                return;
            }

            match ctx_conf.library.file_operations().check_rename_conflict(target_id_conf, &trimmed) {
                Ok(Some(conflict_report)) => {
                    Self::close(&dlg_ref_conf, &ctx_conf.state, &win_main_conf, &fs_conf);

                    let c_report = conflict_report.clone();
                    let ctx_act = ctx_conf.clone();
                    let win_main_act = win_main_conf.clone_strong();
                    let fs_act = fs_conf.clone();
                    let name_act = trimmed.clone();
                    let suggestion = FileOperations::generate_unique_filename(
                        Path::new(&conflict_report.destination.folder_path),
                        &name_act,
                    );

                    conflict_dlg_conf.show(
                        ConflictDialogParams {
                            window: &win_main_conf,
                            thumbnails: &ctx_conf.thumbnails,
                            settings: &ctx_conf.settings,
                            report: &c_report,
                            conflict_index: 1,
                            total_conflicts: 1,
                            transfer_kind: FileTransferKind::Move,
                        },
                        move |resolution, _apply_all| {
                            match resolution {
                                FileConflictResolution::AutoRename => {
                                    Self::do_execute(
                                        target_id_conf,
                                        &suggestion,
                                        false,
                                        &ctx_act,
                                        &win_main_act,
                                        &fs_act,
                                    );
                                }
                                FileConflictResolution::Overwrite => {
                                    Self::do_execute(
                                        target_id_conf,
                                        &name_act,
                                        true,
                                        &ctx_act,
                                        &win_main_act,
                                        &fs_act,
                                    );
                                }
                                _ => {}
                            }
                        },
                    );
                }
                Ok(None) => {
                    Self::close(&dlg_ref_conf, &ctx_conf.state, &win_main_conf, &fs_conf);
                    Self::do_execute(
                        target_id_conf,
                        &trimmed,
                        false,
                        &ctx_conf,
                        &win_main_conf,
                        &fs_conf,
                    );
                }
                Err(e) => {
                    if let Some(w) = win_weak_conf.upgrade() {
                        w.set_error_message(e.to_string().into());
                    }
                }
            }
        });

        // 4. Cancel & Close Events
        let dlg_ref_cancel = dialog_ref.clone();
        let st_cancel = ctx.state.clone();
        let win_main_cancel = window.clone_strong();
        let fs_cancel = fullscreen.clone();
        win.on_cancelled(move || {
            Self::close(&dlg_ref_cancel, &st_cancel, &win_main_cancel, &fs_cancel);
        });

        let dlg_ref_close = dialog_ref.clone();
        let st_close = ctx.state.clone();
        let win_main_close = window.clone_strong();
        let fs_close = fullscreen.clone();
        win.window().on_close_requested(move || {
            Self::close(&dlg_ref_close, &st_close, &win_main_close, &fs_close);
            CloseRequestResponse::HideWindow
        });

        window.set_is_modal_locked(true);
        let _ = win.show();
    }

    /// Closes the rename dialog and safely restores modal locking depending on fullscreen state.
    pub fn close(
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
        fullscreen: &MultiviewHandle,
    ) {
        if let Some(ref win) = *dialog_ref.borrow() {
            let _ = win.hide();
        }
        state.write().active_file_rename_target = None;
        if !fullscreen.is_isolated_open() {
            window.set_is_modal_locked(false);
        }
    }

    /// Performs the atomic physical filesystem move and updates LMDB storage, viewers, and caches.
    fn do_execute(
        media_id: MediaId,
        new_name: &str,
        overwrite: bool,
        ctx: &FileDialogContext,
        window: &MainWindow,
        fullscreen: &MultiviewHandle,
    ) {
        match ctx
            .library
            .file_operations()
            .rename_media_with_overwrite(media_id, new_name, overwrite)
        {
            Ok(updated_record) => {
                tracing::info!(
                    media = %LogObscurer::media(Some(media_id), new_name),
                    overwrite = overwrite,
                    "[FileRenameController: Success] File successfully renamed"
                );

                let overwritten_id_opt = Self::update_in_memory_record(&ctx.state, media_id, &updated_record);
                if let Some(overwritten_id) = overwritten_id_opt {
                    ctx.tab_manager.notify_media_deleted(overwritten_id);
                }

                ctx.tab_manager.notify_media_renamed(media_id, new_name);
                ctx.tab_manager.notify_media_refreshed(media_id, &updated_record);

                fullscreen.sync_tab(&ctx.tab_manager);

                // A new extension can belong to a format the user switched off: the file leaves the lists at once.
                let hidden_by_policy = !FormatPolicy::mask().shows(updated_record.format);
                if hidden_by_policy {
                    StatusBarBridge::warn(
                        window,
                        format!(
                            "Renamed file to '{}'. {} files are switched off in Settings > File Formats, so it is hidden now.",
                            new_name,
                            updated_record.format.display_name()
                        ),
                    );
                } else {
                    StatusBarBridge::success(window, format!("Renamed file to '{}'", new_name));
                }
                BridgeInspector::update(window, &ctx.state);
                BridgeViewerTabs::sync_ui(
                    window,
                    &ctx.tab_manager,
                    &ctx.library,
                    &ctx.settings,
                    &ctx.state,
                );
                BridgeMedia::reload(
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.settings,
                    &ctx.state,
                    false,
                    hidden_by_policy,
                );

                // If in TabGrid mode (mode 2), also update all TabGrid masonry tiles
                if window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(
                        window,
                        &ctx.tab_manager,
                        &ctx.library,
                        &ctx.settings,
                        &ctx.state,
                    );
                }

                StatusBarBridge::update_metrics(window, &ctx.state);
            }
            Err(e) => {
                tracing::error!(
                    media_id = media_id,
                    error = %e,
                    "[FileRenameController: Error] Failed to rename file"
                );
                StatusBarBridge::error(window, format!("Failed to rename file: {}", e));
            }
        }
    }

    fn update_in_memory_record(
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
        updated_record: &MediaRecord,
    ) -> Option<MediaId> {
        let mut st = state.write();

        // 1. Identify and remove any overwritten record from all_folder_media
        let mut overwritten_id = None;
        if let Some(pos) = st.all_folder_media.iter().position(|m| {
            m.record.id != media_id
                && m.record.folder_id == updated_record.folder_id
                && m.record.filename.eq_ignore_ascii_case(&updated_record.filename)
        }) {
            let removed = st.all_folder_media.remove(pos);
            overwritten_id = Some(removed.record.id);
        }

        if let Some(old_id) = overwritten_id {
            st.file_selection.deselect_many(&std::collections::HashSet::from([old_id]));
            st.media_row_map.remove(&old_id);
            st.thumbs.remove(old_id);
        }

        // 2. Update the renamed media record
        if let Some(item) = st
            .all_folder_media
            .iter_mut()
            .find(|m| m.record.id == media_id)
        {
            item.set_record(updated_record.clone());
        }
        st.mark_list_bytes_dirty();

        // 3. Invalidate search cache and slice to guarantee full clean reload
        st.active_indices.clear();
        st.mark_list_bytes_dirty();
        st.last_executed_query.clear();
        st.invalidate_view_window();

        overwritten_id
    }
}
