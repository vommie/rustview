use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::{FileRenameDialog, MainWindow};
use crate::library::Library;
use crate::library::models::{MediaId, MediaRecord};
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub struct FileRenameContext {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub settings: Arc<SettingsManager>,
    pub state: Arc<RwLock<UiState>>,
}

pub struct FileRenameDialogController;

impl FileRenameDialogController {
    pub fn show<F1, F2, F3>(
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
        window: &MainWindow,
        current_name: &str,
        on_confirm: F1,
        on_cancel: F2,
        on_close: F3,
    ) where
        F1: Fn(String) + 'static,
        F2: Fn() + 'static,
        F3: Fn() + 'static,
    {
        window.set_is_context_menu_open(false);
        let mut d_ref = dialog_ref.borrow_mut();
        if d_ref.is_none() {
            let win = FileRenameDialog::new().expect("Failed to instantiate FileRenameDialog");
            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            win.on_confirmed(move |new_name| {
                on_confirm(new_name.to_string());
            });
            win.on_cancelled(on_cancel);
            win.window().on_close_requested(move || {
                on_close();
                CloseRequestResponse::HideWindow
            });

            win.set_initial_name(current_name.into());
            win.set_new_name(current_name.into());
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    pub fn close(
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
    ) {
        if let Some(ref win) = *dialog_ref.borrow() {
            let _ = win.hide();
        }
        state.write().active_file_rename_target = None;
        window.set_is_modal_locked(false);
    }

    pub fn execute(
        media_id: MediaId,
        new_name: &str,
        ctx: &FileRenameContext,
        window: &MainWindow,
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
    ) {
        match ctx
            .library
            .file_operations()
            .rename_media(media_id, new_name)
        {
            Ok(updated_record) => {
                tracing::info!(
                    media_id = media_id,
                    new_name = %new_name,
                    "File successfully renamed via FileOperations subsystem"
                );

                Self::update_in_memory_record(&ctx.state, media_id, &updated_record);
                Self::close(dialog_ref, &ctx.state, window);

                window.set_status_text(format!("Renamed file to '{}'", new_name).into());
                BridgeInspector::update(window, &ctx.state);
                BridgeMedia::reload(
                    window,
                    &ctx.library,
                    &ctx.thumbnails,
                    &ctx.settings,
                    &ctx.state,
                    false,
                    false,
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to rename media file");
                if let Some(ref win) = *dialog_ref.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    fn update_in_memory_record(
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
        updated_record: &MediaRecord,
    ) {
        let mut st = state.write();
        let folder_path_opt = st.current_folder_path.clone();

        if let Some(item) = st
            .all_folder_media
            .iter_mut()
            .find(|m| m.record.id == media_id)
        {
            item.filename_slint = updated_record.filename.clone().into();
            item.filename_lower = updated_record.filename.to_lowercase();
            if let Some(ref folder_path) = folder_path_opt {
                item.full_path = folder_path.join(&updated_record.filename);
            }
            item.record = updated_record.clone();
        }
    }
}
