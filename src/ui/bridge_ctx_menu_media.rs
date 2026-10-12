use super::bridge_file_dialog_conflict::FileOpsConflictDialogController;
use super::bridge_file_dialog_delete::DeleteFileDialogController;
use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
use super::bridge_file_dialogs::{FileDialogContext, FileRenameDialogController, RenameDialogParams};
use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_image_fullscreen::ImageFullscreenDialogController;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::*;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::{BridgeTags, TagActionContext};
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::file_selection::FileSelectionModel;
use super::thumbnail_regeneration::ThumbnailRegeneration;
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{
    CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher,
};
use crate::icons::Icon;
use crate::library::models::{FileTransferKind, FolderHierarchySnapshot, FolderId, MediaId};
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::system::{DesktopApp, DesktopIntegration, SystemClipboard, SystemFileHandler};
use crate::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub struct MediaActionContext<'a> {
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub dispatcher: &'a Arc<CtxMenuDispatcher>,
    pub rename_dialog: &'a Rc<RefCell<Option<FileRenameDialog>>>,
    pub missing_file_dialog: &'a MissingFileDialogController,
    pub delete_file_dialog: &'a DeleteFileDialogController,
    pub remove_orphaned_dialog: &'a RemoveOrphanedFileDialogController,
    pub conflict_dialog: &'a FileOpsConflictDialogController,
    pub fullscreen_controller: &'a ImageFullscreenDialogController,
}

#[derive(Debug, Clone)]
pub struct MediaSelectionMeta {
    pub all_selected_deleted: bool,
    pub selected_count: usize,
    pub is_single_image: bool,
    pub has_video: bool,
    pub target_file_exists: bool,
    pub is_single_pending: bool,
    pub single_filename: String,
    pub current_folder_id: Option<FolderId>,
    pub associated_apps: Vec<DesktopApp>,
}

pub struct MediaContextMenu;

impl MediaContextMenu {
    pub fn prepare_selection(
        window: &MainWindow,
        library: &Library,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
    ) -> MediaSelectionMeta {
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
        StatusBarBridge::update_metrics(window, state);

        let is_single = selected_count == 1;
        let single_id = state
            .read()
            .file_selection
            .selected_ids()
            .iter()
            .next()
            .copied()
            .unwrap_or(media_id);

        let (has_video, is_single_image, all_selected_deleted, target_path_opt, is_single_pending, single_filename, cur_folder_id) = {
            let st = state.read();
            let selected_ids = st.file_selection.selected_ids();

            let all_deleted = !selected_ids.is_empty()
                && selected_ids.iter().all(|&id| {
                    st.all_folder_media
                        .iter()
                        .find(|m| m.record.id == id)
                        .map(|m| m.record.is_deleted)
                        .unwrap_or(false)
                });

            let mut contains_video = false;
            for &id in selected_ids {
                if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id)
                    && matches!(item.record.media_type, MediaType::Video)
                {
                    contains_video = true;
                    break;
                } else if let Ok(Some(rec)) = library.media().get(id)
                    && matches!(rec.media_type, MediaType::Video)
                {
                    contains_video = true;
                    break;
                }
            }

            let (is_single_img, is_pending, fname, f_id) = if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == single_id) {
                (
                    matches!(item.record.media_type, MediaType::Image),
                    item.is_pending,
                    item.record.filename.clone(),
                    Some(item.record.folder_id),
                )
            } else if let Ok(Some(m)) = library.media().get(single_id) {
                let pending = m.source.is_some() && (m.file_size == 0 || m.width == 0);
                (
                    matches!(m.media_type, MediaType::Image),
                    pending,
                    m.filename.clone(),
                    Some(m.folder_id),
                )
            } else {
                (false, false, String::new(), st.current_folder_id)
            };

            let path = if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == single_id) {
                Some(item.full_path.clone())
            } else {
                library.resolve_media_path(single_id).ok()
            };

            (contains_video, is_single_img, all_deleted, path, is_single && is_pending, fname, f_id)
        };

        let target_file_exists = if is_single {
            target_path_opt
                .as_ref()
                .map(|p| p.is_file() && p.metadata().map(|m| m.len() > 0).unwrap_or(false))
                .unwrap_or(false)
        } else {
            !all_selected_deleted
        };

        let associated_apps = if target_file_exists && let Some(ref path) = target_path_opt {
            DesktopIntegration::get_associated_apps(path)
        } else {
            Vec::new()
        };

        state.write().active_associated_apps = associated_apps.clone();

        tracing::debug!(
            selected_count,
            single_media_id = single_id,
            target_file_exists,
            apps_count = associated_apps.len(),
            "[MediaContextMenu: Prepare] Media context menu selection metadata calculated"
        );

        MediaSelectionMeta {
            all_selected_deleted,
            selected_count,
            is_single_image: is_single && is_single_image,
            has_video,
            target_file_exists,
            is_single_pending,
            single_filename,
            current_folder_id: cur_folder_id,
            associated_apps,
        }
    }

    pub fn append_file_open_entries(
        mut builder: CtxMenuBuilder,
        is_single: bool,
        can_open: bool,
        associated_apps: &[DesktopApp],
    ) -> CtxMenuBuilder {
        builder = builder.action(
            CTX_ACTION_OPEN,
            if is_single {
                "Open".to_string()
            } else {
                "Open Selected Files".to_string()
            },
            None,
            Some(Icon::Open),
            can_open,
        );

        if can_open && !associated_apps.is_empty() {
            let apps = associated_apps.to_vec();
            builder = builder.submenu("Open With...", Some(Icon::OpenWith), true, move |mut sub| {
                for (idx, app) in apps.iter().enumerate() {
                    let action_id = CTX_ACTION_OPEN_WITH_APP_BASE + (idx as CtxMenuActionId);
                    sub = sub.action_with_icon_path(
                        action_id,
                        app.name.clone(),
                        None,
                        app.icon_path.clone(),
                        true,
                    );
                }

                if !apps.is_empty() {
                    sub = sub.separator();
                }

                sub.action(
                    CTX_ACTION_OPEN_WITH_OTHER,
                    "Other Application...",
                    None,
                    Some(Icon::OpenWith),
                    true,
                )
            });
        } else {
            builder = builder.action(
                CTX_ACTION_OPEN_WITH_OTHER,
                "Open With...",
                None,
                Some(Icon::OpenWith),
                can_open,
            );
        }

        builder = builder.action(
            CTX_ACTION_OPEN_IN_FILE_MANAGER,
            if is_single {
                "Open in File Manager".to_string()
            } else {
                "Open in File Manager (Multiple Files)".to_string()
            },
            None,
            Some(Icon::FolderOpen),
            can_open,
        );

        builder
    }

    pub fn append_file_management_entries(
        mut builder: CtxMenuBuilder,
        is_single: bool,
        is_single_image: bool,
        all_selected_deleted: bool,
        target_file_exists: bool,
        is_busy: bool,
    ) -> CtxMenuBuilder {
        let can_rename = is_single && !is_busy;

        let copy_name_text = if is_single {
            "File Name".to_string()
        } else {
            "File Names".to_string()
        };

        let copy_path_text = if is_single {
            "File Path".to_string()
        } else {
            "File Paths".to_string()
        };

        let delete_action_text = if all_selected_deleted {
            if is_single {
                "Remove from Database...".to_string()
            } else {
                "Remove Files from Database...".to_string()
            }
        } else if is_single {
            "Delete File...".to_string()
        } else {
            "Delete Files...".to_string()
        };

        let (delete_action_id, delete_icon) = if all_selected_deleted {
            (CTX_ACTION_REMOVE_FROM_DB, Some(Icon::DatabaseClean))
        } else {
            (CTX_ACTION_DELETE_MEDIA, Some(Icon::Delete))
        };

        builder = builder
            .action(
                CTX_ACTION_RENAME,
                if is_single {
                    "Rename...".to_string()
                } else {
                    "Rename (Single File Only)".to_string()
                },
                Some("F2"),
                Some(Icon::Rename),
                can_rename,
            )
            .submenu("Copy...", Some(Icon::Copy), true, move |sub| {
                let mut sub_builder = sub
                    .action(CTX_ACTION_COPY_NAME, copy_name_text, None, Some(Icon::Copy), true)
                    .action(CTX_ACTION_COPY_PATH, copy_path_text, None, Some(Icon::Copy), true);

                if is_single_image {
                    sub_builder = sub_builder.action(
                        CTX_ACTION_COPY_IMAGE,
                        "Image Content",
                        None,
                        Some(Icon::Copy),
                        target_file_exists,
                    );
                }
                sub_builder
            })
            .action(
                delete_action_id,
                delete_action_text,
                Some("Del"),
                delete_icon,
                !is_busy,
            );

        builder
    }

    /// Builds the complete media context menu using SPoT in-memory hierarchy projections for "Copy to..." and "Move to..." submenus.
    pub fn build_menu(
        meta: &MediaSelectionMeta,
        is_busy: bool,
        snapshot: Option<&FolderHierarchySnapshot>,
        show_hidden: bool,
    ) -> CtxMenu {
        let is_single = meta.selected_count == 1;
        let can_open = meta.selected_count > 0 && meta.target_file_exists;

        let open_in_tab_text = if is_single {
            "Open in Tab".to_string()
        } else {
            format!("Open in Tabs ({} Files)", meta.selected_count)
        };
        let open_in_tab_shortcut = if is_single { Some("DoubleClick") } else { None };

        let open_in_bg_tab_text = if is_single {
            "Open in Background-Tab".to_string()
        } else {
            format!("Open in Background-Tabs ({} Files)", meta.selected_count)
        };
        let open_in_bg_tab_shortcut = if is_single { Some("Ctrl+MiddleClick") } else { None };

        let open_in_fullscreen_text = "Open in Fullscreen".to_string();
        let open_in_fullscreen_shortcut = if is_single { Some("MiddleClick") } else { None };

        let refresh_full_text = if is_single {
            "File (Full)".to_string()
        } else {
            format!("Files (Full) ({} Files)", meta.selected_count)
        };

        let refresh_meta_text = if is_single {
            "File (Metadata)".to_string()
        } else {
            format!("Files (Metadata) ({} Files)", meta.selected_count)
        };

        let regen_thumb_text = ThumbnailRegeneration::menu_label(meta.selected_count, meta.has_video);

        let can_refresh = meta.selected_count > 0 && !is_busy;

        let mut builder = CtxMenuBuilder::new();
        if is_single {
            let prefix = if meta.is_single_pending {
                "Pending File: "
            } else if meta.all_selected_deleted {
                "Missing File: "
            } else {
                "File: "
            };
            let (header_title, header_tooltip) =
                CtxMenuBuilder::format_header_title(prefix, &meta.single_filename, 24);
            builder = builder.header_with_tooltip(header_title, header_tooltip);
        } else {
            builder = builder.header(format!("Selected Files ({})", meta.selected_count));
        }

        // 1. File Open Actions
        builder = Self::append_file_open_entries(builder, is_single, can_open, &meta.associated_apps);

        // 2. Tab Navigation Actions
        builder = builder
            .action(
                CTX_ACTION_OPEN_IN_TAB,
                open_in_tab_text,
                open_in_tab_shortcut,
                Some(Icon::OpenInTab),
                can_open,
            )
            .action(
                CTX_ACTION_OPEN_IN_BG_TAB,
                open_in_bg_tab_text,
                open_in_bg_tab_shortcut,
                Some(Icon::OpenInBackgroundTab),
                can_open,
            )
            .action(
                CTX_ACTION_OPEN_IN_FULLSCREEN,
                open_in_fullscreen_text,
                open_in_fullscreen_shortcut,
                Some(Icon::OpenFullscreen),
                can_open,
            )
            .separator();

        // 3. Move To... / Copy To... Submenus Projected directly from SPoT with zero disk I/O
        if let Some(snap) = snapshot {
            let copy_menu_items = snap.project_target_folder_menu(
                CTX_ACTION_COPY_TO_BASE,
                None,
                meta.current_folder_id,
                show_hidden,
            );
            if !copy_menu_items.is_empty() {
                builder = builder.submenu("Copy to...", Some(Icon::Copy), true, move |sub| {
                    let mut s = sub;
                    for item in copy_menu_items {
                        s = s.item(item);
                    }
                    s
                });
            }

            let move_menu_items = snap.project_target_folder_menu(
                CTX_ACTION_MOVE_TO_BASE,
                None,
                meta.current_folder_id,
                show_hidden,
            );
            if !move_menu_items.is_empty() {
                builder = builder.submenu("Move to...", Some(Icon::MoveTo), !is_busy, move |sub| {
                    let mut s = sub;
                    for item in move_menu_items {
                        s = s.item(item);
                    }
                    s
                });
            }
            builder = builder.separator();
        }

        // 4. Shared File Management Actions
        builder = Self::append_file_management_entries(
            builder,
            is_single,
            meta.is_single_image,
            meta.all_selected_deleted,
            meta.target_file_exists,
            is_busy,
        );

        // 5. Ratings, Refresh & Selection Utilities
        builder = builder
            .separator()
            .submenu(
                "Rate Selection",
                Some(Icon::Rate),
                meta.selected_count > 0,
                |sub| {
                    sub.action(CTX_ACTION_RATE_5, "5 Stars", None, Some(Icon::Rating5), true)
                        .action(CTX_ACTION_RATE_4, "4 Stars", None, Some(Icon::Rating4), true)
                        .action(CTX_ACTION_RATE_3, "3 Stars", None, Some(Icon::Rating3), true)
                        .action(CTX_ACTION_RATE_2, "2 Stars", None, Some(Icon::Rating2), true)
                        .action(CTX_ACTION_RATE_1, "1 Star", None, Some(Icon::Rating1), true)
                        .separator()
                        .action(CTX_ACTION_RATE_0, "Clear Rating", None, Some(Icon::RatingClear), true)
                },
            )
            .separator()
            .submenu(
                "Refresh",
                Some(Icon::Refresh),
                can_refresh,
                |sub| {
                    sub.action(
                        CTX_ACTION_REFRESH_FILES,
                        refresh_full_text,
                        Some("F5"),
                        Some(Icon::Refresh),
                        can_refresh,
                    )
                    .action(
                        CTX_ACTION_REFRESH_METADATA,
                        refresh_meta_text,
                        None,
                        Some(Icon::Refresh),
                        can_refresh,
                    )
                    .action(
                        CTX_ACTION_REGENERATE_THUMB_RANDOM,
                        regen_thumb_text,
                        None,
                        Some(Icon::RegenerateThumbnail),
                        can_refresh,
                    )
                },
            )
            .separator()
            .action(
                CTX_ACTION_SELECT_ALL,
                "Select All",
                Some("Ctrl+A"),
                Some(Icon::SelectAll),
                true,
            )
            .action(
                CTX_ACTION_CLEAR_SELECTION,
                "Clear Selection",
                Some("Ctrl+Shift+A"),
                Some(Icon::ClearSelection),
                meta.selected_count > 0,
            );

        builder.build()
    }

    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
    ) {
        let selected_ids: Vec<MediaId> = ctx.state.read().get_selected_ids_ordered();
        let fallback_ids = if selected_ids.is_empty() {
            ctx.state.read().file_selection.selected_ids().iter().copied().collect()
        } else {
            selected_ids
        };

        Self::handle_file_action_for_ids(action_id, window, ctx, &fallback_ids);
    }

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
            super::bridge_dnd::BridgeDnd::start_batch_transfer(super::bridge_dnd::BatchTransferParams {
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
            super::bridge_dnd::BridgeDnd::start_batch_transfer(super::bridge_dnd::BatchTransferParams {
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
        if (CTX_ACTION_OPEN_WITH_APP_BASE..CTX_ACTION_OPEN_WITH_APP_BASE + 500).contains(&action_id) {
            let app_idx = (action_id - CTX_ACTION_OPEN_WITH_APP_BASE) as usize;
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
            CTX_ACTION_OPEN_IN_TAB => {
                if !target_ids.is_empty() {
                    BridgeViewerTabs::open_multiple_media_tabs(
                        target_ids,
                        window,
                        ctx.tab_manager,
                        ctx.library,
                        ctx.settings,
                        ctx.state,
                        ctx.fullscreen_controller,
                    );
                }
            }
            CTX_ACTION_OPEN_IN_BG_TAB => {
                if !target_ids.is_empty() {
                    BridgeViewerTabs::open_multiple_media_tabs_background(
                        target_ids,
                        window,
                        ctx.tab_manager,
                        ctx.library,
                        ctx.settings,
                        ctx.state,
                    );
                }
            }
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
                BridgeMedia::regenerate_selected_thumbnails_random(
                    window,
                    ctx.library,
                    ctx.thumbnails,
                    ctx.tab_manager,
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

    fn handle_open_in_fullscreen_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        if let Some(&media_id) = target_ids.first() {
            tracing::info!(
                media_id,
                "[MediaContextMenu] 'Open in Fullscreen' action triggered"
            );
            ctx.fullscreen_controller.show_for_media(
                media_id,
                ctx.library,
                ctx.settings,
                ctx.state,
                ctx.tab_manager,
                window.as_weak(),
            );
        }
    }

    fn handle_open_action(
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

        for &id in target_ids {
            BridgeFileSelection::open_media(id, ctx.state, ctx.library, window);
        }
    }

    fn handle_open_with_specific_app(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
        app_idx: usize,
    ) {
        let app_opt = ctx.state.read().active_associated_apps.get(app_idx).cloned();
        let Some(app) = app_opt else {
            tracing::warn!(app_idx, "[MediaContextMenu] Target desktop app index not found");
            return;
        };

        let paths = Self::collect_paths_for_ids(ctx, target_ids);
        if paths.is_empty() {
            tracing::warn!(app = %app.name, "[MediaContextMenu] No file paths found to open with app");
            return;
        }

        tracing::info!(
            app = %app.name,
            paths_count = paths.len(),
            first_path = %LogObscurer::path(&paths[0]),
            "[MediaContextMenu] Launching selected file(s) with specific desktop app"
        );

        if let Err(e) = DesktopIntegration::launch_app(&app, &paths) {
            tracing::error!(error = %e, app = %app.name, "[MediaContextMenu] Failed to launch desktop app");
            StatusBarBridge::error(window, format!("Failed to launch '{}': {}", app.name, e));
        } else {
            StatusBarBridge::success(
                window,
                format!("Launched '{}' with {} file(s).", app.name, paths.len()),
            );
        }
    }

    fn handle_open_with_other_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        let paths = Self::collect_paths_for_ids(ctx, target_ids);
        let Some(first_path) = paths.first() else {
            tracing::warn!("[MediaContextMenu] No valid file path found for application chooser");
            return;
        };

        tracing::info!(
            target_file = %LogObscurer::path(first_path),
            "[MediaContextMenu] 'Other Application...' triggered"
        );

        if let Err(e) = DesktopIntegration::open_with_dialog(first_path) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to open native app chooser");
            StatusBarBridge::error(window, format!("Failed to open application chooser: {}", e));
        } else {
            StatusBarBridge::info(window, "Opened native application chooser dialog.");
        }
    }

    pub fn collect_paths_for_ids(
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(target_ids.len());
        let st = ctx.state.read();

        for &id in target_ids {
            if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id) {
                paths.push(item.full_path.clone());
            } else if let Ok(p) = ctx.library.resolve_media_path(id) {
                paths.push(p);
            }
        }
        paths
    }

    fn handle_open_in_file_manager_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        let paths = Self::collect_paths_for_ids(ctx, target_ids);
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

        match SystemFileHandler::reveal_in_file_manager(&paths) {
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

    fn handle_copy_name_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        if target_ids.is_empty() {
            return;
        }

        let mut names = Vec::with_capacity(target_ids.len());
        let st = ctx.state.read();

        for &id in target_ids {
            if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == id) {
                names.push(item.record.filename.clone());
            } else if let Ok(Some(rec)) = ctx.library.media().get(id) {
                names.push(rec.filename);
            } else if let Some(tab) = ctx.tab_manager.get_tab_by_media_id(id) {
                names.push(tab.filename);
            }
        }
        drop(st);

        if names.is_empty() {
            tracing::warn!("[MediaContextMenu] No valid names resolved for copy name action");
            return;
        }

        let count = names.len();
        tracing::info!(
            count,
            first_name = %LogObscurer::filename(&names[0]),
            "[MediaContextMenu] Copying file name(s) to system clipboard"
        );

        if let Err(e) = SystemClipboard::copy_file_names(&names) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to copy file names to clipboard");
            StatusBarBridge::error(window, format!("Failed to copy to clipboard: {}", e));
        } else if count == 1 {
            StatusBarBridge::success(window, format!("Copied file name to clipboard: '{}'", names[0]));
        } else {
            StatusBarBridge::success(window, format!("Copied {} file names to clipboard.", count));
        }
    }

    fn handle_copy_path_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        let paths = Self::collect_paths_for_ids(ctx, target_ids);
        if paths.is_empty() {
            tracing::warn!("[MediaContextMenu] No valid paths resolved for copy path action");
            return;
        }

        let count = paths.len();
        tracing::info!(
            count,
            first_path = %LogObscurer::path(&paths[0]),
            "[MediaContextMenu] Copying file path(s) to system clipboard"
        );

        if let Err(e) = SystemClipboard::copy_file_paths(&paths) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to copy file paths to clipboard");
            StatusBarBridge::error(window, format!("Failed to copy to clipboard: {}", e));
        } else if count == 1 {
            let path_str = paths[0].display().to_string();
            StatusBarBridge::success(window, format!("Copied file path to clipboard: '{}'", path_str));
        } else {
            StatusBarBridge::success(window, format!("Copied {} file paths to clipboard.", count));
        }
    }

    fn handle_copy_image_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        target_ids: &[MediaId],
    ) {
        let Some(&single_id) = target_ids.first() else {
            return;
        };

        if target_ids.len() != 1 {
            return;
        }

        let (filename, full_path) = {
            let st = ctx.state.read();
            if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == single_id) {
                (item.record.filename.clone(), item.full_path.clone())
            } else if let Ok(p) = ctx.library.resolve_media_path(single_id) {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                (name, p)
            } else if let Some(tab) = ctx.tab_manager.get_tab_by_media_id(single_id) {
                (tab.filename.clone(), tab.file_path.clone())
            } else {
                return;
            }
        };

        if !full_path.exists() {
            tracing::warn!(
                single_id,
                path = %LogObscurer::path(&full_path),
                "[MediaContextMenu] Image file not found on disk for copy image content"
            );
            ctx.missing_file_dialog.show(
                window,
                "Copy Image Content",
                vec![MissingFileItem {
                    filename,
                    path: full_path,
                }],
            );
            return;
        }

        tracing::info!(
            single_id,
            path = %LogObscurer::path(&full_path),
            "[MediaContextMenu] Copying raw image content asynchronously to clipboard"
        );

        StatusBarBridge::info(
            window,
            format!("Copying image content '{}' to clipboard...", filename),
        );

        let w_weak = window.as_weak();
        let fname_clone = filename.clone();

        SystemClipboard::copy_image_from_path_async(full_path, move |res| {
            let _ = w_weak.upgrade_in_event_loop(move |w| match res {
                Ok((width, height, metrics)) => {
                    let total_ms = metrics.total_us / 1000;
                    StatusBarBridge::success(
                        &w,
                        format!(
                            "Copied image content ({} x {}) to clipboard in {}ms: '{}'",
                            width, height, total_ms, fname_clone
                        ),
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "[MediaContextMenu] Failed to copy image content to clipboard"
                    );
                    StatusBarBridge::error(&w, format!("Failed to copy image to clipboard: {}", e));
                }
            });
        });
    }

    fn handle_rename_action(
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
                fullscreen_controller: ctx.fullscreen_controller,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(selected_count: usize, deleted: bool) -> MediaSelectionMeta {
        MediaSelectionMeta {
            all_selected_deleted: deleted,
            selected_count,
            is_single_image: selected_count == 1,
            has_video: false,
            target_file_exists: true,
            is_single_pending: false,
            single_filename: "image.jpg".to_string(),
            current_folder_id: None,
            associated_apps: vec![DesktopApp {
                id: "viewer.desktop".to_string(),
                name: "Viewer".to_string(),
                icon_name: "viewer".to_string(),
                icon_path: None,
                exec: "viewer %f".to_string(),
            }],
        }
    }

    #[test]
    fn every_media_menu_entry_has_an_icon() {
        for (count, deleted) in [(1, false), (3, false), (1, true)] {
            let menu = MediaContextMenu::build_menu(&meta(count, deleted), false, None, false);
            assert!(
                menu.entries_without_icon().is_empty(),
                "entries without icon (count={count}, deleted={deleted}): {:?}",
                menu.entries_without_icon()
            );
        }
    }

    #[test]
    fn open_with_entries_fall_back_to_the_generic_application_icon() {
        let menu = MediaContextMenu::build_menu(&meta(1, false), false, None, false);
        let open_with = menu.items.iter().find(|i| i.text == "Open With...").expect("submenu");
        let app_entry = open_with.children.iter().find(|i| i.text == "Viewer").expect("app entry");
        assert_eq!(app_entry.icon, Some(Icon::AppWindow));
    }

    #[test]
    fn rating_entries_use_the_colored_rating_icons() {
        let menu = MediaContextMenu::build_menu(&meta(1, false), false, None, false);
        let rate = menu.items.iter().find(|i| i.text == "Rate Selection").expect("submenu");
        let icons: Vec<Option<Icon>> = rate.children.iter().filter(|i| i.id != 0).map(|i| i.icon).collect();
        assert_eq!(
            icons,
            vec![
                Some(Icon::Rating5),
                Some(Icon::Rating4),
                Some(Icon::Rating3),
                Some(Icon::Rating2),
                Some(Icon::Rating1),
                Some(Icon::RatingClear)
            ]
        );
        assert!(rate.children.iter().all(|i| !i.text.contains('*')));
    }
}
