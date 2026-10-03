use super::bridge_file_dialogs::FileDialogContext;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tab_grid::BridgeTabGrid;
use super::bridge_tags::BridgeTags;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::{MainWindow, RemoveOrphanedFileDialog};
use crate::library::models::MediaId;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Specialized controller managing database cleanup for missing/orphaned media files.
#[derive(Default, Clone)]
pub struct RemoveOrphanedFileDialogController {
    dialog: Rc<RefCell<Option<RemoveOrphanedFileDialog>>>,
}

impl RemoveOrphanedFileDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the confirmation dialog to remove missing file records from the database.
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
        let first_name = {
            let st = ctx.state.read();
            media_ids
                .first()
                .and_then(|&id| {
                    st.all_folder_media
                        .iter()
                        .find(|m| m.record.id == id)
                        .map(|m| m.record.filename.clone())
                })
                .or_else(|| {
                    media_ids.first().and_then(|&id| {
                        ctx.tab_manager.get_tab_by_media_id(id).map(|t| t.filename)
                    })
                })
                .unwrap_or_else(|| "Selected file".to_string())
        };

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, media_ids, ctx);

            win.set_file_name(first_name.into());
            win.set_files_count(count as i32);
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

    /// Executes database purge of the missing media file records from LMDB.
    pub fn execute(
        &self,
        media_ids: &[MediaId],
        ctx: &FileDialogContext,
        window: &MainWindow,
    ) {
        let count = media_ids.len();
        let _ = ctx.library.file_operations().purge_media_from_db(media_ids);

        for &id in media_ids {
            ctx.tab_manager.notify_media_deleted(id);
        }

        let id_set: HashSet<MediaId> = media_ids.iter().copied().collect();
        {
            let mut st = ctx.state.write();
            st.all_folder_media.retain(|m| !id_set.contains(&m.record.id));
            st.file_selection.clear();
        }

        self.close(window);

        let msg = if count == 1 {
            "Removed 1 missing file from database.".to_string()
        } else {
            format!("Removed {} missing files from database.", count)
        };
        StatusBarBridge::success(window, msg);

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
            false,
        );
        BridgeInspector::update(window, &ctx.state);
        BridgeTags::sync_tag_selection(window, &ctx.library, &ctx.state);

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

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = RemoveOrphanedFileDialog::new()
                .expect("Failed to instantiate RemoveOrphanedFileDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &RemoveOrphanedFileDialog,
        window: &MainWindow,
        media_ids: Vec<MediaId>,
        ctx: FileDialogContext,
    ) {
        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        let ids_confirm = media_ids;
        let ctx_confirm = ctx;

        win.on_confirmed(move || {
            ctrl_confirm.execute(&ids_confirm, &ctx_confirm, &win_confirm);
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
