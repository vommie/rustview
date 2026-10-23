//! Actions of the media context menu that open, reveal or rename files.

use crate::ui::file_dialogs::{FileDialogContext, FileRenameDialogController, RenameDialogParams};
use crate::ui::selection::BridgeFileSelection;
use crate::ui::open_confirmation::{OpenConfirmation, OpenKind};
use crate::ui::media::open_with::{MediaOpenWith, OpenWithChoice};
use crate::ui::media::BridgeMedia;
use crate::ui::state::*;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::{BridgeTags, TagActionContext};
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::thumbnail_regeneration::ThumbnailRegeneration;
use crate::ui::MainWindow;
use crate::ctx_menu::{
    CtxMenuActionId, CtxMenuContext,
};
use rustview_core::library::models::{FileTransferKind, MediaId};
use rustview_core::settings::LogObscurer;
use crate::system::SystemFileHandler;
use slint::ComponentHandle;
use std::path::PathBuf;
use super::{MediaActionContext, MediaContextMenu};

impl MediaContextMenu {
    pub fn handle_file_action_for_ids(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        let context = CtxMenuContext {
            position_x: 0.0,
            position_y: 0.0,
            selected_item_count: target_ids.len(),
            target_ids: target_ids.to_vec(),
        };

        // 1. Move to... Folder destination clicked
        if action_id >= CTX_ACTION_MOVE_TO_BASE {
            let target_folder_id = action_id - CTX_ACTION_MOVE_TO_BASE;
            tracing::info!(
                target_folder_id,
                items_count = target_ids.len(),
                "[MediaContextMenu: MoveTo] Context menu Move To triggered"
            );
            crate::ui::dnd::BridgeDnd::start_batch_transfer(crate::ui::dnd::BatchTransferParams {
                window,
                library: ctx.library,
                thumbnails: ctx.thumbnails,
                settings: ctx.settings,
                state: ctx.state,
                conflict_dialog: ctx.conflict_dialog,
                target_folder_id,
                transfer_kind: FileTransferKind::Move,
            });
            return;
        }

        // 2. Copy to... Folder destination clicked
        if action_id >= CTX_ACTION_COPY_TO_BASE {
            let target_folder_id = action_id - CTX_ACTION_COPY_TO_BASE;
            tracing::info!(
                target_folder_id,
                items_count = target_ids.len(),
                "[MediaContextMenu: CopyTo] Context menu Copy To triggered"
            );
            crate::ui::dnd::BridgeDnd::start_batch_transfer(crate::ui::dnd::BatchTransferParams {
                window,
                library: ctx.library,
                thumbnails: ctx.thumbnails,
                settings: ctx.settings,
                state: ctx.state,
                conflict_dialog: ctx.conflict_dialog,
                target_folder_id,
                transfer_kind: FileTransferKind::Copy,
            });
            return;
        }

        // 3. Open With Desktop App clicked
        if let Some(OpenWithChoice::App(app_idx)) = MediaOpenWith::choice_of(action_id) {
            Self::handle_open_with_specific_app(window, ctx, target_ids, app_idx);
            return;
        }

        let target_id_set: std::collections::HashSet<MediaId> = target_ids.iter().copied().collect();
        let tag_action_ctx = TagActionContext {
            window,
            library: ctx.library,
            thumbnails: ctx.thumbnails,
            tab_manager: ctx.tab_manager,
            settings: ctx.settings,
            state: ctx.state,
        };

        match action_id {
            CTX_ACTION_OPEN => {
                Self::handle_open_action(window, ctx, target_ids);
            }
            CTX_ACTION_OPEN_IN_TAB => Self::handle_open_in_tabs_action(window, ctx, target_ids, false),
            CTX_ACTION_OPEN_IN_BG_TAB => Self::handle_open_in_tabs_action(window, ctx, target_ids, true),
            CTX_ACTION_OPEN_IN_FULLSCREEN => {
                Self::handle_open_in_fullscreen_action(window, ctx, target_ids);
            }
            CTX_ACTION_OPEN_WITH_OTHER => {
                Self::handle_open_with_other_action(window, ctx, target_ids);
            }
            CTX_ACTION_OPEN_IN_FILE_MANAGER => {
                Self::handle_open_in_file_manager_action(window, ctx, target_ids);
            }
            CTX_ACTION_COPY_NAME => {
                Self::handle_copy_name_action(window, ctx, target_ids);
            }
            CTX_ACTION_COPY_PATH => {
                Self::handle_copy_path_action(window, ctx, target_ids);
            }
            CTX_ACTION_COPY_IMAGE => {
                Self::handle_copy_image_action(window, ctx, target_ids);
            }
            CTX_ACTION_DELETE_MEDIA => {
                if BridgeMedia::is_system_busy(window, ctx.thumbnails, ctx.state) {
                    StatusBarBridge::warn(
                        window,
                        "Cannot delete files: an active operation is in progress.",
                    );
                    return;
                }
                let dialog_ctx = FileDialogContext {
                    library: ctx.library.clone(),
                    thumbnails: ctx.thumbnails.clone(),
                    tab_manager: ctx.tab_manager.clone(),
                    settings: ctx.settings.clone(),
                    state: ctx.state.clone(),
                };
                ctx.delete_file_dialog.show(window, target_ids.to_vec(), dialog_ctx);
            }
            CTX_ACTION_REMOVE_FROM_DB => {
                if BridgeMedia::is_system_busy(window, ctx.thumbnails, ctx.state) {
                    StatusBarBridge::warn(
                        window,
                        "Cannot modify database: an active operation is in progress.",
                    );
                    return;
                }
                let dialog_ctx = FileDialogContext {
                    library: ctx.library.clone(),
                    thumbnails: ctx.thumbnails.clone(),
                    tab_manager: ctx.tab_manager.clone(),
                    settings: ctx.settings.clone(),
                    state: ctx.state.clone(),
                };
                ctx.remove_orphaned_dialog.show(window, target_ids.to_vec(), dialog_ctx);
            }
            CTX_ACTION_REFRESH_METADATA => {
                if BridgeMedia::is_system_busy(window, ctx.thumbnails, ctx.state) {
                    StatusBarBridge::warn(
                        window,
                        "Cannot start refresh: another operation is already in progress.",
                    );
                    return;
                }
                BridgeMedia::refresh_selected_metadata_only(
                    window,
                    ctx.library,
                    ctx.thumbnails,
                    ctx.state,
                );
            }
            CTX_ACTION_REGENERATE_THUMB_RANDOM => {
                ThumbnailRegeneration::regenerate_selected(
                    window,
                    ctx.thumbnails,
                    ctx.settings,
                    ctx.state,
                );
            }
            CTX_ACTION_REFRESH_FILES => {
                if BridgeMedia::is_system_busy(window, ctx.thumbnails, ctx.state) {
                    StatusBarBridge::warn(
                        window,
                        "Cannot start refresh: another operation is already in progress.",
                    );
                    return;
                }
                BridgeMedia::refresh_selected_media(
                    window,
                    ctx.library,
                    ctx.thumbnails,
                    ctx.tab_manager,
                    ctx.settings,
                    ctx.state,
                );
            }
            CTX_ACTION_RENAME => {
                if BridgeMedia::is_system_busy(window, ctx.thumbnails, ctx.state) {
                    StatusBarBridge::warn(
                        window,
                        "Cannot rename: an active operation is in progress.",
                    );
                    return;
                }
                Self::handle_rename_action(window, ctx, target_ids);
            }
            CTX_ACTION_SELECT_ALL => {
                BridgeFileSelection::select_all(window, ctx.library, ctx.state)
            }
            CTX_ACTION_CLEAR_SELECTION => {
                BridgeFileSelection::clear_selection(window, ctx.library, ctx.state)
            }
            CTX_ACTION_RATE_5 => BridgeTags::apply_rating_to_ids(
                &target_id_set,
                5,
                &tag_action_ctx,
            ),
            CTX_ACTION_RATE_4 => BridgeTags::apply_rating_to_ids(
                &target_id_set,
                4,
                &tag_action_ctx,
            ),
            CTX_ACTION_RATE_3 => BridgeTags::apply_rating_to_ids(
                &target_id_set,
                3,
                &tag_action_ctx,
            ),
            CTX_ACTION_RATE_2 => BridgeTags::apply_rating_to_ids(
                &target_id_set,
                2,
                &tag_action_ctx,
            ),
            CTX_ACTION_RATE_1 => BridgeTags::apply_rating_to_ids(
                &target_id_set,
                1,
                &tag_action_ctx,
            ),
            CTX_ACTION_RATE_0 => BridgeTags::apply_rating_to_ids(
                &target_id_set,
                0,
                &tag_action_ctx,
            ),
            _ => {
                ctx.dispatcher.dispatch(action_id, &context);
            }
        }
    }

    pub(super) fn handle_open_in_fullscreen_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        tracing::info!(
            target_count = target_ids.len(),
            "[MediaContextMenu] 'Open in Fullscreen' action triggered"
        );
        ctx.fullscreen.dismiss();
        ctx.fullscreen
            .open(ctx.settings, ctx.state, window.as_weak());
    }

    pub(super) fn handle_open_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        if target_ids.is_empty() {
            return;
        }

        tracing::info!(
            target_count = target_ids.len(),
            "[MediaContextMenu] 'Open' action triggered from context menu"
        );

        let (ids, state, library, win) = (target_ids.to_vec(), ctx.state.clone(), ctx.library.clone(), window.clone_strong());
        OpenConfirmation::run_or_ask(window, ctx.settings, OpenKind::Default, ids.len(), move || {
            for id in ids {
                BridgeFileSelection::open_media(id, &state, &library, &win);
            }
        });
    }

    pub(super) fn handle_open_in_tabs_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
        background: bool,
    ) {
        if target_ids.is_empty() {
            return;
        }
        let ids = target_ids.to_vec();
        let (win, tab_manager, library, settings, state, fullscreen) = (
            window.clone_strong(),
            ctx.tab_manager.clone(),
            ctx.library.clone(),
            ctx.settings.clone(),
            ctx.state.clone(),
            ctx.fullscreen.clone(),
        );
        let kind = if background { OpenKind::BackgroundTabs } else { OpenKind::Tabs };
        OpenConfirmation::run_or_ask(window, ctx.settings, kind, ids.len(), move || {
            if background {
                BridgeViewerTabs::open_multiple_media_tabs_background(&ids, &win, &tab_manager, &library, &settings, &state);
            } else {
                BridgeViewerTabs::open_multiple_media_tabs(&ids, &win, &tab_manager, &library, &settings, &state, &fullscreen);
            }
        });
    }

    pub(super) fn handle_open_with_specific_app(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
        app_idx: usize,
    ) {
        let (ids, state, library, win) = (target_ids.to_vec(), ctx.state.clone(), ctx.library.clone(), window.clone_strong());
        OpenConfirmation::run_or_ask(window, ctx.settings, OpenKind::OpenWith, ids.len(), move || {
            let paths = MediaOpenWith::paths_for(&state, &library, &ids);
            MediaOpenWith::launch(&win, &state, &paths, app_idx);
        });
    }

    pub(super) fn handle_open_with_other_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        let (ids, state, library, win) = (target_ids.to_vec(), ctx.state.clone(), ctx.library.clone(), window.clone_strong());
        OpenConfirmation::run_or_ask(window, ctx.settings, OpenKind::OpenWith, ids.len(), move || {
            let paths = MediaOpenWith::paths_for(&state, &library, &ids);
            MediaOpenWith::choose_application(&win, &paths);
        });
    }

    pub fn collect_paths_for_ids(
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) -> Vec<PathBuf> {
        MediaOpenWith::paths_for(ctx.state, ctx.library, target_ids)
    }

    pub(super) fn handle_open_in_file_manager_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        let (ids, state, library, win) = (target_ids.to_vec(), ctx.state.clone(), ctx.library.clone(), window.clone_strong());
        OpenConfirmation::run_or_ask(window, ctx.settings, OpenKind::FileManager, ids.len(), move || {
            let paths = MediaOpenWith::paths_for(&state, &library, &ids);
            Self::reveal_paths_in_file_manager(&win, &paths);
        });
    }

    pub(super) fn reveal_paths_in_file_manager(window: &MainWindow, paths: &[PathBuf]) {
        if paths.is_empty() {
            tracing::warn!("[MediaContextMenu] No file paths found to reveal in File Manager");
            return;
        }

        let count = paths.len();
        tracing::info!(
            count,
            first_path = %LogObscurer::path(&paths[0]),
            "[MediaContextMenu] Revealing file(s) in File Manager"
        );

        match SystemFileHandler::reveal_in_file_manager(paths) {
            Ok(()) => {
                if count == 1 {
                    let name = paths[0]
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    StatusBarBridge::success(
                        window,
                        format!("Opened file '{}' in File Manager.", name),
                    );
                } else {
                    StatusBarBridge::success(
                        window,
                        format!("Opened {} files in File Manager.", count),
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "[MediaContextMenu] Failed to open file(s) in File Manager"
                );
                StatusBarBridge::error(window, format!("Failed to open File Manager: {}", e));
            }
        }
    }

    pub(super) fn handle_rename_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        if let Some(&target_id) = target_ids.first()
            && target_ids.len() == 1
        {
            tracing::info!(target_id, "[MediaContextMenu] Opening rename dialog for single media item");

            let dialog_ctx = FileDialogContext {
                library: ctx.library.clone(),
                thumbnails: ctx.thumbnails.clone(),
                tab_manager: ctx.tab_manager.clone(),
                settings: ctx.settings.clone(),
                state: ctx.state.clone(),
            };

            FileRenameDialogController::open(RenameDialogParams {
                dialog_ref: ctx.rename_dialog,
                window,
                media_id: target_id,
                ctx: &dialog_ctx,
                conflict_dialog: ctx.conflict_dialog,
                missing_dialog: ctx.missing_file_dialog,
                fullscreen: ctx.fullscreen,
            });
        }
    }
}
