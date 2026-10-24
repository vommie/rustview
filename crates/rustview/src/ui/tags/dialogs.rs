use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::tags::history_bridge::BridgeTagHistory;
use crate::ui::tags::{BridgeTags, TagActionDialogs};
use crate::ui::{AddTagDialog, DeleteTagDialog, MainWindow, RenameTagDialog};
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::TagId;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct DeleteTagParams {
    pub tag_id: TagId,
    pub tag_name: String,
    pub children: Vec<String>,
}

#[derive(Clone)]
pub struct TagDialogContext {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub settings: Arc<SettingsManager>,
    pub state: Arc<RwLock<UiState>>,
}

#[derive(Default, Clone)]
pub struct TagDialogController {
    add: Rc<RefCell<Option<AddTagDialog>>>,
    rename: Rc<RefCell<Option<RenameTagDialog>>>,
    delete: Rc<RefCell<Option<DeleteTagDialog>>>,
}

impl TagDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_action_bundle(
        &self,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) -> TagActionDialogs {
        let ctrl_add = self.clone();
        let win_add = window.clone_strong();
        let lib_add = library.clone();
        let set_add = settings.clone();
        let st_add = state.clone();

        let show_add = Box::new(move |parent_id: Option<TagId>, parent_info: &str| {
            ctrl_add.show_add(
                &win_add,
                parent_id,
                parent_info,
                &lib_add.clone(),
                &set_add.clone(),
                &st_add.clone(),
            );
        });

        let ctrl_ren = self.clone();
        let win_ren = window.clone_strong();
        let lib_ren = library.clone();
        let st_ren = state.clone();

        let show_rename = Box::new(move |tag_id: TagId, current_name: &str| {
            ctrl_ren.show_rename(&win_ren, tag_id, current_name, &lib_ren.clone(), &st_ren.clone());
        });

        let ctrl_del = self.clone();
        let win_del = window.clone_strong();
        let lib_del = library.clone();
        let thumb_del = thumbnails.clone();
        let set_del = settings.clone();
        let st_del = state.clone();

        let show_delete = Box::new(move |tag_id: TagId, tag_name: &str, children: Vec<String>| {
            ctrl_del.show_delete(
                &win_del,
                DeleteTagParams {
                    tag_id,
                    tag_name: tag_name.to_string(),
                    children,
                },
                TagDialogContext {
                    library: lib_del.clone(),
                    thumbnails: thumb_del.clone(),
                    settings: set_del.clone(),
                    state: st_del.clone(),
                },
            );
        });

        TagActionDialogs {
            show_add,
            show_rename,
            show_delete,
        }
    }

    // =========================================================================
    // Add Tag Dialog
    // =========================================================================

    pub fn show_add(
        &self,
        window: &MainWindow,
        parent_id: Option<TagId>,
        parent_info: &str,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let mut d_ref = self.add.borrow_mut();
        if d_ref.is_none() {
            let win = AddTagDialog::new().expect("Failed to instantiate AddTagDialog");
            crate::ui::dialog_fit::install(&win);
            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            let ctrl_confirm = self.clone();
            let win_confirm = window.clone_strong();
            let lib_confirm = library.clone();
            let set_confirm = settings.clone();
            let st_confirm = state.clone();
            win.on_confirmed(move |name| {
                ctrl_confirm.execute_add(&name, parent_id, &lib_confirm, &set_confirm, &st_confirm, &win_confirm);
            });

            let ctrl_cancel = self.clone();
            let win_cancel = window.clone_strong();
            win.on_cancelled(move || {
                ctrl_cancel.close_add(&win_cancel);
            });

            let ctrl_close = self.clone();
            let win_close = window.clone_strong();
            win.window().on_close_requested(move || {
                ctrl_close.close_add(&win_close);
                CloseRequestResponse::HideWindow
            });

            win.set_parent_info(parent_info.into());
            win.set_tag_name("".into());
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    pub fn close_add(&self, window: &MainWindow) {
        if let Some(ref win) = *self.add.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    pub fn execute_add(
        &self,
        name: &str,
        parent_id: Option<TagId>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
    ) {
        tracing::info!(
            tag = %LogObscurer::tag(None, name),
            parent_id = ?parent_id,
            "[TagManager: Create] Executing tag creation in LMDB"
        );

        match library.tags().create_tag(name, parent_id) {
            Ok(created) => {
                if let Some(pid) = parent_id {
                    let mut st = state.write();
                    st.expanded_tag_ids.insert(pid);
                    BridgeTags::persist_expanded_tags(settings, &st.expanded_tag_ids);
                }
                self.close_add(window);
                window.set_status_text(format!("Created tag '{}'", created.name).into());
                BridgeTags::reload_tags(window, library, state);
            }
            Err(e) => {
                tracing::warn!(error = %e, "[TagManager: Create Error] Failed to create tag");
                if let Some(ref win) = *self.add.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    // =========================================================================
    // Rename Tag Dialog
    // =========================================================================

    pub fn show_rename(
        &self,
        window: &MainWindow,
        tag_id: TagId,
        current_name: &str,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let mut d_ref = self.rename.borrow_mut();
        if d_ref.is_none() {
            let win = RenameTagDialog::new().expect("Failed to instantiate RenameTagDialog");
            crate::ui::dialog_fit::install(&win);
            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            let ctrl_confirm = self.clone();
            let win_confirm = window.clone_strong();
            let lib_confirm = library.clone();
            let st_confirm = state.clone();
            win.on_confirmed(move |name| {
                ctrl_confirm.execute_rename(tag_id, &name, &lib_confirm, &st_confirm, &win_confirm);
            });

            let ctrl_cancel = self.clone();
            let win_cancel = window.clone_strong();
            win.on_cancelled(move || {
                ctrl_cancel.close_rename(&win_cancel);
            });

            let ctrl_close = self.clone();
            let win_close = window.clone_strong();
            win.window().on_close_requested(move || {
                ctrl_close.close_rename(&win_close);
                CloseRequestResponse::HideWindow
            });

            win.set_initial_name(current_name.into());
            win.set_new_name(current_name.into());
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    pub fn close_rename(&self, window: &MainWindow) {
        if let Some(ref win) = *self.rename.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    pub fn execute_rename(
        &self,
        tag_id: TagId,
        new_name: &str,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
    ) {
        tracing::info!(
            tag = %LogObscurer::tag(Some(tag_id), new_name),
            "[TagManager: Rename] Renaming tag in LMDB"
        );

        match library.tags().rename_tag(tag_id, new_name) {
            Ok(updated) => {
                self.close_rename(window);
                window.set_status_text(format!("Renamed tag to '{}'", updated.name).into());
                BridgeTags::reload_tags(window, library, state);
                BridgeTagHistory::sync_ui(window, library, state);
            }
            Err(e) => {
                tracing::warn!(error = %e, "[TagManager: Rename Error] Failed to rename tag");
                if let Some(ref win) = *self.rename.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }

    // =========================================================================
    // Delete Tag Dialog
    // =========================================================================

    pub fn show_delete(&self, window: &MainWindow, params: DeleteTagParams, ctx: TagDialogContext) {
        let mut d_ref = self.delete.borrow_mut();
        if d_ref.is_none() {
            let win = DeleteTagDialog::new().expect("Failed to instantiate DeleteTagDialog");
            crate::ui::dialog_fit::install(&win);
            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            let ctrl_confirm = self.clone();
            let win_confirm = window.clone_strong();
            let tag_id = params.tag_id;
            let ctx_confirm = ctx;

            win.on_confirmed(move || {
                ctrl_confirm.execute_delete(
                    tag_id,
                    &ctx_confirm.library,
                    &ctx_confirm.thumbnails,
                    &ctx_confirm.settings,
                    &ctx_confirm.state,
                    &win_confirm,
                );
            });

            let ctrl_cancel = self.clone();
            let win_cancel = window.clone_strong();
            win.on_cancelled(move || {
                ctrl_cancel.close_delete(&win_cancel);
            });

            let ctrl_close = self.clone();
            let win_close = window.clone_strong();
            win.window().on_close_requested(move || {
                ctrl_close.close_delete(&win_close);
                CloseRequestResponse::HideWindow
            });

            win.set_tag_name(params.tag_name.into());
            win.set_has_children(!params.children.is_empty());
            win.set_children_names(params.children.join(", ").into());
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    pub fn close_delete(&self, window: &MainWindow) {
        if let Some(ref win) = *self.delete.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    pub fn execute_delete(
        &self,
        tag_id: TagId,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
    ) {
        tracing::info!(
            tag = %LogObscurer::tag_id(tag_id),
            "[TagManager: Delete] Purging tag subtree in LMDB"
        );

        match library.tags().delete_tag_subtree(tag_id) {
            Ok(deleted) => {
                let deleted_ids_set: HashSet<TagId> = deleted.iter().map(|d| d.id).collect();

                {
                    let mut st = state.write();
                    for d in &deleted {
                        st.filter.tag_ids.remove(&d.id);
                        st.expanded_tag_ids.remove(&d.id);
                    }
                    BridgeTags::persist_expanded_tags(settings, &st.expanded_tag_ids);
                }

                // Automatically clean up deleted tags from history
                BridgeTagHistory::prune_deleted_tags(&deleted_ids_set, settings, state);

                self.close_delete(window);
                window.set_status_text(format!("Deleted {} tag(s)", deleted.len()).into());

                BridgeTags::reload_tags(window, library, state);
                BridgeTagHistory::sync_ui(window, library, state);
                BridgeInspector::update(window, state);
                BridgeMedia::reload(window, library, thumbnails, settings, state, false, true);
            }
            Err(e) => {
                tracing::warn!(error = %e, "[TagManager: Delete Error] Failed to delete tag");
                if let Some(ref win) = *self.delete.borrow() {
                    win.set_error_message(e.to_string().into());
                }
            }
        }
    }
}
