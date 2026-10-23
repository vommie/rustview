use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::{MainWindow, RenameFolderDialog};
use parking_lot::RwLock;
use rustview_core::library::folder_operations::FolderOperations;
use rustview_core::library::models::FolderId;
use rustview_core::query::FolderDir;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Specialized controller managing the Rename Folder modal dialog lifecycle and execution.
#[derive(Default, Clone)]
pub struct RenameFolderDialogController {
    dialog: Rc<RefCell<Option<RenameFolderDialog>>>,
}

impl RenameFolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Displays the Rename Folder modal dialog with pre-populated name and real-time conflict checking.
    pub fn show(&self, window: &MainWindow, folder_id: FolderId, current_name: &str, ctx: FolderDialogContext) {
        window.set_is_context_menu_open(false);
        self.ensure_dialog_instantiated();

        let parent_path_opt = ctx
            .library
            .folders()
            .get_folder(folder_id)
            .ok()
            .flatten()
            .and_then(|f| PathBuf::from(&f.path).parent().map(std::path::Path::to_path_buf));

        if let Some(ref win) = *self.dialog.borrow() {
            self.bind_dialog_events(win, window, folder_id, parent_path_opt, ctx);
            self.populate_dialog_fields(win, current_name);

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

    /// Executes atomic folder renaming on disk and updates LMDB path indices recursively.
    pub fn execute(&self, folder_id: FolderId, new_name: &str, ctx: &FolderDialogContext, window: &MainWindow) {
        match ctx.library.folder_operations().rename_folder(folder_id, new_name) {
            Ok(updated) => {
                let is_active = self.update_in_memory_state(folder_id, &updated.path, &ctx.state);

                self.close(window);
                StatusBarBridge::success(window, format!("Renamed folder to '{new_name}'"));

                BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
                if is_active {
                    BridgeMedia::reload(
                        window,
                        &ctx.library,
                        &ctx.thumbnails,
                        &ctx.settings,
                        &ctx.state,
                        true,
                        true,
                    );
                }
                StatusBarBridge::update_metrics(window, &ctx.state);
            }
            Err(e) => {
                tracing::warn!(folder_id = folder_id, error = %e, "Failed to rename folder");
                if let Some(ref win) = *self.dialog.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            let win = RenameFolderDialog::new().expect("Failed to instantiate RenameFolderDialog");
            *d_ref = Some(win);
        }
    }

    fn bind_dialog_events(
        &self,
        win: &RenameFolderDialog,
        window: &MainWindow,
        folder_id: FolderId,
        parent_path_opt: Option<PathBuf>,
        ctx: FolderDialogContext,
    ) {
        let win_weak_edit = win.as_weak();
        let lib_edit = ctx.library.clone();
        let parent_p_edit = parent_path_opt;

        win.on_name_edited(move |typed| {
            let trimmed = typed.trim().to_string();
            if let Some(w) = win_weak_edit.upgrade() {
                if trimmed.is_empty() {
                    w.set_has_conflict(false);
                    w.set_suggested_name("".into());
                    w.set_conflict_message("".into());
                    w.set_error_message("".into());
                    return;
                }

                match lib_edit.folder_operations().check_rename_conflict(folder_id, &trimmed) {
                    Ok(Some(conflict_path)) => {
                        let folder_name = conflict_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or(trimmed.clone());

                        let parent = parent_p_edit.as_deref().unwrap_or_else(|| Path::new("/"));
                        let suggestion = FolderOperations::generate_unique_folder_name(parent, &trimmed);

                        w.set_has_conflict(true);
                        w.set_suggested_name(suggestion.into());
                        w.set_conflict_message(
                            format!("Folder '{folder_name}' already exists in parent directory.").into(),
                        );
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

        let ctrl_confirm = self.clone();
        let win_confirm = window.clone_strong();
        let ctx_confirm = ctx;

        win.on_confirmed(move |name| {
            ctrl_confirm.execute(folder_id, &name, &ctx_confirm, &win_confirm);
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

    fn populate_dialog_fields(&self, win: &RenameFolderDialog, current_name: &str) {
        win.set_initial_name(current_name.into());
        win.set_new_name(current_name.into());
        win.set_has_conflict(false);
        win.set_suggested_name("".into());
        win.set_conflict_message("".into());
        win.set_error_message("".into());
    }

    fn update_in_memory_state(&self, folder_id: FolderId, new_path_str: &str, state: &RwLock<UiState>) -> bool {
        let is_active = state.read().current_folder_id == Some(folder_id);
        let new_path = PathBuf::from(new_path_str);
        let mut st = state.write();

        if is_active {
            st.current_folder_path = Some(new_path.clone());
        }

        let folder_dir = FolderDir::new(new_path);
        for item in st.all_folder_media.iter_mut() {
            if item.record.folder_id == folder_id {
                item.set_folder(folder_dir.clone());
            }
        }

        st.invalidate_folder_hierarchy();
        is_active
    }
}
