use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::file_selection::FileSelectionModel;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher};
use crate::library::Library;
use crate::library::models::MediaId;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Subsystem responsible for context menu construction, action dispatching, and rename operations.
pub struct BridgeContextMenu;

impl BridgeContextMenu {
    /// Builds and displays the dynamic context menu at the cursor position.
    pub fn show(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
        x: f32,
        y: f32,
    ) {
        let t_start = Instant::now();
        let (selected_count, is_single, can_rename, changed_ids) = {
            let mut st = state.write();
            let mut changed = Vec::new();
            if !st.file_selection.is_selected(media_id) {
                let prev = st.file_selection.selected_ids().clone();
                st.file_selection.select_single(media_id);
                let newly = st.file_selection.selected_ids().clone();
                changed = FileSelectionModel::compute_selection_diff(&prev, &newly);
            }
            let count = st.file_selection.count();
            (count, count == 1, count == 1, changed)
        };

        if !changed_ids.is_empty() {
            BridgeFileSelection::sync_selection_diff(window, state, &changed_ids);
        }
        BridgeInspector::update(window, state);

        let menu = CtxMenuBuilder::new()
            .header(if is_single {
                "Selected File"
            } else {
                "Selected Files"
            })
            .action(
                CTX_ACTION_RENAME,
                if can_rename {
                    "Rename...".to_string()
                } else {
                    format!("Rename (Single File Only, {} selected)", selected_count)
                },
                Some("F2"),
                Some("edit"),
                can_rename,
            )
            .separator()
            .submenu("Rate Selection", Some("star"), selected_count > 0, |sub| {
                sub.action(CTX_ACTION_RATE_5, "5 Stars (★★★★★)", None, None, true)
                    .action(CTX_ACTION_RATE_4, "4 Stars (★★★★☆)", None, None, true)
                    .action(CTX_ACTION_RATE_3, "3 Stars (★★★☆☆)", None, None, true)
                    .action(CTX_ACTION_RATE_2, "2 Stars (★★☆☆☆)", None, None, true)
                    .action(CTX_ACTION_RATE_1, "1 Star  (★☆☆☆☆)", None, None, true)
                    .separator()
                    .action(CTX_ACTION_RATE_0, "Clear Rating", None, None, true)
            })
            .separator()
            .action(
                CTX_ACTION_CLEAR_SELECTION,
                "Clear Selection",
                Some("Esc"),
                None,
                selected_count > 0,
            )
            .action(
                CTX_ACTION_SELECT_ALL,
                "Select All",
                Some("Ctrl+A"),
                None,
                true,
            )
            .build();

        let (top_model, sub_model) = CtxMenuDispatcher::to_slint_models(&menu);

        window.set_context_menu_items(top_model);
        window.set_context_submenu_items(sub_model);
        window.set_context_menu_x(x);
        window.set_context_menu_y(y);
        window.set_is_context_menu_open(true);

        tracing::debug!(
            target_media_id = media_id,
            pos_x = x,
            pos_y = y,
            selected_items = selected_count,
            changed_rows = changed_ids.len(),
            elapsed_us = t_start.elapsed().as_micros(),
            "[ContextMenu: FastOpen] Context menu opened with instant selection"
        );
    }

    /// Dispatches and executes selected context menu actions.
    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        dispatcher: &Arc<CtxMenuDispatcher>,
        show_rename_fn: impl Fn(&str),
        apply_rating_fn: impl Fn(&std::collections::HashSet<MediaId>, u8),
    ) {
        window.set_is_context_menu_open(false);

        let selected_ids = state.read().file_selection.selected_ids().clone();
        let context = CtxMenuContext {
            position_x: 0.0,
            position_y: 0.0,
            selected_item_count: selected_ids.len(),
            target_ids: selected_ids.iter().copied().collect(),
        };

        tracing::info!(ctx_action_id = action_id, "Executing context menu action");

        match action_id {
            CTX_ACTION_RENAME => {
                if let Some(&target_id) = selected_ids.iter().next()
                    && selected_ids.len() == 1
                    && let Ok(Some(record)) = library.media().get(target_id)
                {
                    state.write().active_file_rename_target = Some(target_id);
                    show_rename_fn(&record.filename);
                }
            }
            CTX_ACTION_SELECT_ALL => {
                BridgeFileSelection::select_all(window, state);
            }
            CTX_ACTION_CLEAR_SELECTION => {
                BridgeFileSelection::clear_selection(window, state);
            }
            CTX_ACTION_RATE_5 => apply_rating_fn(&selected_ids, 5),
            CTX_ACTION_RATE_4 => apply_rating_fn(&selected_ids, 4),
            CTX_ACTION_RATE_3 => apply_rating_fn(&selected_ids, 3),
            CTX_ACTION_RATE_2 => apply_rating_fn(&selected_ids, 2),
            CTX_ACTION_RATE_1 => apply_rating_fn(&selected_ids, 1),
            CTX_ACTION_RATE_0 => apply_rating_fn(&selected_ids, 0),
            _ => {
                dispatcher.dispatch(action_id, &context);
            }
        }
    }

    /// Displays the modal file rename dialog.
    pub fn show_rename_dialog<F1, F2, F3>(
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
        let mut d_ref = dialog_ref.borrow_mut();
        if d_ref.is_none() {
            let win = FileRenameDialog::new().expect("Failed to instantiate FileRenameDialog");

            win.on_confirmed(move |new_name| {
                on_confirm(new_name.to_string());
            });

            win.on_cancelled(move || {
                on_cancel();
            });

            win.window().on_close_requested(move || {
                on_close();
                CloseRequestResponse::HideWindow
            });

            *d_ref = Some(win);
        }

        if let Some(ref win) = *d_ref {
            win.set_initial_name(current_name.into());
            win.set_new_name(current_name.into());
            win.set_error_message("".into());
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Closes the modal file rename dialog and releases window lock.
    pub fn close_rename_dialog(
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

    /// Commits file rename operation to physical filesystem and LMDB database.
    pub fn execute_file_rename(
        new_name: &str,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
    ) {
        let target_id = state.read().active_file_rename_target;

        if let Some(media_id) = target_id {
            match library.file_operations().rename_media(media_id, new_name) {
                Ok(updated_record) => {
                    tracing::info!(
                        media_id = media_id,
                        new_name = %new_name,
                        "File successfully renamed via FileOperations subsystem"
                    );

                    {
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
                            item.record = updated_record;
                        }
                    }

                    Self::close_rename_dialog(dialog_ref, state, window);
                    window.set_status_text(format!("Renamed file to '{}'", new_name).into());

                    BridgeInspector::update(window, state);
                    BridgeMedia::reload(window, library, thumbnails, state, false, false);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to rename media file");
                    if let Some(ref win) = *dialog_ref.borrow() {
                        win.set_error_message(e.to_string().into());
                    }
                }
            }
        }
    }
}
