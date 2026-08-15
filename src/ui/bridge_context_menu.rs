use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_tag_dialogs::TagDialogController;
use super::bridge_tags::BridgeTags;
use super::file_selection::FileSelectionModel;
use super::{ContextMenuItemModel, FileRenameDialog, MainWindow};
use crate::ctx_menu::{
    CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher,
};
use crate::library::Library;
use crate::library::models::{MediaId, MediaRecord};
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Parameter object bundling dependencies for context menu callback registration.
pub struct ContextMenuRegistration {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub state: Arc<RwLock<UiState>>,
    pub dispatcher: Arc<CtxMenuDispatcher>,
    pub rename_dialog: Rc<RefCell<Option<FileRenameDialog>>>,
    pub tag_dialogs: TagDialogController,
    pub settings: Arc<SettingsManager>,
}

// =============================================================================
// Section 1: MediaContextMenuBuilder (Selection Prep & Menu Construction)
// =============================================================================

pub struct MediaContextMenuBuilder;

impl MediaContextMenuBuilder {
    /// Ensures the clicked media item is selected (synchronizing diffs if needed) and returns selection metrics.
    pub fn prepare_selection(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
    ) -> (usize, bool) {
        let (selected_count, changed_ids) = {
            let mut st = state.write();
            let mut changed = Vec::new();
            if !st.file_selection.is_selected(media_id) {
                let prev = st.file_selection.selected_ids().clone();
                st.file_selection.select_single(media_id);
                let newly = st.file_selection.selected_ids().clone();
                changed = FileSelectionModel::compute_selection_diff(&prev, &newly);
            }
            (st.file_selection.count(), changed)
        };

        if !changed_ids.is_empty() {
            BridgeFileSelection::sync_selection_diff(window, state, &changed_ids);
        }
        BridgeInspector::update(window, state);

        let is_single = selected_count == 1;
        (selected_count, is_single)
    }

    /// Builds the declarative CtxMenu hierarchy for media file items.
    pub fn build_menu(selected_count: usize) -> CtxMenu {
        let is_single = selected_count == 1;
        let can_rename = is_single;

        CtxMenuBuilder::new()
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
            .build()
    }
}

// =============================================================================
// Section 2: SlintContextMenuPresenter (Multi-Level Cascading Presentation)
// =============================================================================

pub struct SlintContextMenuPresenter;

impl SlintContextMenuPresenter {
    /// Renders any dynamic CtxMenu structure into the multi-level Slint popup.
    pub fn open(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        mut menu: CtxMenu,
        x: f32,
        y: f32,
    ) {
        menu.index_items();
        let level0 = CtxMenuDispatcher::items_to_slint_model(&menu.items, 0);

        state.write().active_context_menu = Some(menu);

        window.set_level0_context_items(level0);
        Self::clear_submenus_from_level(window, 1);

        window.set_context_menu_x(x);
        window.set_context_menu_y(y);
        window.set_is_context_menu_open(true);
    }

    /// Handles submenu drill-down / flyout requests for any depth level.
    pub fn handle_submenu_request(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        depth: i32,
        menu_idx: usize,
    ) {
        let st = state.read();
        let Some(ref menu) = st.active_context_menu else {
            return;
        };

        if menu_idx == 0 {
            Self::clear_submenus_from_level(window, depth + 1);
            return;
        }

        if let Some(children) = menu.find_children_by_menu_index(menu_idx) {
            let next_depth = depth + 1;
            let model = CtxMenuDispatcher::items_to_slint_model(children, next_depth);
            Self::set_level_model(window, next_depth, model);
            Self::clear_submenus_from_level(window, next_depth + 1);
        }
    }

    fn set_level_model(window: &MainWindow, depth: i32, model: ModelRc<ContextMenuItemModel>) {
        match depth {
            0 => window.set_level0_context_items(model),
            1 => window.set_level1_context_items(model),
            2 => window.set_level2_context_items(model),
            3 => window.set_level3_context_items(model),
            4 => window.set_level4_context_items(model),
            5 => window.set_level5_context_items(model),
            _ => {}
        }
    }

    fn clear_submenus_from_level(window: &MainWindow, start_depth: i32) {
        let empty = || ModelRc::new(VecModel::from(vec![]));
        for d in start_depth..=5 {
            match d {
                1 => window.set_level1_context_items(empty()),
                2 => window.set_level2_context_items(empty()),
                3 => window.set_level3_context_items(empty()),
                4 => window.set_level4_context_items(empty()),
                5 => window.set_level5_context_items(empty()),
                _ => {}
            }
        }
    }
}

// =============================================================================
// Section 3: MediaMenuActionHandler (Action Routing & Execution)
// =============================================================================

pub struct MediaMenuActionHandler;

impl MediaMenuActionHandler {
    /// Dispatches and executes selected context menu actions.
    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        dispatcher: &Arc<CtxMenuDispatcher>,
        show_rename_fn: impl Fn(&str),
        apply_rating_fn: impl Fn(&HashSet<MediaId>, u8),
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
                Self::handle_rename_action(state, library, &selected_ids, show_rename_fn);
            }
            CTX_ACTION_SELECT_ALL => {
                BridgeFileSelection::select_all(window, library, state);
            }
            CTX_ACTION_CLEAR_SELECTION => {
                BridgeFileSelection::clear_selection(window, library, state);
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

    fn handle_rename_action(
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        selected_ids: &HashSet<MediaId>,
        show_rename_fn: impl Fn(&str),
    ) {
        if let Some(&target_id) = selected_ids.iter().next()
            && selected_ids.len() == 1
            && let Ok(Some(record)) = library.media().get(target_id)
        {
            state.write().active_file_rename_target = Some(target_id);
            show_rename_fn(&record.filename);
        }
    }
}

// =============================================================================
// Section 4: FileRenameController (Modal Dialog Lifecycle & Physical Renaming)
// =============================================================================

pub struct FileRenameController;

impl FileRenameController {
    /// Displays the modal file rename dialog, updating closure bindings.
    pub fn show_dialog<F1, F2, F3>(
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

    /// Closes the modal file rename dialog and releases window lock.
    pub fn close_dialog(
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

    /// Commits file rename operation to physical filesystem, LMDB storage, and in-memory cache.
    pub fn execute_rename(
        new_name: &str,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
    ) {
        let target_id = state.read().active_file_rename_target;
        let Some(media_id) = target_id else { return };

        match library.file_operations().rename_media(media_id, new_name) {
            Ok(updated_record) => {
                tracing::info!(
                    media_id = media_id,
                    new_name = %new_name,
                    "File successfully renamed via FileOperations subsystem"
                );

                Self::update_in_memory_state(state, media_id, &updated_record);
                Self::close_dialog(dialog_ref, state, window);

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

    fn update_in_memory_state(
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

// =============================================================================
// Section 5: BridgeContextMenu (Unified Facade)
// =============================================================================

pub struct BridgeContextMenu;

impl BridgeContextMenu {
    /// Registers context menu and action callbacks onto MainWindow.
    pub fn register_callbacks(window: &MainWindow, reg: ContextMenuRegistration) {
        let win = window.clone_strong();
        let st = reg.state.clone();
        window.on_media_context_menu(move |id, x, y| {
            Self::show(&win, &st, id as u64, x, y);
        });

        let win = window.clone_strong();
        let st = reg.state.clone();
        window.on_context_menu_request_submenu(move |depth, m_idx| {
            Self::request_submenu(&win, &st, depth, m_idx as usize);
        });

        let win = window.clone_strong();
        window.on_context_menu_dismissed(move || {
            win.set_is_context_menu_open(false);
        });

        let win = window.clone_strong();
        let lib = reg.library;
        let thumb = reg.thumbnails;
        let st = reg.state;
        let set = reg.settings;
        let disp = reg.dispatcher;
        let ren_dlg = reg.rename_dialog;
        let tag_dlg = reg.tag_dialogs;

        window.on_context_menu_item_clicked(move |action_id| {
            let action_u64 = action_id as u64;

            if action_u64 >= 2000 {
                let bundle = tag_dlg.create_action_bundle(&win, &lib, &thumb, &set, &st);
                BridgeTags::handle_tag_action(action_u64, &win, &lib, &set, &st, bundle);
                return;
            }

            let ren_dlg_confirm = ren_dlg.clone();
            let win_confirm = win.clone_strong();
            let lib_confirm = lib.clone();
            let thumb_confirm = thumb.clone();
            let st_confirm = st.clone();

            let show_rename = move |current_name: &str| {
                let ren_exec = ren_dlg_confirm.clone();
                let win_exec = win_confirm.clone_strong();
                let lib_exec = lib_confirm.clone();
                let thumb_exec = thumb_confirm.clone();
                let st_exec = st_confirm.clone();

                let ren_close1 = ren_dlg_confirm.clone();
                let win_close1 = win_confirm.clone_strong();
                let st_close1 = st_confirm.clone();

                let ren_close2 = ren_dlg_confirm.clone();
                let win_close2 = win_confirm.clone_strong();
                let st_close2 = st_confirm.clone();

                Self::show_rename_dialog(
                    &ren_dlg_confirm,
                    &win_confirm,
                    current_name,
                    move |new_name| {
                        Self::execute_file_rename(
                            &new_name,
                            &lib_exec,
                            &thumb_exec,
                            &st_exec,
                            &win_exec,
                            &ren_exec,
                        );
                    },
                    move || {
                        Self::close_rename_dialog(&ren_close1, &st_close1, &win_close1);
                    },
                    move || {
                        Self::close_rename_dialog(&ren_close2, &st_close2, &win_close2);
                    },
                );
            };

            let win_rating = win.clone_strong();
            let lib_rating = lib.clone();
            let thumb_rating = thumb.clone();
            let st_rating = st.clone();

            let apply_rating = move |ids: &HashSet<MediaId>, rating: u8| {
                BridgeTags::apply_rating_to_ids(
                    ids,
                    rating,
                    &win_rating,
                    &lib_rating,
                    &thumb_rating,
                    &st_rating,
                );
            };

            Self::handle_action(
                action_u64,
                &win,
                &lib,
                &st,
                &disp,
                show_rename,
                apply_rating,
            );
        });
    }

    /// Displays the multi-level context menu at the cursor position.
    pub fn show(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
        x: f32,
        y: f32,
    ) {
        let t_start = Instant::now();
        let (selected_count, _) =
            MediaContextMenuBuilder::prepare_selection(window, state, media_id);
        let menu = MediaContextMenuBuilder::build_menu(selected_count);
        Self::open_custom_menu(window, state, menu, x, y);

        tracing::debug!(
            target_media_id = media_id,
            pos_x = x,
            pos_y = y,
            selected_items = selected_count,
            elapsed_us = t_start.elapsed().as_micros(),
            "[ContextMenu: FastOpen] Context menu opened with instant selection"
        );
    }

    /// Renders any dynamic CtxMenu structure into the multi-level Slint popup.
    #[inline]
    pub fn open_custom_menu(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        menu: CtxMenu,
        x: f32,
        y: f32,
    ) {
        SlintContextMenuPresenter::open(window, state, menu, x, y);
    }

    /// Handles submenu drill-down / flyout requests for any depth level.
    #[inline]
    pub fn request_submenu(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        depth: i32,
        menu_idx: usize,
    ) {
        SlintContextMenuPresenter::handle_submenu_request(window, state, depth, menu_idx);
    }

    /// Dispatches and executes selected context menu actions.
    #[inline]
    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        dispatcher: &Arc<CtxMenuDispatcher>,
        show_rename_fn: impl Fn(&str),
        apply_rating_fn: impl Fn(&HashSet<MediaId>, u8),
    ) {
        MediaMenuActionHandler::handle_action(
            action_id,
            window,
            library,
            state,
            dispatcher,
            show_rename_fn,
            apply_rating_fn,
        );
    }

    /// Displays the modal file rename dialog, always updating closure bindings.
    #[inline]
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
        FileRenameController::show_dialog(
            dialog_ref,
            window,
            current_name,
            on_confirm,
            on_cancel,
            on_close,
        );
    }

    /// Closes the modal file rename dialog and releases window lock.
    #[inline]
    pub fn close_rename_dialog(
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
    ) {
        FileRenameController::close_dialog(dialog_ref, state, window);
    }

    /// Commits file rename operation to physical filesystem and LMDB database.
    #[inline]
    pub fn execute_file_rename(
        new_name: &str,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
        window: &MainWindow,
        dialog_ref: &Rc<RefCell<Option<FileRenameDialog>>>,
    ) {
        FileRenameController::execute_rename(
            new_name, library, thumbnails, state, window, dialog_ref,
        );
    }
}
