use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
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
use crate::library::models::MediaId;
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::system::{DesktopApp, DesktopIntegration, SystemClipboard, SystemFileHandler};
use crate::thumbnails::ThumbnailManager;
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
        library: &Library,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
    ) -> (usize, bool, Vec<DesktopApp>) {
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
        let single_id = state
            .read()
            .file_selection
            .selected_ids()
            .iter()
            .next()
            .copied()
            .unwrap_or(media_id);

        let (is_single_image, target_path_opt) = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == single_id) {
                (matches!(item.record.media_type, MediaType::Image), Some(item.full_path.clone()))
            } else {
                let is_img = library
                    .media()
                    .get(single_id)
                    .ok()
                    .flatten()
                    .map(|m| matches!(m.media_type, MediaType::Image))
                    .unwrap_or(false);
                let path = library.resolve_media_path(single_id).ok();
                (is_img, path)
            }
        };

        let associated_apps = if let Some(ref path) = target_path_opt {
            DesktopIntegration::get_associated_apps(path)
        } else {
            Vec::new()
        };

        state.write().active_associated_apps = associated_apps.clone();

        (selected_count, is_single && is_single_image, associated_apps)
    }

    pub fn build_menu(
        selected_count: usize,
        is_single_image: bool,
        _is_busy: bool,
        associated_apps: &[DesktopApp],
    ) -> CtxMenu {
        let is_single = selected_count == 1;
        let can_open = selected_count > 0;
        let can_rename = is_single;
        let can_copy = selected_count > 0;
        let can_refresh = selected_count > 0;

        let refresh_text = if is_single {
            "Refresh File".to_string()
        } else {
            format!("Refresh {} Files", selected_count)
        };

        let copy_name_text = if is_single {
            "File Name".to_string()
        } else {
            format!("File Names ({})", selected_count)
        };

        let copy_path_text = if is_single {
            "File Path".to_string()
        } else {
            format!("File Paths ({})", selected_count)
        };

        let mut builder = CtxMenuBuilder::new()
            .header(if is_single {
                "Selected File"
            } else {
                "Selected Files"
            })
            .action(
                CTX_ACTION_OPEN,
                if is_single {
                    "Open".to_string()
                } else {
                    format!("Open ({} Files)", selected_count)
                },
                None,
                None,
                can_open,
            );

        // Submenu: Open With... (directly below Open with real Freedesktop application icons)
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

        builder = builder
            .action(
                CTX_ACTION_OPEN_IN_FILE_MANAGER,
                if is_single {
                    "Open in File Manager".to_string()
                } else {
                    format!("Open in File Manager ({} Files)", selected_count)
                },
                None,
                None,
                can_open,
            )
            .separator()
            .action(
                CTX_ACTION_RENAME,
                if is_single {
                    "Rename...".to_string()
                } else {
                    format!("Rename (Single File Only, {} selected)", selected_count)
                },
                Some("F2"),
                None,
                can_rename,
            )
            .submenu("Copy...", None, can_copy, move |sub| {
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
            .action(
                CTX_ACTION_REFRESH_FILES,
                refresh_text,
                Some("F5"),
                None,
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
            );

        builder.build()
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

        if (CTX_ACTION_OPEN_WITH_APP_BASE..CTX_ACTION_OPEN_WITH_APP_BASE + 500).contains(&action_id) {
            let app_idx = (action_id - CTX_ACTION_OPEN_WITH_APP_BASE) as usize;
            Self::handle_open_with_specific_app(window, ctx, &selected_ids, app_idx);
            return;
        }

        match action_id {
            CTX_ACTION_OPEN => {
                Self::handle_open_action(window, ctx, &selected_ids);
            }
            CTX_ACTION_OPEN_WITH_OTHER => {
                Self::handle_open_with_other_action(window, ctx, &selected_ids);
            }
            CTX_ACTION_OPEN_IN_FILE_MANAGER => {
                Self::handle_open_in_file_manager_action(window, ctx, &selected_ids);
            }
            CTX_ACTION_COPY_NAME => {
                Self::handle_copy_name_action(window, ctx, &selected_ids);
            }
            CTX_ACTION_COPY_PATH => {
                Self::handle_copy_path_action(window, ctx, &selected_ids);
            }
            CTX_ACTION_COPY_IMAGE => {
                Self::handle_copy_image_action(window, ctx, &selected_ids);
            }
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

    fn handle_open_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) {
        if selected_ids.is_empty() {
            return;
        }

        tracing::info!(
            selected_count = selected_ids.len(),
            "[MediaContextMenu] 'Open' action triggered from context menu"
        );

        for &id in selected_ids {
            BridgeFileSelection::open_media(id, ctx.state, ctx.library, window);
        }
    }

    fn handle_open_with_specific_app(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
        app_idx: usize,
    ) {
        let app_opt = ctx.state.read().active_associated_apps.get(app_idx).cloned();
        let Some(app) = app_opt else {
            tracing::warn!(app_idx, "[MediaContextMenu] Target desktop app index not found");
            return;
        };

        let paths = Self::collect_selected_paths(ctx, selected_ids);
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
            window.set_status_text(format!("Failed to launch '{}': {}", app.name, e).into());
        } else {
            window.set_status_text(format!("Launched '{}' with {} file(s).", app.name, paths.len()).into());
        }
    }

    fn handle_open_with_other_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) {
        let paths = Self::collect_selected_paths(ctx, selected_ids);
        let Some(first_path) = paths.first() else {
            return;
        };

        tracing::info!(
            target_file = %LogObscurer::path(first_path),
            "[MediaContextMenu] 'Other Application...' triggered"
        );

        if let Err(e) = DesktopIntegration::open_with_dialog(first_path) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to open native app chooser");
            window.set_status_text(format!("Failed to open application chooser: {}", e).into());
        } else {
            window.set_status_text("Opened native application chooser dialog.".into());
        }
    }

    fn collect_selected_paths(
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) -> Vec<PathBuf> {
        let st = ctx.state.read();
        let mut paths: Vec<PathBuf> = Vec::with_capacity(selected_ids.len());

        for item in &st.all_folder_media {
            if selected_ids.contains(&item.record.id) {
                paths.push(item.full_path.clone());
            }
        }
        drop(st);

        if paths.is_empty() {
            for &id in selected_ids {
                if let Ok(p) = ctx.library.resolve_media_path(id) {
                    paths.push(p);
                }
            }
        }

        paths
    }

    fn handle_open_in_file_manager_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) {
        let paths = Self::collect_selected_paths(ctx, selected_ids);
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
                    window.set_status_text(
                        format!("Opened file '{}' in File Manager.", name).into(),
                    );
                } else {
                    window.set_status_text(
                        format!("Opened {} files in File Manager.", count).into(),
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "[MediaContextMenu] Failed to open file(s) in File Manager"
                );
                window.set_status_text(format!("Failed to open File Manager: {}", e).into());
            }
        }
    }

    fn handle_copy_name_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) {
        if selected_ids.is_empty() {
            return;
        }

        let st = ctx.state.read();
        let mut names = Vec::with_capacity(selected_ids.len());

        for item in &st.all_folder_media {
            if selected_ids.contains(&item.record.id) {
                names.push(item.record.filename.clone());
            }
        }
        drop(st);

        if names.is_empty() {
            for &id in selected_ids {
                if let Ok(Some(rec)) = ctx.library.media().get(id) {
                    names.push(rec.filename);
                }
            }
        }

        let count = names.len();
        if let Err(e) = SystemClipboard::copy_file_names(&names) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to copy file names to clipboard");
            window.set_status_text(format!("Failed to copy to clipboard: {}", e).into());
        } else if count == 1 {
            tracing::info!(name = %LogObscurer::filename(&names[0]), "[MediaContextMenu] Copied single file name to clipboard");
            window.set_status_text(format!("Copied file name to clipboard: '{}'", names[0]).into());
        } else {
            tracing::info!(count = count, "[MediaContextMenu] Copied multiple file names to clipboard");
            window.set_status_text(format!("Copied {} file names to clipboard.", count).into());
        }
    }

    fn handle_copy_path_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) {
        let paths = Self::collect_selected_paths(ctx, selected_ids);
        if paths.is_empty() {
            return;
        }

        let count = paths.len();
        if let Err(e) = SystemClipboard::copy_file_paths(&paths) {
            tracing::error!(error = %e, "[MediaContextMenu] Failed to copy file paths to clipboard");
            window.set_status_text(format!("Failed to copy to clipboard: {}", e).into());
        } else if count == 1 {
            let path_str = paths[0].display().to_string();
            tracing::info!(path = %LogObscurer::path(&paths[0]), "[MediaContextMenu] Copied single file path to clipboard");
            window.set_status_text(format!("Copied file path to clipboard: '{}'", path_str).into());
        } else {
            tracing::info!(count = count, "[MediaContextMenu] Copied multiple file paths to clipboard");
            window.set_status_text(format!("Copied {} file paths to clipboard.", count).into());
        }
    }

    fn handle_copy_image_action(
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
        selected_ids: &std::collections::HashSet<MediaId>,
    ) {
        let Some(&single_id) = selected_ids.iter().next() else {
            return;
        };

        if selected_ids.len() != 1 {
            return;
        }

        let (filename, full_path) = {
            let st = ctx.state.read();
            if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == single_id) {
                (item.record.filename.clone(), item.full_path.clone())
            } else if let Ok(p) = ctx.library.resolve_media_path(single_id) {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                (name, p)
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

        window.set_status_text(
            format!("Copying image content '{}' to clipboard...", filename).into(),
        );

        let w_weak = window.as_weak();
        let fname_clone = filename.clone();

        SystemClipboard::copy_image_from_path_async(full_path, move |res| {
            let _ = w_weak.upgrade_in_event_loop(move |w| match res {
                Ok((width, height, metrics)) => {
                    let total_ms = metrics.total_us / 1000;
                    w.set_status_text(
                        format!(
                            "Copied image content ({} × {}) to clipboard in {}ms: '{}'",
                            width, height, total_ms, fname_clone
                        )
                        .into(),
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "[MediaContextMenu] Failed to copy image content to clipboard in worker"
                    );
                    w.set_status_text(format!("Failed to copy image to clipboard: {}", e).into());
                }
            });
        });
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
