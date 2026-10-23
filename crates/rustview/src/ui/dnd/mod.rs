use crate::system::SystemDndHandler;
use crate::system::fs_access::can_write_directory;
use crate::ui::MainWindow;
use crate::ui::browse_folders::{BrowseDragScroller, BrowseFolders};
use crate::ui::file_dialogs::conflict::FileOpsConflictDialogController;
use crate::ui::file_dialogs::missing::MissingFileDialogController;
use crate::ui::folder_tree::dnd_autoscroller::{
    DndFolderTreeAutoScroller, FOLDER_TREE_ROW_HEIGHT_PX, FOLDER_TREE_TOP_OFFSET_PX,
};
use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::selection::BridgeFileSelection;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::{BatchMoveSummary, FileTransferKind, FolderId, MediaId};
use rustview_core::media::MediaType;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{ComponentHandle, Image, Model};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

mod batch_transfer;
use batch_transfer::BatchTransferRunner;

pub struct DragStartParams<'a> {
    pub media_id: MediaId,
    pub start_x: f32,
    pub start_y: f32,
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub autoscroller: &'a DndFolderTreeAutoScroller,
    pub browse_scroller: &'a BrowseDragScroller,
    pub fullscreen: &'a MultiviewHandle,
}

pub struct BatchTransferParams<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub conflict_dialog: &'a FileOpsConflictDialogController,
    pub target_folder_id: FolderId,
    pub transfer_kind: FileTransferKind,
}

pub struct BridgeDnd;

impl BridgeDnd {
    /// Why files must not be dropped into a folder right now, if so. The scan check is memory only (it also keeps the drop target from
    /// lighting up); `target` adds what a drop needs from the drive: the folder is there and the user may write to it.
    pub fn transfer_block_reason(
        window: &MainWindow,
        state: &RwLock<UiState>,
        target: Option<(&str, &Path)>,
    ) -> Option<String> {
        if BridgeMedia::is_scan_running(window, state) {
            return Some("Cannot move or copy files while a scan or refresh is running.".to_string());
        }
        let (name, path) = target?;
        if !path.exists() {
            return Some("Target folder no longer exists on physical disk.".to_string());
        }
        if !can_write_directory(path) {
            return Some(format!("Cannot move or copy files: the folder '{name}' is read-only."));
        }
        None
    }

    pub fn handle_drag_started(params: DragStartParams<'_>) {
        let t0 = Instant::now();
        params.autoscroller.stop();
        params.browse_scroller.stop();

        // A fullscreen that is isolated from the Browse view locks the main window; a Multiview that follows the Browse
        // selection does not, so dragging works next to it.
        if params.fullscreen.is_isolated_open() {
            tracing::debug!(
                media_id = params.media_id,
                "[MediaDrag: Blocked] Drag & Drop disabled while an isolated fullscreen is open"
            );
            return;
        }

        let (_drag_ids, changed_selection) = {
            let mut st = params.state.write();
            let mut changed = Vec::new();
            if !st.file_selection.is_selected(params.media_id) {
                let prev = st.file_selection.selected_ids().clone();
                st.select_single_media(params.media_id);
                let newly = st.file_selection.selected_ids().clone();
                changed = crate::ui::selection::model::FileSelectionModel::compute_selection_diff(&prev, &newly);
            }
            (st.file_selection.selected_ids().clone(), changed)
        };

        if !changed_selection.is_empty() {
            BrowseFolders::refresh_folder_tiles(params.window, params.state);
            BridgeFileSelection::sync_selection_diff(params.window, params.state, &changed_selection);
            BridgeInspector::update(params.window, params.state);
            BridgeTags::sync_tag_selection(params.window, params.library, params.state);
            StatusBarBridge::update_metrics(params.window, params.state);
        }

        let (drag_count, title, thumbnail_img, ordered_file_paths, thumb_path_opt) = {
            let st = params.state.read();
            let ordered_selected = st.get_selected_media_ordered();
            let count = ordered_selected.len();

            let first_item = ordered_selected
                .first()
                .copied()
                .or_else(|| st.all_folder_media.find(params.media_id));

            let name = first_item.map_or_else(|| format!("Media #{}", params.media_id), |m| m.record.filename.clone());

            let thumb_path_opt = first_item
                .filter(|item| matches!(item.record.media_type, MediaType::Image))
                .map(rustview_core::query::IndexedMediaRecord::full_path)
                .filter(|path| path.exists());

            let thumb = if let Some(buf) = st.thumbs.get(params.media_id) {
                Image::from_rgb8(buf)
            } else {
                Image::default()
            };

            let title_text = if count > 1 {
                format!("{} (+{} more)", name, count - 1)
            } else {
                name
            };

            let mut paths: Vec<PathBuf> = ordered_selected
                .into_iter()
                .map(rustview_core::query::IndexedMediaRecord::full_path)
                .collect();
            if paths.is_empty()
                && let Ok(p) = params.library.resolve_media_path(params.media_id)
            {
                paths.push(p);
            }

            (count, title_text, thumb, paths, thumb_path_opt)
        };

        tracing::info!(
            drag_items_count = drag_count,
            primary_media = %LogObscurer::media(Some(params.media_id), &title),
            start_x = params.start_x,
            start_y = params.start_y,
            ghost_prep_us = t0.elapsed().as_micros(),
            "[MediaDrag: Start] Media drag gesture initiated with visual sorted order and OS-wide ghost overlay"
        );

        params.window.set_is_dragging_media(true);
        params.window.set_drag_count(drag_count as i32);
        params.window.set_drag_title(title.clone().into());
        params.window.set_drag_thumbnail(thumbnail_img);
        params.window.set_drag_x(params.start_x);
        params.window.set_drag_y(params.start_y);
        params.window.set_drop_target_folder_id(-1);
        params.window.set_drop_target_row_id(0);

        let thumb_opt = thumb_path_opt
            .as_deref()
            .or_else(|| ordered_file_paths.first().map(std::path::PathBuf::as_path));
        SystemDndHandler::start_ghost_overlay(&title, drag_count, thumb_opt);
    }

    pub fn handle_drag_moved(
        current_x: f32,
        current_y: f32,
        window: &MainWindow,
        _library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        autoscroller: &DndFolderTreeAutoScroller,
        browse_scroller: &BrowseDragScroller,
    ) {
        if !window.get_is_dragging_media() {
            return;
        }

        window.set_drag_x(current_x);
        window.set_drag_y(current_y);

        autoscroller.update_cursor(window, current_x, current_y);
        browse_scroller.update(window, state, current_x, current_y);

        if !autoscroller.is_active() {
            if BridgeMedia::is_scan_running(window, state) {
                if window.get_drop_target_folder_id() != -1 {
                    window.set_drop_target_folder_id(-1);
                }
                return;
            }
            let active_panel_tab = window.get_active_sidebar_panel_tab();
            if active_panel_tab.as_str() != "folders" {
                if window.get_drop_target_folder_id() != -1 {
                    window.set_drop_target_folder_id(-1);
                }
                return;
            }

            let sidebar_w = window.get_sidebar_left_width();
            let scroll_y = window.get_folder_tree_scroll_y();
            let tree_container_h = window.get_folder_tree_container_height();
            let tree_bottom_y = FOLDER_TREE_TOP_OFFSET_PX + tree_container_h;

            if current_x >= 0.0
                && current_x <= sidebar_w
                && current_y >= FOLDER_TREE_TOP_OFFSET_PX
                && current_y <= tree_bottom_y
            {
                let local_y = current_y - FOLDER_TREE_TOP_OFFSET_PX - scroll_y;
                let row_idx = (local_y / FOLDER_TREE_ROW_HEIGHT_PX).floor() as i32;
                let folders = window.get_folders();

                if row_idx >= 0
                    && (row_idx as usize) < folders.row_count()
                    && let Some(row) = folders.row_data(row_idx as usize)
                    && !row.is_deleted
                {
                    let target_id = row.id as u64;
                    if window.get_drop_target_folder_id() != target_id as i32 {
                        window.set_drop_target_folder_id(target_id as i32);
                    }
                    return;
                }
            }

            if window.get_drop_target_folder_id() != -1 {
                window.set_drop_target_folder_id(-1);
            }
        }
    }

    pub fn handle_folder_hovered(folder_id: FolderId, window: &MainWindow, state: &Arc<RwLock<UiState>>) {
        let is_dragging = window.get_is_dragging_media();
        if !is_dragging {
            return;
        }

        let active_panel_tab = window.get_active_sidebar_panel_tab();
        if active_panel_tab.as_str() != "folders" || BridgeMedia::is_scan_running(window, state) {
            return;
        }

        let is_valid = {
            let st = state.read();
            st.folder_hierarchy
                .as_ref()
                .and_then(|snap| snap.get_node(folder_id))
                .is_none_or(|n| !n.is_deleted)
        };

        if is_valid && window.get_drop_target_folder_id() != folder_id as i32 {
            window.set_drop_target_folder_id(folder_id as i32);
        }
    }

    pub fn handle_folder_unhovered(folder_id: FolderId, window: &MainWindow) {
        if window.get_drop_target_folder_id() == folder_id as i32 {
            window.set_drop_target_folder_id(-1);
        }
    }

    pub fn handle_drag_ended(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        autoscroller: &DndFolderTreeAutoScroller,
        browse_scroller: &BrowseDragScroller,
    ) {
        autoscroller.stop();
        browse_scroller.stop();
        let tree_target_id = window.get_drop_target_folder_id();
        let browse_row = window.get_drop_target_row_id();
        let is_dragging = window.get_is_dragging_media();
        let release_x = window.get_drag_x();
        let release_y = window.get_drag_y();

        window.set_is_dragging_media(false);
        window.set_drop_target_folder_id(-1);
        window.set_drop_target_row_id(0);

        SystemDndHandler::cancel_ghost_overlay();

        if !is_dragging {
            return;
        }

        // A folder row of the Browse view under the pointer wins; the pointer cannot be over the folder tree then.
        let browse_target = (browse_row != 0)
            .then(|| BrowseFolders::drop_target(&state.read(), browse_row))
            .flatten();
        let target_folder_id = browse_target.map_or(tree_target_id, |id| id as i32);

        if target_folder_id > 0
            && let Ok(Some(target_folder_rec)) = library.folders().get_folder(target_folder_id as FolderId)
        {
            let target_name = target_folder_rec.name.clone();
            if let Some(reason) =
                Self::transfer_block_reason(window, state, Some((&target_name, Path::new(&target_folder_rec.path))))
            {
                tracing::info!(target = %LogObscurer::folder(Some(target_folder_id as FolderId), Path::new(&target_folder_rec.path)), %reason, "[MediaDrag: Blocked] Drop refused");
                StatusBarBridge::warn(window, reason);
                return;
            }
            let selected_count = state.read().file_selection.count();
            let sidebar_w = window.get_sidebar_left_width();

            let (popup_x, popup_y) = if browse_target.is_some() {
                (release_x, release_y)
            } else {
                ((sidebar_w + 4.0).min(release_x.max(8.0)), release_y.max(48.0))
            };

            window.set_choice_target_folder_id(target_folder_id);
            window.set_choice_target_folder_name(target_name.into());
            window.set_choice_item_count(selected_count as i32);
            window.set_choice_popup_x(popup_x);
            window.set_choice_popup_y(popup_y);
            window.set_is_choice_popup_open(true);
        } else {
            // While a scan runs no folder is a drop target (nothing lights up); a release over the folder tree or a folder row of the
            // Browse view still gets an answer instead of being taken for a drop onto another application.
            if BridgeMedia::is_scan_running(window, state) {
                let over_tree = window.get_active_sidebar_panel_tab().as_str() == "folders"
                    && release_x <= window.get_sidebar_left_width();
                let over_folder_row = window.invoke_drag_row_id_at(release_x, release_y) != 0;
                if over_tree || over_folder_row {
                    StatusBarBridge::warn(window, "Cannot move or copy files while a scan or refresh is running.");
                    return;
                }
            }
            let ordered_paths = state.read().get_selected_paths_ordered();
            let existing_paths: Vec<PathBuf> = ordered_paths.into_iter().filter(|p| p.exists()).collect();

            if !existing_paths.is_empty() {
                SystemDndHandler::execute_external_drop_async(&existing_paths);
            }
        }
    }

    pub fn start_batch_transfer(params: BatchTransferParams<'_>) {
        let t_start = Instant::now();

        let Ok(Some(target_folder_rec)) = params.library.folders().get_folder(params.target_folder_id) else {
            return;
        };

        let target_folder_path = PathBuf::from(&target_folder_rec.path);
        let target_folder_name = target_folder_rec.name.clone();
        // The popup can stay open while a scan starts or the folder changes: the check is made again before anything moves.
        if let Some(reason) = Self::transfer_block_reason(
            params.window,
            params.state,
            Some((&target_folder_name, &target_folder_path)),
        ) {
            StatusBarBridge::warn(params.window, reason);
            return;
        }

        let ordered_selected_ids = params.state.read().get_selected_ids_ordered();
        if ordered_selected_ids.is_empty() {
            return;
        }

        let mut items_to_transfer = Vec::with_capacity(ordered_selected_ids.len());
        {
            let st = params.state.read();
            for &id in &ordered_selected_ids {
                let folder_id = st
                    .all_folder_media
                    .iter()
                    .find(|m| m.record.id == id)
                    .map(|m| m.record.folder_id)
                    .or_else(|| params.library.media().get(id).ok().flatten().map(|m| m.folder_id));

                if params.transfer_kind == FileTransferKind::Copy || folder_id != Some(params.target_folder_id) {
                    items_to_transfer.push(id);
                }
            }
        }

        if items_to_transfer.is_empty() {
            StatusBarBridge::info(
                params.window,
                format!("All selected files are already in '{target_folder_name}'"),
            );
            return;
        }

        let runner = BatchTransferRunner {
            window: params.window.clone_strong(),
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
            conflict_dialog: params.conflict_dialog.clone(),
            missing_dialog: MissingFileDialogController::new(),
            target_folder_id: params.target_folder_id,
            target_folder_path,
            target_folder_name,
            items: items_to_transfer,
            current_index: 0,
            transfer_kind: params.transfer_kind,
            summary: BatchMoveSummary::default(),
            moved_media_ids: HashSet::new(),
            remembered_resolution: None,
            start_time: t_start,
            first_failure: None,
        };

        runner.step();
    }
}
