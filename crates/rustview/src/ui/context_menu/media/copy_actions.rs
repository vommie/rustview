//! Actions of the media context menu that copy a name, a path or the picture to the clipboard.

use super::{MediaActionContext, MediaContextMenu};
use crate::system::SystemClipboard;
use crate::ui::MainWindow;
use crate::ui::file_dialogs::missing::MissingFileItem;
use crate::ui::status_bar::StatusBarBridge;
use rustview_core::library::models::MediaId;
use rustview_core::settings::LogObscurer;
use slint::ComponentHandle;

impl MediaContextMenu {
    pub(super) fn handle_copy_name_action(window: &MainWindow, ctx: &MediaActionContext<'_>, target_ids: &[MediaId]) {
        if target_ids.is_empty() {
            return;
        }

        let mut names = Vec::with_capacity(target_ids.len());
        let st = ctx.state.read();

        for &id in target_ids {
            if let Some(item) = st.all_folder_media.find(id) {
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

    pub(super) fn handle_copy_path_action(window: &MainWindow, ctx: &MediaActionContext<'_>, target_ids: &[MediaId]) {
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

    pub(super) fn handle_copy_image_action(window: &MainWindow, ctx: &MediaActionContext<'_>, target_ids: &[MediaId]) {
        let Some(&single_id) = target_ids.first() else {
            return;
        };

        if target_ids.len() != 1 {
            return;
        }

        let (filename, full_path) = {
            let st = ctx.state.read();
            if let Some(item) = st.all_folder_media.find(single_id) {
                (item.record.filename.clone(), item.full_path())
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

        StatusBarBridge::info(window, format!("Copying image content '{}' to clipboard...", filename));

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
}
