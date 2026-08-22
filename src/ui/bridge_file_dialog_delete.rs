use super::bridge_file_dialogs::FileDialogContext;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_tags::BridgeTags;
use super::models::format_filesize;
use super::{DeleteFileDialog, MainWindow};
use crate::library::models::MediaId;
use crate::settings::LogObscurer;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Specialized controller managing active media file deletion (Trash / Permanent Delete).
#[derive(Default, Clone)]
pub struct DeleteFileDialogController {
    dialog: Rc<RefCell<Option<DeleteFileDialog>>>,
}

impl DeleteFileDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the Delete File modal dialog.
    pub fn show(
        &self,
        window: &MainWindow,
        media_ids: Vec<MediaId>,
        ctx: FileDialogContext,
    ) {
        if media_ids.is_empty() {
            return;
        }

        window.set_is_context_menu_open(false);
        self.ensure_dialog_instantiated();

        let count = media_ids.len();
        let (first_name, total_bytes) = {
            let st = ctx.state.read();
            let mut total = 0u64;
            let mut name = String::new();
            for (idx, &id) in media_ids.iter().enumerate() {
                if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id) {
                    if idx == 0 {
                        name = item.record.filename.clone();
                    }
                    total += item.record.file_size;
                } else if let Ok(Some(rec)) = ctx.library.media().get(id) {
                    if idx == 0 {
                        name = rec.filename.clone();
                    }
                    total += rec.file_size;
                }
            }
            (name, total)
        };

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, media_ids, ctx);

            win.set_file_name(first_name.into());
            win.set_files_count(count as i32);
            win.set_file_size_str(format_filesize(total_bytes).into());
            win.set_error_message("".into());

            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the dialog and releases modal lock on the main application window.
    pub fn close(&self, window: &MainWindow) {
        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    /// Executes file deletion physically on disk (Trash or Permanent) and removes records from LMDB.
    pub fn execute(
        &self,
        media_ids: &[MediaId],
        to_trash: bool,
        ctx: &FileDialogContext,
        window: &MainWindow,
    ) {
        let count = media_ids.len();
        let mut paths_to_delete = Vec::with_capacity(count);

        {
            let st = ctx.state.read();
            for &id in media_ids {
                if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id) {
                    paths_to_delete.push(item.full_path.clone());
                } else if let Ok(p) = ctx.library.resolve_media_path(id) {
                    paths_to_delete.push(p);
                }
            }
        }

        tracing::info!(
            count,
            to_trash,
            "[DeleteFileDialog: Execute] Deleting file(s) on disk and purging database records..."
        );

        // 1. Physical Filesystem Deletion
        for path in &paths_to_delete {
            if path.exists() {
                if to_trash {
                    if let Err(e) = trash::delete(path) {
                        tracing::warn!(path = %LogObscurer::path(path), error = %e, "Failed to move file to trash");
                    }
                } else {
                    let _ = std::fs::remove_file(path);
                }
            }
        }

        // 2. Database Purge
        let _ = ctx.library.file_operations().purge_media_from_db(media_ids);

        let id_set: HashSet<MediaId> = media_ids.iter().copied().collect();
        {
            let mut st = ctx.state.write();
            st.all_folder_media.retain(|m| !id_set.contains(&m.record.id));
            st.file_selection.clear();
        }

        self.close(window);

        let verb = if to_trash { "Moved to Trash" } else { "Permanently deleted" };
        window.set_status_text(
            if count == 1 {
                format!("{} 1 file.", verb).into()
            } else {
                format!("{} {} files.", verb, count).into()
            },
        );

        BridgeMedia::reload(window, &ctx.library, &ctx.thumbnails, &ctx.settings, &ctx.state, false, false);
        BridgeInspector::update(window, &ctx.state);
        BridgeTags::sync_tag_selection(window, &ctx.library, &ctx.state);
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = DeleteFileDialog::new().expect("Failed to instantiate DeleteFileDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &DeleteFileDialog,
        window: &MainWindow,
        media_ids: Vec<MediaId>,
        ctx: FileDialogContext,
    ) {
        let ctrl_trash = self.clone();
        let win_trash = window.clone_strong();
        let ids_trash = media_ids.clone();
        let ctx_trash = ctx.clone();

        win.on_move_to_trash_confirmed(move || {
            ctrl_trash.execute(&ids_trash, true, &ctx_trash, &win_trash);
        });

        let ctrl_perm = self.clone();
        let win_perm = window.clone_strong();
        let ids_perm = media_ids;
        let ctx_perm = ctx;

        win.on_permanent_delete_confirmed(move || {
            ctrl_perm.execute(&ids_perm, false, &ctx_perm, &win_perm);
        });

        let ctrl_cancel = self.clone();
        let win_cancel = window.clone_strong();
        win.on_cancelled(move || {
            ctrl_cancel.close(&win_cancel);
        });

        let ctrl_close = self.clone();
        let win_close = window.clone_strong();
        win.window().on_close_requested(move || {
            ctrl_close.close(&win_close);
            CloseRequestResponse::HideWindow
        });
    }
}
