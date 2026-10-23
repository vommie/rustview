use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::folder_tree::move_sync::BridgeFolderMoveSync;
use crate::ui::{MainWindow, MoveFolderToDialog};
use rustview_core::library::models::FolderId;
use rustview_core::util::cast::SaturatingAs;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::rc::Rc;

/// Specialized controller managing the Move/Reparent Folder modal dialog and execution.
#[derive(Default, Clone)]
pub struct MoveFolderDialogController {
    dialog: Rc<RefCell<Option<MoveFolderToDialog>>>,
}

impl MoveFolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the Move Folder dialog confirming relocation under destination folder.
    pub fn show(
        &self,
        window: &MainWindow,
        src_folder_id: FolderId,
        dst_folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        window.set_is_context_menu_open(false);

        let (src_name, metrics) = match (
            ctx.library.folders().get_folder(src_folder_id),
            ctx.library.folder_operations().get_metrics(src_folder_id),
        ) {
            (Ok(Some(src)), Ok(m)) => (src.name, m),
            _ => return,
        };

        let dst_name = match ctx.library.folders().get_folder(dst_folder_id) {
            Ok(Some(dst)) => dst.name,
            _ => return,
        };

        self.ensure_dialog_instantiated();

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, src_folder_id, dst_folder_id, ctx);

            win.set_source_folder_name(src_name.into());
            win.set_target_folder_name(dst_name.into());
            win.set_subfolders_count(metrics.subfolders_count.saturating_as::<i32>());
            win.set_files_count(metrics.media_files_count.saturating_as::<i32>());
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

    /// Executes the physical relocation (atomic with the database update) and then applies the result to the
    /// in-memory snapshot and the views surgically, without rebuilding the whole folder tree.
    pub fn execute(
        &self,
        src_folder_id: FolderId,
        dst_folder_id: FolderId,
        ctx: &FolderDialogContext,
        window: &MainWindow,
    ) {
        if let Some(reason) = Self::busy_reason(ctx) {
            tracing::warn!(
                src_folder_id,
                dst_folder_id,
                reason,
                "[FolderMove: Rejected] Folder move rejected because another operation is running"
            );
            if let Some(ref win) = *self.dialog.borrow() {
                win.set_error_message(reason.into());
            }
            return;
        }

        tracing::info!(
            src_folder_id,
            dst_folder_id,
            "[FolderMove: Request] User confirmed folder move"
        );

        match ctx
            .library
            .folder_operations()
            .move_folder(src_folder_id, dst_folder_id)
        {
            Ok(report) => {
                self.close(window);
                BridgeFolderMoveSync::apply(&report, window, ctx);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to move folder");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    /// A folder move must not run while a folder tree scan or media refresh is changing the same data.
    fn busy_reason(ctx: &FolderDialogContext) -> Option<&'static str> {
        let st = ctx.state.read();
        if st.is_refreshing_folders {
            Some("A folder tree scan is currently running. Please wait until it has finished.")
        } else if st.is_refreshing_media {
            Some("A media refresh is currently running. Please wait until it has finished.")
        } else {
            None
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = MoveFolderToDialog::new().expect("Failed to instantiate MoveFolderToDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &MoveFolderToDialog,
        window: &MainWindow,
        src_folder_id: FolderId,
        dst_folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        let ctx_confirm = ctx;

        win.on_confirmed(move || {
            ctrl_confirm.execute(src_folder_id, dst_folder_id, &ctx_confirm, &win_confirm);
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
