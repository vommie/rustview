use super::bridge_file_dialog_conflict::{ConflictDialogParams, FileOpsConflictDialogController};
use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::bridge_file_selection::BridgeFileSelection;
use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::dnd_foldertree_autoscroller::{
    DndFolderTreeAutoScroller, FOLDER_TREE_ROW_HEIGHT_PX, FOLDER_TREE_TOP_OFFSET_PX,
};
use super::MainWindow;
use crate::library::error::LibraryError;
use crate::library::file_operations::FileOperations;
use crate::library::models::{
    BatchMoveSummary, FileConflictReport, FileConflictResolution, FileTransferKind, FolderId,
    MediaId,
};
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::system::SystemDndHandler;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{ComponentHandle, Image, Model, Timer};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Parameter struct grouping drag initiation arguments.
pub struct DragStartParams<'a> {
    pub media_id: MediaId,
    pub start_x: f32,
    pub start_y: f32,
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub autoscroller: &'a DndFolderTreeAutoScroller,
}

/// Parameter struct grouping batch transfer arguments.
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

/// Encapsulated coordinator managing media drag gestures, flying ghost overlays, drop hit-testing,
/// floating choice popups, internal batched folder relocations, and external Drag & Drop exports to 3rd-party applications.
pub struct BridgeDnd;

impl BridgeDnd {
    /// Handles drag initiation from media grid tiles or table rows, strictly preserving the visual sorted order of selected items.
    pub fn handle_drag_started(params: DragStartParams<'_>) {
        let t0 = Instant::now();
        params.autoscroller.stop();

        let (_drag_ids, changed_selection) = {
            let mut st = params.state.write();
            let mut changed = Vec::new();
            if !st.file_selection.is_selected(params.media_id) {
                let prev = st.file_selection.selected_ids().clone();
                st.file_selection.select_single(params.media_id);
                let newly = st.file_selection.selected_ids().clone();
                changed = super::file_selection::FileSelectionModel::compute_selection_diff(&prev, &newly);
            }
            (st.file_selection.selected_ids().clone(), changed)
        };

        if !changed_selection.is_empty() {
            BridgeFileSelection::sync_selection_diff(params.window, params.state, &changed_selection);
            BridgeInspector::update(params.window, params.state);
            BridgeTags::sync_tag_selection(params.window, params.library, params.state);
        }

        // Collect all selected items in exact visual sorted order with instant zero-cost thumbnail lookup
        let (drag_count, title, thumbnail_img, ordered_file_paths, thumb_path_opt) = {
            let st = params.state.read();
            let ordered_selected = st.get_selected_media_ordered();
            let count = ordered_selected.len();

            let first_item = ordered_selected
                .first()
                .copied()
                .or_else(|| st.all_folder_media.iter().find(|m| m.record.id == params.media_id));

            let name = first_item
                .map(|m| m.record.filename.clone())
                .unwrap_or_else(|| format!("Media #{}", params.media_id));

            // Instant zero-cost thumbnail resolution without blocking the main UI thread with PNG compression
            let thumb_path_opt = first_item.and_then(|item| {
                if matches!(item.record.media_type, MediaType::Image) && item.full_path.exists() {
                    Some(item.full_path.clone())
                } else {
                    None
                }
            });

            let thumb = if let Some(buf) = st.folder_thumbnails.get(&params.media_id) {
                Image::from_rgb8(buf.clone())
            } else {
                Image::default()
            };

            let title_text = if count > 1 {
                format!("{} (+{} more)", name, count - 1)
            } else {
                name
            };

            let mut paths: Vec<PathBuf> = ordered_selected.into_iter().map(|item| item.full_path.clone()).collect();
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

        // Start OS-wide click-through floating ghost overlay tracking mouse everywhere with real thumbnail and Catppuccin theme
        let thumb_opt = thumb_path_opt
            .as_deref()
            .or_else(|| ordered_file_paths.first().map(|p| p.as_path()));
        SystemDndHandler::start_ghost_overlay(&title, drag_count, thumb_opt);
    }

    /// Handles pointer movement during active media dragging with continuous auto-scroller updates and FolderTree hit-testing.
    pub fn handle_drag_moved(
        current_x: f32,
        current_y: f32,
        window: &MainWindow,
        _library: &Arc<Library>,
        _state: &Arc<RwLock<UiState>>,
        autoscroller: &DndFolderTreeAutoScroller,
    ) {
        window.set_drag_x(current_x);
        window.set_drag_y(current_y);

        // Update auto-scroll physics and hotzone evaluation
        autoscroller.update_cursor(window, current_x, current_y);

        // If auto-scrolling is not actively shifting content, perform stationary hit-testing
        if !autoscroller.is_active() {
            let sidebar_w = window.get_sidebar_left_width();
            let scroll_y = window.get_folder_tree_scroll_y();
            let window_h = window.window().size().height as f32;
            let tree_bottom_y = window_h - 28.0;

            if current_x >= 0.0 && current_x <= sidebar_w && current_y >= FOLDER_TREE_TOP_OFFSET_PX && current_y <= tree_bottom_y {
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
                        tracing::info!(
                            target_folder_id = target_id,
                            folder_name = %row.name,
                            row_index = row_idx,
                            cursor_x = current_x,
                            cursor_y = current_y,
                            "[MediaDrag: TargetHover] Cursor entered valid folder drop target in tree"
                        );
                        window.set_drop_target_folder_id(target_id as i32);
                    }
                    return;
                }
            }

            if window.get_drop_target_folder_id() != -1 {
                tracing::debug!(
                    cursor_x = current_x,
                    cursor_y = current_y,
                    "[MediaDrag: TargetLeave] Cursor moved outside folder tree drop zone"
                );
                window.set_drop_target_folder_id(-1);
            }
        }
    }

    /// Handles hover entry over a folder tree row during an active drag.
    pub fn handle_folder_hovered(folder_id: FolderId, window: &MainWindow, state: &Arc<RwLock<UiState>>) {
        let is_dragging = window.get_is_dragging_media();
        if !is_dragging {
            return;
        }

        let is_valid_target = {
            let st = state.read();
            if let Some(ref tree) = st.cached_folder_tree {
                Self::is_folder_valid_in_tree(tree, folder_id)
            } else {
                true
            }
        };

        if is_valid_target && window.get_drop_target_folder_id() != folder_id as i32 {
            tracing::info!(
                target_folder_id = folder_id,
                "[MediaDrag: TargetHover] Row hover callback activated for folder"
            );
            window.set_drop_target_folder_id(folder_id as i32);
        }
    }

    /// Handles hover leave from a folder tree row.
    pub fn handle_folder_unhovered(folder_id: FolderId, window: &MainWindow) {
        if window.get_drop_target_folder_id() == folder_id as i32 {
            window.set_drop_target_folder_id(-1);
        }
    }

    /// Handles drag release: if dropped on an internal folder, opens Choice Popup.
    /// If dropped outside internal folders (e.g. over VLC, MPV, Chrome, Dolphin, Discord, etc.), executes direct XDND drop transaction.
    pub fn handle_drag_ended(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        autoscroller: &DndFolderTreeAutoScroller,
    ) {
        autoscroller.stop();
        let target_folder_id = window.get_drop_target_folder_id();
        let is_dragging = window.get_is_dragging_media();
        let release_x = window.get_drag_x();
        let release_y = window.get_drag_y();

        window.set_is_dragging_media(false);
        window.set_drop_target_folder_id(-1);

        // Immediately dismiss the OS ghost overlay window (0ms latency)
        SystemDndHandler::cancel_ghost_overlay();

        if !is_dragging {
            return;
        }

        if target_folder_id > 0 {
            if let Ok(Some(target_folder_rec)) = library.folders().get_folder(target_folder_id as FolderId) {
                let target_folder_path = PathBuf::from(&target_folder_rec.path);
                let target_name = target_folder_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| target_folder_rec.path.clone());

                let selected_count = state.read().file_selection.count();
                let sidebar_w = window.get_sidebar_left_width();

                // Compute anchor position near the sidebar edge / release point
                let popup_x = (sidebar_w + 4.0).min(release_x.max(8.0));
                let popup_y = release_y.max(48.0);

                tracing::info!(
                    target_folder_id = target_folder_id,
                    target_name = %target_name,
                    items_count = selected_count,
                    popup_x = popup_x,
                    popup_y = popup_y,
                    "[DndChoicePopup: Show] Presenting Floating Choice Popup at drop target folder"
                );

                window.set_choice_target_folder_id(target_folder_id);
                window.set_choice_target_folder_name(target_name.into());
                window.set_choice_item_count(selected_count as i32);
                window.set_choice_popup_x(popup_x);
                window.set_choice_popup_y(popup_y);
                window.set_is_choice_popup_open(true);
            }
        } else {
            // Drag released outside internal folder targets -> Execute external Drop into 3rd-party application
            let ordered_paths = state.read().get_selected_paths_ordered();
            if !ordered_paths.is_empty() {
                tracing::info!(
                    files_count = ordered_paths.len(),
                    "[MediaDrag: ExternalDrop] Dropped outside RustView folder tree. Triggering external application XDND drop..."
                );
                SystemDndHandler::execute_external_drop_async(&ordered_paths);
            }
        }
    }

    /// Orchestrates the batch transfer operation (Copy or Move), sequentially checking conflicts, displaying split-view comparison dialogs, and atomically committing changes.
    pub fn start_batch_transfer(params: BatchTransferParams<'_>) {
        let t_start = Instant::now();

        let Ok(Some(target_folder_rec)) = params.library.folders().get_folder(params.target_folder_id) else {
            tracing::warn!(target_folder_id = params.target_folder_id, "[BatchTransfer] Target folder not found in database");
            return;
        };

        let target_folder_path = PathBuf::from(&target_folder_rec.path);
        if !target_folder_path.exists() {
            tracing::warn!(
                target_folder = %LogObscurer::folder(Some(params.target_folder_id), &target_folder_path),
                "[BatchTransfer] Target folder no longer exists on physical storage"
            );
            params.window.set_status_text("Target folder no longer exists on physical disk.".into());
            return;
        }

        let target_folder_name = target_folder_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| target_folder_rec.path.clone());

        // Extract selected IDs in exact visual sorted order
        let ordered_selected_ids = params.state.read().get_selected_ids_ordered();
        if ordered_selected_ids.is_empty() {
            return;
        }

        // For Move: Filter out items already in the target folder
        // For Copy: Allow copying into target folder even if source is the same folder
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
            tracing::info!(
                target_folder = %LogObscurer::folder(Some(params.target_folder_id), &target_folder_path),
                total_selected = ordered_selected_ids.len(),
                "[BatchTransfer] All selected media files already reside in the target directory. No-op."
            );
            params.window.set_status_text(
                format!("All selected files are already in '{}'", target_folder_name).into(),
            );
            return;
        }

        tracing::info!(
            target_folder = %LogObscurer::folder(Some(params.target_folder_id), &target_folder_path),
            transfer_kind = ?params.transfer_kind,
            total_items = items_to_transfer.len(),
            "[BatchTransfer: Init] Starting batch file transfer workflow..."
        );

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
        };

        runner.step();
    }

    fn is_folder_valid_in_tree(nodes: &[super::bridge_folders::FolderTreeNode], target_id: FolderId) -> bool {
        for node in nodes {
            if node.record.id == target_id {
                return !node.record.is_deleted;
            }
            if Self::is_folder_valid_in_tree(&node.children, target_id) {
                return true;
            }
        }
        false
    }
}

/// Sequential asynchronous step runner managing conflict dialog popups and atomic transfers across a batch of media items.
struct BatchTransferRunner {
    window: MainWindow,
    library: Arc<Library>,
    thumbnails: Arc<ThumbnailManager>,
    settings: Arc<SettingsManager>,
    state: Arc<RwLock<UiState>>,
    conflict_dialog: FileOpsConflictDialogController,
    missing_dialog: MissingFileDialogController,
    target_folder_id: FolderId,
    target_folder_path: PathBuf,
    target_folder_name: String,
    items: Vec<MediaId>,
    current_index: usize,
    transfer_kind: FileTransferKind,
    summary: BatchMoveSummary,
    moved_media_ids: HashSet<MediaId>,
    remembered_resolution: Option<FileConflictResolution>,
    start_time: Instant,
}

impl BatchTransferRunner {
    fn step(mut self) {
        if self.current_index >= self.items.len() {
            self.finish();
            return;
        }

        let media_id = self.items[self.current_index];

        // Check if the record still exists in the library (might have been overwritten by an earlier item in the same batch)
        if self.library.media().get(media_id).ok().flatten().is_none() {
            tracing::info!(
                media_id = media_id,
                "[BatchTransfer: SkipOverwritten] Media #{} was consumed/overwritten by a preceding item in this batch. Skipping.",
                media_id
            );
            self.current_index += 1;
            Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            return;
        }

        self.summary.total_requested += 1;
        let t_step = Instant::now();

        // Check if collision exists in destination folder
        match self.library.file_operations().check_transfer_conflict(
            media_id,
            self.target_folder_id,
            self.transfer_kind,
        ) {
            Ok(Some(conflict_report)) => {
                if let Some(res) = self.remembered_resolution {
                    tracing::debug!(
                        media_id = media_id,
                        remembered_action = ?res,
                        "[BatchTransfer: AutoApply] Applying remembered collision decision to remaining conflict"
                    );
                    self.apply_resolution(media_id, &conflict_report, res);
                } else {
                    let conflict_for_show = conflict_report.clone();
                    let conflict_for_closure = conflict_report.clone();
                    let runner_rc = Rc::new(RefCell::new(Some(self)));
                    let runner_for_resolve = runner_rc.clone();

                    let win = runner_rc.borrow().as_ref().unwrap().window.clone_strong();
                    let thumb = runner_rc.borrow().as_ref().unwrap().thumbnails.clone();
                    let set = runner_rc.borrow().as_ref().unwrap().settings.clone();
                    let dlg = runner_rc.borrow().as_ref().unwrap().conflict_dialog.clone();
                    let kind = runner_rc.borrow().as_ref().unwrap().transfer_kind;
                    let total_items = runner_rc.borrow().as_ref().unwrap().items.len();
                    let current_idx = runner_rc.borrow().as_ref().unwrap().current_index;

                    tracing::info!(
                        media_id = media_id,
                        transfer_kind = ?kind,
                        source_filename = %LogObscurer::filename(&conflict_for_show.source.filename),
                        item_index = current_idx + 1,
                        total_items = total_items,
                        "[BatchTransfer: Prompt] Prompting user with side-by-side Conflict Dialog..."
                    );

                    dlg.show(
                        ConflictDialogParams {
                            window: &win,
                            thumbnails: &thumb,
                            settings: &set,
                            report: &conflict_for_show,
                            conflict_index: current_idx + 1,
                            total_conflicts: total_items,
                            transfer_kind: kind,
                        },
                        move |resolution, apply_to_all| {
                            if let Some(mut runner) = runner_for_resolve.borrow_mut().take() {
                                if apply_to_all {
                                    tracing::info!(
                                        chosen_resolution = ?resolution,
                                        "[BatchTransfer: ApplyToAll] Remembered collision action for all remaining items in batch"
                                    );
                                    runner.remembered_resolution = Some(resolution);
                                }
                                runner.apply_resolution(media_id, &conflict_for_closure, resolution);
                            }
                        },
                    );
                }
            }
            Ok(None) => {
                // No conflict -> execute direct transfer
                let exec_res = match self.transfer_kind {
                    FileTransferKind::Move => self.library.file_operations().move_single_media(
                        media_id,
                        self.target_folder_id,
                        None,
                        false,
                    ),
                    FileTransferKind::Copy => self.library.file_operations().copy_single_media(
                        media_id,
                        self.target_folder_id,
                        None,
                        false,
                    ),
                };

                match exec_res {
                    Ok(rec) => {
                        tracing::debug!(
                            media = %LogObscurer::media(Some(media_id), &rec.filename),
                            transfer_kind = ?self.transfer_kind,
                            step_time_us = t_step.elapsed().as_micros(),
                            "[BatchTransfer: Direct] Successfully processed media item without collision"
                        );
                        self.summary.moved_count += 1;
                        if self.transfer_kind == FileTransferKind::Move {
                            self.moved_media_ids.insert(media_id);
                        }
                    }
                    Err(LibraryError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                        self.summary.failed_count += 1;
                        let missing_path = self.resolve_media_path(media_id);
                        tracing::warn!(
                            media_id = media_id,
                            missing_path = %LogObscurer::path(&missing_path),
                            "[BatchTransfer: MissingFile] Source media file was not found on physical disk"
                        );
                        self.missing_dialog.show(
                            &self.window,
                            if self.transfer_kind == FileTransferKind::Move { "Move File" } else { "Copy File" },
                            vec![MissingFileItem {
                                filename: format!("Media #{}", media_id),
                                path: missing_path,
                            }],
                        );
                    }
                    Err(e) => {
                        tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: Error] Direct transfer failed");
                        self.summary.failed_count += 1;
                    }
                }
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            Err(e) => {
                tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: Error] Failed to check conflict");
                self.summary.failed_count += 1;
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
        }
    }

    fn apply_resolution(
        mut self,
        media_id: MediaId,
        conflict: &FileConflictReport,
        resolution: FileConflictResolution,
    ) {
        let t_res = Instant::now();
        match resolution {
            FileConflictResolution::Skip => {
                tracing::info!(
                    media_id = media_id,
                    file = %LogObscurer::filename(&conflict.source.filename),
                    elapsed_us = t_res.elapsed().as_micros(),
                    "[BatchTransfer: Resolution] Action: SKIP. File left untouched in source directory."
                );
                self.summary.skipped_count += 1;
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            FileConflictResolution::AutoRename => {
                let auto_name = FileOperations::generate_unique_filename(
                    &self.target_folder_path,
                    &conflict.source.filename,
                );
                tracing::info!(
                    media_id = media_id,
                    transfer_kind = ?self.transfer_kind,
                    original = %LogObscurer::filename(&conflict.source.filename),
                    auto_rename = %LogObscurer::filename(&auto_name),
                    "[BatchTransfer: Resolution] Action: AUTO-RENAME. Transferring file with generated unique suffix..."
                );
                let exec_res = match self.transfer_kind {
                    FileTransferKind::Move => self.library.file_operations().move_single_media(
                        media_id,
                        self.target_folder_id,
                        Some(&auto_name),
                        false,
                    ),
                    FileTransferKind::Copy => self.library.file_operations().copy_single_media(
                        media_id,
                        self.target_folder_id,
                        Some(&auto_name),
                        false,
                    ),
                };

                match exec_res {
                    Ok(_) => {
                        self.summary.auto_renamed_count += 1;
                        self.summary.moved_count += 1;
                        if self.transfer_kind == FileTransferKind::Move {
                            self.moved_media_ids.insert(media_id);
                        }
                        tracing::info!(
                            media_id = media_id,
                            resolved_name = %LogObscurer::filename(&auto_name),
                            elapsed_ms = t_res.elapsed().as_millis(),
                            "[BatchTransfer: Resolution] Successfully transferred and auto-renamed file"
                        );
                    }
                    Err(e) => {
                        tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: AutoRename Error] Failed to transfer auto-renamed file");
                        self.summary.failed_count += 1;
                    }
                }
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            FileConflictResolution::Overwrite => {
                tracing::info!(
                    media_id = media_id,
                    transfer_kind = ?self.transfer_kind,
                    target_file = %LogObscurer::filename(&conflict.destination.filename),
                    "[BatchTransfer: Resolution] Action: OVERWRITE. Overwriting conflicting destination file..."
                );
                let exec_res = match self.transfer_kind {
                    FileTransferKind::Move => self.library.file_operations().move_single_media(
                        media_id,
                        self.target_folder_id,
                        None,
                        true,
                    ),
                    FileTransferKind::Copy => self.library.file_operations().copy_single_media(
                        media_id,
                        self.target_folder_id,
                        None,
                        true,
                    ),
                };

                match exec_res {
                    Ok(_) => {
                        self.summary.overwritten_count += 1;
                        self.summary.moved_count += 1;
                        if self.transfer_kind == FileTransferKind::Move {
                            self.moved_media_ids.insert(media_id);
                        }
                        tracing::info!(
                            media_id = media_id,
                            elapsed_ms = t_res.elapsed().as_millis(),
                            "[BatchTransfer: Resolution] Successfully overwritten destination file and updated LMDB"
                        );
                    }
                    Err(e) => {
                        tracing::error!(media_id = media_id, error = %e, "[BatchTransfer: Overwrite Error] Failed to overwrite destination file");
                        self.summary.failed_count += 1;
                    }
                }
                self.current_index += 1;
                Timer::single_shot(std::time::Duration::from_millis(1), move || self.step());
            }
            FileConflictResolution::Cancel => {
                tracing::info!(
                    processed = self.current_index,
                    total = self.items.len(),
                    "[BatchTransfer: Resolution] Action: CANCEL. Batch operation cancelled by user. Aborting remaining items."
                );
                self.finish();
            }
        }
    }

    fn resolve_media_path(&self, media_id: MediaId) -> PathBuf {
        let st = self.state.read();
        if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == media_id) {
            return item.full_path.clone();
        }
        drop(st);

        self.library
            .resolve_media_path(media_id)
            .unwrap_or_else(|_| PathBuf::from(format!("Media_{}", media_id)))
    }

    fn finish(self) {
        let elapsed_ms = self.start_time.elapsed().as_millis();

        // For Move: Remove only successfully moved IDs from active file selection
        // For Copy: Retain selection of source files
        {
            let mut st = self.state.write();
            st.invalidate_folder_tree_cache();
            if self.transfer_kind == FileTransferKind::Move {
                st.file_selection.deselect_many(&self.moved_media_ids);
            }
        }

        BridgeFolders::reload_tree(&self.window, &self.library, &self.settings, &self.state);
        BridgeMedia::reload(
            &self.window,
            &self.library,
            &self.thumbnails,
            &self.settings,
            &self.state,
            false,
            true,
        );
        BridgeInspector::update(&self.window, &self.state);
        BridgeTags::sync_tag_selection(&self.window, &self.library, &self.state);

        let verb_past = if self.transfer_kind == FileTransferKind::Move { "Moved" } else { "Copied" };

        let mut details = Vec::new();
        if self.summary.auto_renamed_count > 0 {
            details.push(format!("{} auto-renamed", self.summary.auto_renamed_count));
        }
        if self.summary.overwritten_count > 0 {
            details.push(format!("{} overwritten", self.summary.overwritten_count));
        }
        if self.summary.skipped_count > 0 {
            details.push(format!("{} skipped", self.summary.skipped_count));
        }
        if self.summary.failed_count > 0 {
            details.push(format!("{} failed", self.summary.failed_count));
        }

        let detail_str = if details.is_empty() {
            String::new()
        } else {
            format!(" ({})", details.join(", "))
        };

        let msg = format!(
            "{} {} file(s) to '{}' in {}ms{}",
            verb_past, self.summary.moved_count, self.target_folder_name, elapsed_ms, detail_str
        );

        tracing::info!(
            action = verb_past,
            total_requested = self.summary.total_requested,
            transferred = self.summary.moved_count,
            renamed = self.summary.auto_renamed_count,
            overwritten = self.summary.overwritten_count,
            skipped = self.summary.skipped_count,
            failed = self.summary.failed_count,
            target_folder = %LogObscurer::folder(Some(self.target_folder_id), &self.target_folder_path),
            total_batch_time_ms = elapsed_ms,
            "[Benchmark: BatchTransfer] Complete batch operation finished and committed to LMDB storage"
        );

        self.window.set_status_text(msg.into());
    }
}
