use crate::ui::file_dialogs::FileDialogContext;
use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::selection::delete::missing_files_hint;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tags::BridgeTags;
use crate::ui::{DeleteFileDialog, MainWindow};
use rustview_core::library::models::MediaId;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::SaturatingAs;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// What runs once the dialog deleted the files, before the viewers are told (UI thread); the Multiview uses it to drop the tiles it
/// asked to delete.
pub type DeleteFollowup = Rc<dyn Fn(&[MediaId])>;

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
    pub fn show(&self, window: &MainWindow, media_ids: Vec<MediaId>, ctx: FileDialogContext) {
        self.show_with_followup(window, media_ids, ctx, None);
    }

    /// Like `show`; `followup` runs once the files are deleted (not when the dialog is cancelled).
    pub fn show_with_followup(
        &self,
        window: &MainWindow,
        media_ids: Vec<MediaId>,
        ctx: FileDialogContext,
        followup: Option<DeleteFollowup>,
    ) {
        if media_ids.is_empty() {
            return;
        }

        window.set_is_context_menu_open(false);
        self.ensure_dialog_instantiated();

        let count = media_ids.len();
        let (first_name, missing) = {
            let st = ctx.state.read();
            let mut name = String::new();
            let mut missing = 0usize;
            for (idx, &id) in media_ids.iter().enumerate() {
                if let Some(item) = st.all_folder_media.find(id) {
                    if idx == 0 {
                        name.clone_from(&item.record.filename);
                    }
                    // Flagged by a refresh, or gone from the drive since (the same test the combined dialog makes).
                    if item.record.is_deleted || !item.full_path().exists() {
                        missing += 1;
                    }
                } else if let Ok(Some(rec)) = ctx.library.media().get(id) {
                    if idx == 0 {
                        name.clone_from(&rec.filename);
                    }
                    if rec.is_deleted || ctx.library.resolve_media_path(id).is_ok_and(|p| !p.exists()) {
                        missing += 1;
                    }
                } else if let Some(tab) = ctx.tab_manager.get_tab_by_media_id(id) {
                    if idx == 0 {
                        name.clone_from(&tab.filename);
                    }
                    if !tab.file_path.exists() {
                        missing += 1;
                    }
                }
            }
            (name, missing)
        };

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, media_ids, ctx, followup);

            win.set_file_name(first_name.into());
            win.set_files_count(count.saturating_as::<i32>());
            win.set_missing_note(missing_files_hint(missing, count).unwrap_or_default().into());
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
        followup: Option<&DeleteFollowup>,
    ) {
        let count = media_ids.len();
        let mut paths_to_delete = Vec::with_capacity(count);

        {
            let st = ctx.state.read();
            for &id in media_ids {
                if let Some(item) = st.all_folder_media.find(id) {
                    paths_to_delete.push(item.full_path());
                } else if let Ok(p) = ctx.library.resolve_media_path(id) {
                    paths_to_delete.push(p);
                } else if let Some(tab) = ctx.tab_manager.get_tab_by_media_id(id) {
                    paths_to_delete.push(tab.file_path.clone());
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

        // 3. The Multiview that asked for the deletion lets the tiles leave by its regular way (a playing player torn down
        // as an error tile under a visible window ended the application), then the viewers hear about it.
        if let Some(followup) = followup {
            followup(media_ids);
        }
        for &id in media_ids {
            ctx.tab_manager.notify_media_deleted(id);
        }

        let id_set: HashSet<MediaId> = media_ids.iter().copied().collect();
        ctx.state.write().remove_media(&id_set);

        self.close(window);

        let verb = if to_trash {
            "Moved to Trash"
        } else {
            "Permanently deleted"
        };
        let msg = if count == 1 {
            format!("{verb} 1 file.")
        } else {
            format!("{verb} {count} files.")
        };
        StatusBarBridge::success(window, msg);

        BridgeViewerTabs::sync_ui(window, &ctx.tab_manager, &ctx.library, &ctx.settings, &ctx.state);
        BridgeMedia::reload(
            window,
            &ctx.library,
            &ctx.thumbnails,
            &ctx.settings,
            &ctx.state,
            false,
            false,
        );
        BridgeInspector::update(window, &ctx.state);
        BridgeTags::sync_tag_selection(window, &ctx.library, &ctx.state);

        if window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(window, &ctx.tab_manager, &ctx.library, &ctx.settings, &ctx.state);
        }

        StatusBarBridge::update_metrics(window, &ctx.state);
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = DeleteFileDialog::new().expect("Failed to instantiate DeleteFileDialog");
            crate::ui::dialog_fit::install(&win);
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &DeleteFileDialog,
        window: &MainWindow,
        media_ids: Vec<MediaId>,
        ctx: FileDialogContext,
        followup: Option<DeleteFollowup>,
    ) {
        let ctrl_trash = self.clone();
        let win_trash = window.clone_strong();
        let ids_trash = media_ids.clone();
        let ctx_trash = ctx.clone();
        let followup_trash = followup.clone();

        win.on_move_to_trash_confirmed(move || {
            ctrl_trash.execute(&ids_trash, true, &ctx_trash, &win_trash, followup_trash.as_ref());
        });

        let ctrl_perm = self.clone();
        let win_perm = window.clone_strong();
        let ids_perm = media_ids;
        let ctx_perm = ctx;

        win.on_permanent_delete_confirmed(move || {
            ctrl_perm.execute(&ids_perm, false, &ctx_perm, &win_perm, followup.as_ref());
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
