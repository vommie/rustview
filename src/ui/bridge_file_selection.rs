use super::MainWindow;
use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::bridge_image_fullscreen::ImageFullscreenDialogController;
use super::bridge_inspector::BridgeInspector;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::file_selection::FileSelectionModel;
use crate::library::Library;
use crate::library::models::MediaId;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::system::SystemFileHandler;
use crate::viewer::ViewerTabManager;
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle, Model};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Parameters representing a media click event with modifier keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileClickEvent {
    pub media_id: MediaId,
    pub is_ctrl: bool,
    pub is_shift: bool,
}

/// Context parameter bundle for handling media file click events.
pub struct FileClickContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub settings: &'a Arc<SettingsManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub last_click: &'a Arc<Mutex<Option<LastClickRecord>>>,
    pub fullscreen_controller: &'a ImageFullscreenDialogController,
}

/// Discriminant representing the detected double click type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleClickKind {
    None,
    Normal,
    Ctrl,
}

/// Record capturing timestamp and modifier state of the previous click for double-click evaluation.
#[derive(Debug, Clone, Copy)]
pub struct LastClickRecord {
    pub media_id: MediaId,
    pub instant: Instant,
    pub is_ctrl: bool,
}

/// Subsystem managing media file selection calculations, fast in-place model mutations, clicks, and quick fullscreen triggers.
pub struct BridgeFileSelection;

impl BridgeFileSelection {
    /// Registers media click, middle click, background click, and selection callbacks onto MainWindow.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        settings: Arc<SettingsManager>,
        tab_manager: Arc<ViewerTabManager>,
        state: Arc<RwLock<UiState>>,
        last_click: Arc<Mutex<Option<LastClickRecord>>>,
        fullscreen_controller: ImageFullscreenDialogController,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings.clone();
        let tm = tab_manager.clone();
        let st = state.clone();
        let lc = last_click;
        let fs = fullscreen_controller.clone();
        window.on_media_clicked(move |id, is_ctrl, is_shift| {
            Self::handle_click(
                FileClickEvent {
                    media_id: id as u64,
                    is_ctrl,
                    is_shift,
                },
                FileClickContext {
                    window: &win,
                    library: &lib,
                    settings: &set,
                    tab_manager: &tm,
                    state: &st,
                    last_click: &lc,
                    fullscreen_controller: &fs,
                },
            );
        });

        let win_mid = window.clone_strong();
        let lib_mid = library.clone();
        let set_mid = settings;
        let st_mid = state.clone();
        let tm_mid = tab_manager;
        let fs_mid = fullscreen_controller.clone();
        window.on_media_middle_clicked(move |id| {
            Self::handle_middle_click(
                id as u64, &win_mid, &lib_mid, &set_mid, &st_mid, &tm_mid, &fs_mid,
            );
        });

        let win_bg = window.clone_strong();
        let lib_bg = library;
        let st_bg = state;
        let fs_bg = fullscreen_controller;
        window.on_background_clicked(move || {
            tracing::info!(
                "[FileSelection: BackgroundClick] Background free space clicked, clearing active file selection"
            );
            fs_bg.dismiss_if_quick_fullscreen();
            Self::clear_selection(&win_bg, &lib_bg, &st_bg);
        });
    }

    /// Handles middle-click on a media tile or table row in the browse view:
    /// selects the item and opens it in Quick-Fullscreen Mode (non-modal) so Inspector and Tag & Rate Panel stay in sync.
    pub fn handle_middle_click(
        media_id: MediaId,
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        tab_manager: &Arc<ViewerTabManager>,
        fullscreen_controller: &ImageFullscreenDialogController,
    ) {
        tracing::info!(
            media_id,
            "[FileSelection: MiddleClick] Quick-Fullscreen mode triggered directly on media item (bypassing tabs, non-modal)"
        );

        // Select the media item so Inspector and Tag & Rate Panel are immediately synchronized
        let (changed_ids, _) = {
            let mut st = state.write();
            let prev_selected = st.file_selection.selected_ids().clone();
            st.file_selection.select_single(media_id);
            let newly_selected = st.file_selection.selected_ids();
            let changed = FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected);
            (changed, st.file_selection.count())
        };
        Self::sync_selection_diff(window, state, &changed_ids);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        fullscreen_controller.show_for_media(
            media_id,
            library,
            settings,
            state,
            tab_manager,
            window.as_weak(),
        );
    }

    /// Handles media tile clicks with modifier keys, double-click timing, live Quick-Fullscreen synchronization, and surgical UI updates.
    pub fn handle_click(
        event: FileClickEvent,
        ctx: FileClickContext<'_>,
    ) {
        let t_start = Instant::now();

        // If Quick-Fullscreen is currently active from Browse view, dynamically update its preview to the clicked media item!
        if ctx.fullscreen_controller.is_quick_fullscreen_active() {
            ctx.fullscreen_controller.update_quick_fullscreen_media(event.media_id, ctx.library, ctx.settings);
        }

        // 1. Double Click Detection (< 400ms delta on same ID)
        match Self::check_double_click(event, ctx.last_click) {
            DoubleClickKind::Normal => {
                Self::handle_double_click(
                    event.media_id,
                    ctx.state,
                    ctx.library,
                    ctx.settings,
                    ctx.tab_manager,
                    ctx.window,
                );
                return;
            }
            DoubleClickKind::Ctrl => {
                Self::handle_ctrl_double_click(
                    event.media_id,
                    ctx.state,
                    ctx.library,
                    ctx.settings,
                    ctx.tab_manager,
                    ctx.window,
                );
                return;
            }
            DoubleClickKind::None => {}
        }

        // 2. Selection Mutation Math (Strictly scoped to active visible items)
        let (changed_ids, sel_count) = Self::mutate_selection(event, ctx.state);
        let t_math = t_start.elapsed();

        // 3. Fast In-Place Slint Model Mutation with Viewport Translation
        let t_sync_start = Instant::now();
        Self::sync_selection_diff(ctx.window, ctx.state, &changed_ids);
        let t_sync = t_sync_start.elapsed();

        // 4. Update Inspector Fields, Synchronize TagTree Highlight/Counts, and Update Status Bar
        let t_insp_start = Instant::now();
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeTags::sync_tag_selection(ctx.window, ctx.library, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        let t_insp = t_insp_start.elapsed();

        tracing::debug!(
            target_media_id = event.media_id,
            is_ctrl = event.is_ctrl,
            is_shift = event.is_shift,
            selected_count = sel_count,
            changed_rows = changed_ids.len(),
            quick_fullscreen_active = ctx.fullscreen_controller.is_quick_fullscreen_active(),
            math_us = t_math.as_micros(),
            model_sync_us = t_sync.as_micros(),
            inspector_and_tags_us = t_insp.as_micros(),
            total_click_us = t_start.elapsed().as_micros(),
            "[FileSelection: FastInPlace] Instant selection updated with tag synchronization, status bar, and live quick fullscreen"
        );
    }

    /// Evaluates if successive clicks qualify as a double-click (< 400ms delta), distinguishing between normal and Ctrl-modified double clicks.
    pub fn check_double_click(
        event: FileClickEvent,
        last_click: &Arc<Mutex<Option<LastClickRecord>>>,
    ) -> DoubleClickKind {
        let now = Instant::now();
        let mut click_guard = last_click.lock();
        if let Some(last) = *click_guard
            && last.media_id == event.media_id
            && !event.is_shift
        {
            let elapsed = now.duration_since(last.instant);
            if elapsed < Duration::from_millis(400) {
                *click_guard = None;
                if event.is_ctrl && last.is_ctrl {
                    return DoubleClickKind::Ctrl;
                } else if !event.is_ctrl && !last.is_ctrl {
                    return DoubleClickKind::Normal;
                }
            }
        }
        *click_guard = Some(LastClickRecord {
            media_id: event.media_id,
            instant: now,
            is_ctrl: event.is_ctrl,
        });
        DoubleClickKind::None
    }

    fn mutate_selection(
        event: FileClickEvent,
        state: &Arc<RwLock<UiState>>,
    ) -> (Vec<MediaId>, usize) {
        let mut st = state.write();
        let prev_selected = st.file_selection.selected_ids().clone();

        if event.is_shift {
            let visible_ids: Vec<MediaId> = st
                .active_indices
                .iter()
                .filter_map(|&idx| st.all_folder_media.get(idx))
                .map(|item| item.record.id)
                .collect();

            let additive = event.is_ctrl;
            st.file_selection
                .select_range(event.media_id, &visible_ids, additive);
        } else if event.is_ctrl {
            st.file_selection.toggle(event.media_id);
        } else {
            st.file_selection.select_single(event.media_id);
        }

        let newly_selected = st.file_selection.selected_ids();
        let changed = FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected);
        let count = st.file_selection.count();
        (changed, count)
    }

    /// Surgically modifies only changed rows in the Slint Model translating global indices to local viewport offsets.
    pub fn sync_selection_diff(
        window: &MainWindow,
        state: &RwLock<UiState>,
        changed_ids: &[MediaId],
    ) {
        if changed_ids.is_empty() {
            return;
        }

        let media_model = window.get_media_items();
        let start_window = window.get_visible_start_index() as usize;
        let visible_count = media_model.row_count();
        let st = state.read();

        for &id in changed_ids {
            if let Some(&global_row_idx) = st.media_row_map.get(&id)
                && global_row_idx >= start_window
                && global_row_idx < start_window + visible_count
            {
                let local_idx = global_row_idx - start_window;
                if let Some(mut item) = media_model.row_data(local_idx)
                    && item.id == id as i32
                {
                    let is_selected = st.file_selection.is_selected(id);
                    if item.is_selected != is_selected {
                        item.is_selected = is_selected;
                        media_model.set_row_data(local_idx, item);
                    }
                }
            }
        }
    }

    pub fn select_all(window: &MainWindow, library: &Arc<Library>, state: &Arc<RwLock<UiState>>) {
        let t_start = Instant::now();
        let changed_ids = {
            let mut st = state.write();
            let prev_selected = st.file_selection.selected_ids().clone();
            let visible_ids: Vec<MediaId> = st
                .active_indices
                .iter()
                .filter_map(|&idx| st.all_folder_media.get(idx))
                .map(|m| m.record.id)
                .collect();
            st.file_selection.select_all(&visible_ids);
            let newly_selected = st.file_selection.selected_ids();
            FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected)
        };

        Self::sync_selection_diff(window, state, &changed_ids);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        tracing::debug!(
            changed_rows = changed_ids.len(),
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: SelectAll] Fast in-place select all executed"
        );
    }

    pub fn clear_selection(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t_start = Instant::now();
        let prev_selected = {
            let mut st = state.write();
            let prev = st.file_selection.selected_ids().clone();
            st.file_selection.clear();
            prev
        };

        let changed_ids: Vec<MediaId> = prev_selected.into_iter().collect();
        Self::sync_selection_diff(window, state, &changed_ids);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        tracing::debug!(
            cleared_rows = changed_ids.len(),
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: ClearSelection] Fast in-place clear selection executed"
        );
    }

    /// Handles standard double-click on a media tile: opens images and videos in foreground Viewer Tabs if enabled in Settings.
    pub fn handle_double_click(
        media_id: MediaId,
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        tab_manager: &Arc<ViewerTabManager>,
        window: &MainWindow,
    ) {
        let (is_image, is_video) = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == media_id) {
                (
                    matches!(item.record.media_type, MediaType::Image),
                    matches!(item.record.media_type, MediaType::Video),
                )
            } else if let Ok(Some(record)) = library.media().get(media_id) {
                (
                    matches!(record.media_type, MediaType::Image),
                    matches!(record.media_type, MediaType::Video),
                )
            } else {
                (false, false)
            }
        };

        let open_images_in_tab = settings.get().viewer.images.open_in_tab;
        let open_videos_in_tab = settings.get().viewer.videos.open_in_tab;

        let should_open_in_tab =
            (is_image && open_images_in_tab) || (is_video && open_videos_in_tab);

        if should_open_in_tab {
            tracing::info!(
                media_id,
                is_image,
                is_video,
                "[FileSelection: DoubleClick] Opening media in foreground viewer tab"
            );
            BridgeViewerTabs::open_media_tab(
                media_id,
                window,
                tab_manager,
                library,
                settings,
                state,
            );
        } else {
            tracing::info!(
                media_id,
                is_image,
                is_video,
                open_images_in_tab,
                open_videos_in_tab,
                "[FileSelection: DoubleClick] Delegating media opening to OS default application"
            );
            Self::open_media(media_id, state, library, window);
        }
    }

    /// Handles Ctrl + double-click on a media tile: restores original selection state (compensating rollback)
    /// and opens images/videos in a background viewer tab without stealing focus or altering the selection.
    pub fn handle_ctrl_double_click(
        media_id: MediaId,
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        tab_manager: &Arc<ViewerTabManager>,
        window: &MainWindow,
    ) {
        let (is_image, is_video) = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == media_id) {
                (
                    matches!(item.record.media_type, MediaType::Image),
                    matches!(item.record.media_type, MediaType::Video),
                )
            } else if let Ok(Some(record)) = library.media().get(media_id) {
                (
                    matches!(record.media_type, MediaType::Image),
                    matches!(record.media_type, MediaType::Video),
                )
            } else {
                (false, false)
            }
        };

        // 1. Revert/rollback the selection toggle done by the first click of the double-click sequence
        {
            let mut st = state.write();
            st.file_selection.toggle(media_id);
        }
        Self::sync_selection_diff(window, state, &[media_id]);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        let open_images_in_tab = settings.get().viewer.images.open_in_tab;
        let open_videos_in_tab = settings.get().viewer.videos.open_in_tab;

        let should_open_in_tab =
            (is_image && open_images_in_tab) || (is_video && open_videos_in_tab);

        if should_open_in_tab {
            tracing::info!(
                media_id,
                is_image,
                is_video,
                "[FileSelection: CtrlDoubleClick] Opening media in background viewer tab (Ctrl+DoubleClick, selection reverted)"
            );
            BridgeViewerTabs::open_media_tab_background(
                media_id,
                window,
                tab_manager,
                library,
                settings,
                state,
            );
        } else {
            tracing::info!(
                media_id,
                is_image,
                is_video,
                open_images_in_tab,
                open_videos_in_tab,
                "[FileSelection: CtrlDoubleClick] Settings delegate to OS default application"
            );
            Self::open_media(media_id, state, library, window);
        }
    }

    pub fn open_media(
        media_id: MediaId,
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        window: &MainWindow,
    ) {
        let full_path_opt = {
            let st = state.read();
            st.all_folder_media
                .iter()
                .find(|m| m.record.id == media_id)
                .map(|m| {
                    (
                        m.record.filename.clone(),
                        m.full_path.clone(),
                        m.record.is_corrupted,
                    )
                })
        };

        let (filename, full_path, is_corrupted) = match full_path_opt {
            Some(tuple) => tuple,
            None => match library.resolve_media_path(media_id) {
                Ok(p) => {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let corrupt = library
                        .media()
                        .get(media_id)
                        .ok()
                        .flatten()
                        .map(|m| m.is_corrupted)
                        .unwrap_or(false);
                    (name, p, corrupt)
                }
                Err(e) => {
                    tracing::error!(
                        media_id = media_id,
                        error = %e,
                        "Failed to resolve media path from database"
                    );
                    StatusBarBridge::error(
                        window,
                        format!("Media record not found in database: #{}", media_id),
                    );
                    return;
                }
            },
        };

        if !full_path.exists() {
            tracing::warn!(
                media_id = media_id,
                path = %LogObscurer::path(&full_path),
                "Cannot open media: File not found on physical disk"
            );
            let missing_dlg = MissingFileDialogController::new();
            missing_dlg.show(
                window,
                "Open Media",
                vec![MissingFileItem {
                    filename,
                    path: full_path,
                }],
            );
            return;
        }

        if is_corrupted {
            tracing::warn!(
                media = %LogObscurer::media(Some(media_id), &filename),
                path = %LogObscurer::path(&full_path),
                "Opening file flagged as corrupted. Attempting launch with OS default application..."
            );
        }

        tracing::info!(
            media = %LogObscurer::media(Some(media_id), &filename),
            file = %LogObscurer::path(&full_path),
            "Opening media file with Linux system default handler..."
        );

        if let Err(e) = SystemFileHandler::open_file(&full_path) {
            tracing::error!(
                file = %LogObscurer::path(&full_path),
                error = %e,
                "Failed to launch system default application for media file"
            );
            StatusBarBridge::error(window, format!("Failed to open file: {}", e));
        } else {
            let status_msg = if is_corrupted {
                format!(
                    "Opened file '{}' (Flagged: Corrupted) with system application.",
                    filename
                )
            } else {
                format!("Opened '{}' with system default application.", filename)
            };
            StatusBarBridge::success(window, status_msg);
        }
    }
}
