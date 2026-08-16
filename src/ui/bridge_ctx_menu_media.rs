use super::bridge_file_dialog_missing::MissingFileDialogController;
use super::bridge_file_dialogs::{FileRenameContext, FileRenameDialogController};
use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_tags::BridgeTags;
use super::file_selection::FileSelectionModel;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{
    CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher,
};
use crate::library::Library;
use crate::library::models::MediaId;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Grouped dependencies bundle for media context menu actions.
pub struct MediaActionContext<'a> {
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub dispatcher: &'a Arc<CtxMenuDispatcher>,
    pub rename_dialog: &'a Rc<RefCell<Option<FileRenameDialog>>>,
    pub missing_file_dialog: &'a MissingFileDialogController,
}

pub struct MediaContextMenu;

impl MediaContextMenu {
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

    pub fn build_menu(selected_count: usize, is_busy: bool) -> CtxMenu {
        let is_single = selected_count == 1;
        let can_rename = is_single && !is_busy;
        let can_refresh = selected_count > 0 && !is_busy;

        let refresh_text = if is_single {
            "Refresh File".to_string()
        } else {
            format!("Refresh {} Files", selected_count)
        };

        CtxMenuBuilder::new()
            .header(if is_single {
                "Selected File"
            } else {
                "Selected Files"
            })
            .action(
                CTX_ACTION_RENAME,
                if is_single {
                    "Rename...".to_string()
                } else {
                    format!("Rename (Single File Only, {} selected)", selected_count)
                },
                Some("F2"),
                Some("edit"),
                can_rename,
            )
            .separator()
            .submenu(
                "Rate Selection",
                Some("star"),
                selected_count > 0 && !is_busy,
                |sub| {
                    sub.action(CTX_ACTION_RATE_5, "5 Stars (★★★★★)", None, None, true)
                        .action(CTX_ACTION_RATE_4, "4 Stars (★★★★☆)", None, None, true)
                        .action(CTX_ACTION_RATE_3, "3 Stars (★★★☆☆)", None, None, true)
                        .action(CTX_ACTION_RATE_2, "2 Stars (★★☆☆☆)", None, None, true)
                        .action(CTX_ACTION_RATE_1, "1 Star  (★☆☆☆☆)", None, None, true)
                        .separator()
                        .action(CTX_ACTION_RATE_0, "Clear Rating", None, None, true)
                },
            )
            .separator()
            .action(
                CTX_ACTION_REFRESH_FILES,
                refresh_text,
                Some("F5"),
                Some("refresh"),
                can_refresh,
            )
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

    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
    ) {
        let selected_ids = ctx.state.read().file_selection.selected_ids().clone();
        let context = CtxMenuContext {
            position_x: 0.0,
            position_y: 0.0,
            selected_item_count: selected_ids.len(),
            target_ids: selected_ids.iter().copied().collect(),
        };

        match action_id {
            CTX_ACTION_REFRESH_FILES => {
                BridgeMedia::refresh_selected_media(
                    window,
                    ctx.library,
                    ctx.thumbnails,
                    ctx.settings,
                    ctx.state,
                );
            }
            CTX_ACTION_RENAME => {
                Self::handle_rename_action(window, ctx, &selected_ids);
            }
            CTX_ACTION_SELECT_ALL => {
                BridgeFileSelection::select_all(window, ctx.library, ctx.state)
            }
            CTX_ACTION_CLEAR_SELECTION => {
                BridgeFileSelection::clear_selection(window, ctx.library, ctx.state)
            }
            CTX_ACTION_RATE_5 => BridgeTags::apply_rating_to_ids(
                &selected_ids,
                5,
                window,
                ctx.library,
                ctx.thumbnails,
                ctx.settings,
                ctx.state,
            ),
            CTX_ACTION_RATE_4 => BridgeTags::apply_rating_to_ids(
                &selected_ids,
                4,
                window,
                ctx.library,
                ctx.thumbnails,
                ctx.settings,
                ctx.state,
            ),
            CTX_ACTION_RATE_3 => BridgeTags::apply_rating_to_ids(
                &selected_ids,
                3,
                window,
                ctx.library,
                ctx.thumbnails,
                ctx.settings,
                ctx.state,
            ),
            CTX_ACTION_RATE_2 => BridgeTags::apply_rating_to_ids(
                &selected_ids,
                2,
                window,
                ctx.library,
                ctx.thumbnails,
                ctx.settings,
                ctx.state,
            ),
            CTX_ACTION_RATE_1 => BridgeTags::apply_rating_to_ids(
                &selected_ids,
                1,
                window,
                ctx.library,
                ctx.thumbnails,
                ctx.settings,
                ctx.state,
            ),
            CTX_ACTION_RATE_0 => BridgeTags::apply_rating_to_ids(
                &selected_ids,
                0,
                window,
                ctx.library,
                ctx.thumbnails,
                ctx.settings,
                ctx.state,
            ),
            _ => {
                ctx.dispatcher.dispatch(action_id, &context);
            }
        }
    }

    fn handle_rename_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) {
        if let Some(&target_id) = selected_ids.iter().next()
            && selected_ids.len() == 1
            && let Ok(Some(record)) = ctx.library.media().get(target_id)
        {
            ctx.state.write().active_file_rename_target = Some(target_id);

            let rename_ctx = FileRenameContext {
                library: ctx.library.clone(),
                thumbnails: ctx.thumbnails.clone(),
                settings: ctx.settings.clone(),
                state: ctx.state.clone(),
            };

            let ren_exec = ctx.rename_dialog.clone();
            let win_exec = window.clone_strong();
            let ren_close1 = ctx.rename_dialog.clone();
            let win_close1 = window.clone_strong();
            let st_close1 = ctx.state.clone();
            let ren_close2 = ctx.rename_dialog.clone();
            let win_close2 = window.clone_strong();
            let st_close2 = ctx.state.clone();
            let missing_dlg = ctx.missing_file_dialog.clone();

            FileRenameDialogController::show(
                ctx.rename_dialog,
                window,
                &record.filename,
                move |new_name| {
                    FileRenameDialogController::execute(
                        target_id,
                        &new_name,
                        &rename_ctx,
                        &win_exec,
                        &ren_exec,
                        &missing_dlg,
                    );
                },
                move || {
                    FileRenameDialogController::close(&ren_close1, &st_close1, &win_close1);
                },
                move || {
                    FileRenameDialogController::close(&ren_close2, &st_close2, &win_close2);
                },
            );
        }
    }
}
