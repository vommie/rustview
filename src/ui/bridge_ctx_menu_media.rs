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
use super::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{
    CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher,
};
use crate::library::models::MediaId;
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

/// Grouped dependencies bundle for media context menu actions.
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

/// Metadata describing active selection state for context menu construction.
#[derive(Debug, Clone)]
pub struct MediaSelectionMeta {
    pub all_selected_deleted: bool,
    pub selected_count: usize,
    pub is_single_image: bool,
    pub has_video: bool,
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

        let (has_video, is_single_image, all_selected_deleted, target_path_opt) = {
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

            let is_single_img = if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == single_id) {
                matches!(item.record.media_type, MediaType::Image)
            } else {
                library
                    .media()
                    .get(single_id)
                    .ok()
                    .flatten()
                    .map(|m| matches!(m.media_type, MediaType::Image))
                    .unwrap_or(false)
            };

            let path = if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == single_id) {
                Some(item.full_path.clone())
            } else {
                library.resolve_media_path(single_id).ok()
            };

            (contains_video, is_single_img, all_deleted, path)
        };

        let associated_apps = if let Some(ref path) = target_path_opt {
            DesktopIntegration::get_associated_apps(path)
        } else {
            Vec::new()
        };

        state.write().active_associated_apps = associated_apps.clone();

        MediaSelectionMeta {
            all_selected_deleted,
            selected_count,
            is_single_image: is_single && is_single_image,
            has_video,
            associated_apps,
        }
    }

    /// Single Point of Truth (SPoT): Reusable builder appending the file open actions:
    /// "Open", "Open With..." (desktop application icons + Other Application...), and "Open in File Manager".
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
            None,
            can_open,
        );

        if can_open {
            let apps = associated_apps.to_vec();
            builder = builder.submenu("Open With...", None, true, move |mut sub| {
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
                    None,
                    true,
                )
            });
        }

        builder = builder.action(
            CTX_ACTION_OPEN_IN_FILE_MANAGER,
            if is_single {
                "Open in File Manager".to_string()
            } else {
                "Open in File Manager (Multiple Files)".to_string()
            },
            None,
            None,
            can_open,
        );

        builder
    }

    /// Single Point of Truth (SPoT): Reusable builder appending file management actions:
    /// "Rename...", "Copy..." (File Name, File Path, and Image Content if single image),
    /// and "Delete File..." / "Remove from Database...".
    pub fn append_file_management_entries(
        mut builder: CtxMenuBuilder,
        is_single: bool,
        is_single_image: bool,
        all_selected_deleted: bool,
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

        let delete_action_id = if all_selected_deleted {
            CTX_ACTION_REMOVE_FROM_DB
        } else {
            CTX_ACTION_DELETE_MEDIA
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
                None,
                can_rename,
            )
            .submenu("Copy...", None, true, move |sub| {
                let mut sub_builder = sub
                    .action(CTX_ACTION_COPY_NAME, copy_name_text, None, None, true)
                    .action(CTX_ACTION_COPY_PATH, copy_path_text, None, None, true);

                if is_single_image {
                    sub_builder = sub_builder.action(
                        CTX_ACTION_COPY_IMAGE,
                        "Image Content",
                        None,
                        None,
                        true,
                    );
                }
                sub_builder
            })
            .action(
                delete_action_id,
                delete_action_text,
                Some("Del"),
                Some("trash"),
                !is_busy,
            );

        builder
    }

    pub fn build_menu(
        selected_count: usize,
        is_single_image: bool,
        has_video: bool,
        all_selected_deleted: bool,
        is_busy: bool,
        associated_apps: &[DesktopApp],
    ) -> CtxMenu {
        let is_single = selected_count == 1;
        let can_open = selected_count > 0;

        let open_in_tab_text = if is_single {
            "Open in Tab".to_string()
        } else {
            format!("Open in Tabs ({} Files)", selected_count)
        };
        let open_in_tab_shortcut = if is_single { Some("DoubleClick") } else { None };

        let open_in_bg_tab_text = if is_single {
            "Open in Background-Tab".to_string()
        } else {
            format!("Open in Background-Tabs ({} Files)", selected_count)
        };
        let open_in_bg_tab_shortcut = if is_single { Some("Ctrl+MiddleClick") } else { None };

        let open_in_fullscreen_text = "Open in Fullscreen".to_string();
        let open_in_fullscreen_shortcut = if is_single { Some("MiddleClick") } else { None };

        // Submenu: Refresh -> File (Full), File (Metadata), Thumbnail (Random Pos.) / Thumbnail
        let refresh_full_text = if is_single {
            "File (Full)".to_string()
        } else {
            format!("Files (Full) ({} Files)", selected_count)
        };

        let refresh_meta_text = if is_single {
            "File (Metadata)".to_string()
        } else {
            format!("Files (Metadata) ({} Files)", selected_count)
        };

        // Context-sensitive naming: videos specify Random Pos., pure image selections use Thumbnail
        let regen_thumb_text = if is_single {
            if has_video {
                "Thumbnail (Random Pos.)".to_string()
            } else {
                "Thumbnail".to_string()
            }
        } else if has_video {
            format!("Thumbnails (Random Pos.) ({} Files)", selected_count)
        } else {
            format!("Thumbnails ({} Files)", selected_count)
        };

        let can_refresh = selected_count > 0 && !is_busy;

        let mut builder = CtxMenuBuilder::new()
            .header(if is_single {
                "Selected File"
            } else {
                "Selected Files"
            });

        // 1. Shared File Open Actions (Open, Open With..., Open in File Manager)
        builder = Self::append_file_open_entries(builder, is_single, can_open, associated_apps);

        // 2. Tab Navigation Actions
        builder = builder
            .action(
                CTX_ACTION_OPEN_IN_TAB,
                open_in_tab_text,
                open_in_tab_shortcut,
                None,
                can_open,
            )
            .action(
                CTX_ACTION_OPEN_IN_BG_TAB,
                open_in_bg_tab_text,
                open_in_bg_tab_shortcut,
                None,
                can_open,
            )
            .action(
                CTX_ACTION_OPEN_IN_FULLSCREEN,
                open_in_fullscreen_text,
                open_in_fullscreen_shortcut,
                None,
                can_open,
            )
            .separator();

        // 3. Shared File Management Actions (Rename, Copy, Delete File / Remove from DB)
        builder = Self::append_file_management_entries(
            builder,
            is_single,
            is_single_image,
            all_selected_deleted,
            is_busy,
        );

        // 4. Rating, Modular Refresh Submenu & Selection Utilities
        builder = builder
            .separator()
            .submenu(
                "Rate Selection",
                None,
                selected_count > 0,
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
            .submenu(
                "Refresh",
                Some("refresh"),
                can_refresh,
                |sub| {
                    sub.action(
                        CTX_ACTION_REFRESH_FILES,
                        refresh_full_text,
                        Some("F5"),
                        Some("refresh"),
                        can_refresh,
                    )
                    .action(
                        CTX_ACTION_REFRESH_METADATA,
                        refresh_meta_text,
                        None,
                        Some("refresh"),
                        can_refresh,
                    )
                    .action(
                        CTX_ACTION_REGENERATE_THUMB_RANDOM,
                        regen_thumb_text,
                        None,
                        Some("refresh"),
                        can_refresh,
                    )
                },
            )
            .separator()
            .action(
                CTX_ACTION_SELECT_ALL,
                "Select All",
                Some("Ctrl+A"),
                None,
                true,
            )
            .action(
                CTX_ACTION_CLEAR_SELECTION,
                "Clear Selection",
                Some("Esc"),
                None,
                selected_count > 0,
            );

        builder.build()
    }

    /// Primary action handler executing media file context actions on active browser selections.
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

    /// Single Point of Truth (SPoT): Shared execution logic for file operations across both Media and Tab context menus.
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
                if BridgeMedia::is_system_busy(window, ctx.thumbnails, ctx.state) {
                    StatusBarBridge::warn(
                        window,
                        "Cannot regenerate thumbnail: another operation is already in progress.",
                    );
                    return;
                }
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
            return;
        }

        tracing::info!(
            app = %app.name,
            paths_count = paths.len(),
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
            return;
        }

        let count = paths.len();
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
            return;
        }

        let count = names.len();
        if let Err(e) = SystemClipboard::copy_file_names(&names) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to copy file names to clipboard");
            StatusBarBridge::error(window, format!("Failed to copy to clipboard: {}", e));
        } else if count == 1 {
            tracing::info!(name = %LogObscurer::filename(&names[0]), "[MediaContextMenu] Copied single file name to clipboard");
            StatusBarBridge::success(window, format!("Copied file name to clipboard: '{}'", names[0]));
        } else {
            tracing::info!(count = count, "[MediaContextMenu] Copied multiple file names to clipboard");
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
            return;
        }

        let count = paths.len();
        if let Err(e) = SystemClipboard::copy_file_paths(&paths) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to copy file paths to clipboard");
            StatusBarBridge::error(window, format!("Failed to copy to clipboard: {}", e));
        } else if count == 1 {
            let path_str = paths[0].display().to_string();
            tracing::info!(path = %LogObscurer::path(&paths[0]), "[MediaContextMenu] Copied single file path to clipboard");
            StatusBarBridge::success(window, format!("Copied file path to clipboard: '{}'", path_str));
        } else {
            tracing::info!(count = count, "[MediaContextMenu] Copied multiple file paths to clipboard");
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
                            "Copied image content ({} × {}) to clipboard in {}ms: '{}'",
                            width, height, total_ms, fname_clone
                        ),
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "[MediaContextMenu] Failed to copy image content to clipboard in worker"
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
