use super::MainWindow;
use super::bridge_file_dialog_missing::{MissingFileDialogController, MissingFileItem};
use super::multiview::MultiviewHandle;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::{BridgeMedia, MediaReloadOptions};
use super::bridge_state::{LastActivePanel, UiState};
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::browse_folders::BrowseFolders;
use super::browse_window::update_tile;
use super::file_selection::FileSelectionModel;
use super::multiview::handle::{self as multiview_handle, Pace};
use crate::library::Library;
use crate::library::models::MediaId;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::system::SystemFileHandler;
use crate::thumbnails::ThumbnailManager;
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
    pub fullscreen: &'a MultiviewHandle,
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

/// Subsystem managing media file selection calculations, fast in-place model mutations, clicks, and the triggers that drive the fullscreen Multiview.
pub struct BridgeFileSelection;

impl BridgeFileSelection {
    /// Registers media click, middle click, background click, and global selection hotkey callbacks onto MainWindow.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        settings: Arc<SettingsManager>,
        tab_manager: Arc<ViewerTabManager>,
        state: Arc<RwLock<UiState>>,
        last_click: Arc<Mutex<Option<LastClickRecord>>>,
        fullscreen: MultiviewHandle,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings.clone();
        let tm = tab_manager.clone();
        let st = state.clone();
        let lc = last_click;
        let fs = fullscreen.clone();
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
                    fullscreen: &fs,
                },
            );
        });

        let win_mid = window.clone_strong();
        let lib_mid = library.clone();
        let set_mid = settings;
        let st_mid = state.clone();
        let fs_mid = fullscreen;
        window.on_media_middle_clicked(move |id| {
            Self::handle_middle_click(id as u64, &win_mid, &lib_mid, &set_mid, &st_mid, &fs_mid);
        });

        let win_bg = window.clone_strong();
        let lib_bg = library.clone();
        let st_bg = state.clone();
        window.on_background_clicked(move || {
            tracing::info!(
                "[FileSelection: BackgroundClick] Background free space clicked, clearing active file selection"
            );
            st_bg.write().last_active_panel = LastActivePanel::Media;
            Self::clear_selection(&win_bg, &lib_bg, &st_bg);
        });

        // Global Select All (Ctrl+A) Callback in Browse View
        let win_sel_all = window.clone_strong();
        let lib_sel_all = library.clone();
        let st_sel_all = state.clone();
        window.on_select_all_media(move || {
            tracing::info!("[FileSelection: Shortcut] Ctrl+A shortcut received in Browse View: selecting all media");
            Self::select_all(&win_sel_all, &lib_sel_all, &st_sel_all);
        });

        // Global Clear Selection (Ctrl+Shift+A / Esc) Callback in Browse View
        let win_clr_sel = window.clone_strong();
        let lib_clr_sel = library;
        let st_clr_sel = state;
        window.on_clear_media_selection(move || {
            tracing::info!("[FileSelection: Shortcut] Ctrl+Shift+A / Esc shortcut received in Browse View: clearing selection");
            Self::clear_selection(&win_clr_sel, &lib_clr_sel, &st_clr_sel);
        });
    }

    /// Handles middle-click on a media tile or table row in the browse view: selects the item and shows it in the
    /// fullscreen (the Multiview). If a fullscreen that follows the selection is open already, it follows the new
    /// selection instead. The main window stays usable, so the Inspector and Tag & Rate Panel stay in sync.
    pub fn handle_middle_click(
        media_id: MediaId,
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        fullscreen: &MultiviewHandle,
    ) {
        tracing::info!(
            media_id,
            "[FileSelection: MiddleClick] Fullscreen requested directly on a media item (non-modal)"
        );

        let (changed_ids, _) = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
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

        fullscreen.dismiss();
        if !fullscreen
            .open(settings, state, window.as_weak())
        {
            StatusBarBridge::warn(
                window,
                "This file cannot be shown in fullscreen (deleted, pending or corrupted).",
            );
        }
    }

    /// Programmatically selects a single media item in Browse Mode, synchronizing UI tiles,
    /// viewport windowing/slicing, Inspector, Tag & Rate panel, and status bar.
    pub fn select_and_reveal_media(
        media_id: MediaId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: Option<&Arc<ThumbnailManager>>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t_start = Instant::now();

        let (changed_ids, target_visual_idx_opt) = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let prev_selected = st.file_selection.selected_ids().clone();
            st.file_selection.select_single(media_id);
            let newly_selected = st.file_selection.selected_ids();
            let changed = FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected);

            let visual_idx = st.row_of(media_id).map(|row| row + st.folder_front_len());

            (changed, visual_idx)
        };

        if let Some(target_idx) = target_visual_idx_opt {
            let need_slice_reanchor = !state.read().ring.range().contains(&target_idx);

            if need_slice_reanchor {
                if let Some(thumb_mgr) = thumbnails {
                    BridgeMedia::reload_with_options(
                        window,
                        library,
                        thumb_mgr,
                        settings,
                        state,
                        MediaReloadOptions::with_anchor(false, false, target_idx),
                    );
                } else if !changed_ids.is_empty() {
                    Self::sync_selection_diff(window, state, &changed_ids);
                }
            } else if !changed_ids.is_empty() {
                Self::sync_selection_diff(window, state, &changed_ids);
            }

            window.invoke_scroll_media_item_into_view(target_idx as i32);
        } else if !changed_ids.is_empty() {
            Self::sync_selection_diff(window, state, &changed_ids);
        }

        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
        multiview_handle::on_user_selection_changed(Pace::Immediate);

        tracing::info!(
            media_id,
            visual_index = ?target_visual_idx_opt,
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: SelectAndReveal] Media item selected and revealed in Browse Mode"
        );
    }

    /// Handles media tile clicks with modifier keys, double-click timing, the live Multiview follow, and surgical UI updates.
    pub fn handle_click(
        event: FileClickEvent,
        ctx: FileClickContext<'_>,
    ) {
        let t_start = Instant::now();

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
                    ctx.fullscreen,
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

        // 2. Selection Mutation Math
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
        multiview_handle::on_user_selection_changed(Pace::Immediate);
        let t_insp = t_insp_start.elapsed();

        tracing::debug!(
            target_media_id = event.media_id,
            is_ctrl = event.is_ctrl,
            is_shift = event.is_shift,
            selected_count = sel_count,
            changed_rows = changed_ids.len(),
            fullscreen_open = ctx.fullscreen.is_open(),
            math_us = t_math.as_micros(),
            model_sync_us = t_sync.as_micros(),
            inspector_and_tags_us = t_insp.as_micros(),
            total_click_us = t_start.elapsed().as_micros(),
            "[FileSelection: FastInPlace] Instant selection updated with tag synchronization, status bar, and live fullscreen follow"
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
        st.last_active_panel = LastActivePanel::Media;
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
    /// Employs an O(Viewport) threshold optimization when changed_ids is large (e.g. Select All / Deselect All)
    /// to eliminate CPU freezes on large datasets.
    pub fn sync_selection_diff(
        window: &MainWindow,
        state: &RwLock<UiState>,
        changed_ids: &[MediaId],
    ) {
        if changed_ids.is_empty() {
            return;
        }
        // A medium was (de)selected: a highlighted folder row gives way.
        BrowseFolders::clear_folder_selection(window, state);

        let media_model = window.get_media_items();
        let visible_count = media_model.row_count();
        let st = state.read();

        // O(Viewport) Optimization:
        // If changed_ids contains many elements (Select All or Clear Selection across 1k-100k items),
        // iterating directly over the rows of the tile pool (~60 to ~160 rows) is orders of magnitude
        // faster than running tens of thousands of hashmap lookups.
        if changed_ids.len() > visible_count * 2 {
            for slot in 0..visible_count {
                if let Some(mut item) = media_model.row_data(slot) {
                    let is_selected = st.file_selection.is_selected(item.id as u64);
                    if item.is_selected != is_selected {
                        item.is_selected = is_selected;
                        media_model.set_row_data(slot, item);
                    }
                }
            }
            return;
        }

        for &id in changed_ids {
            let is_selected = st.file_selection.is_selected(id);
            update_tile(window, &st, id, |item| item.is_selected = is_selected);
        }
    }

    /// Selects all visible media items in the current Browse View (Ctrl+A).
    pub fn select_all(window: &MainWindow, library: &Arc<Library>, state: &Arc<RwLock<UiState>>) {
        let t_start = Instant::now();
        let (changed_ids, total_count) = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let prev_selected = st.file_selection.selected_ids().clone();

            let visible_ids: Vec<MediaId> = if !st.active_indices.is_empty() {
                st.active_indices
                    .iter()
                    .filter_map(|&idx| st.all_folder_media.get(idx))
                    .map(|m| m.record.id)
                    .collect()
            } else if st.is_query_active || !st.filter.is_empty() {
                // Active filter/query returned zero results; do not select inactive items
                Vec::new()
            } else {
                st.all_folder_media.iter().map(|m| m.record.id).collect()
            };

            st.file_selection.select_all(&visible_ids);
            let newly_selected = st.file_selection.selected_ids();
            let diff = FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected);
            let count = st.file_selection.count();
            (diff, count)
        };

        Self::sync_selection_diff(window, state, &changed_ids);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
        multiview_handle::on_user_selection_changed(Pace::Immediate);

        if total_count > 0 {
            StatusBarBridge::info(window, format!("Selected all {} media file(s).", total_count));
        } else {
            StatusBarBridge::info(window, "No media files to select.");
        }

        tracing::info!(
            selected_count = total_count,
            changed_rows = changed_ids.len(),
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: SelectAll] Selected all visible media items in Browse view"
        );
    }

    /// Deselects one media item (for example because the Multiview removed it) and brings the tiles, Inspector, Tag & Rate
    /// panel and status bar in line. Does nothing if the item is not selected.
    pub fn deselect_media(
        media_id: MediaId,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        {
            let mut st = state.write();
            if !st.file_selection.is_selected(media_id) {
                return;
            }
            st.file_selection
                .deselect_many(&std::collections::HashSet::from([media_id]));
        }
        Self::sync_selection_diff(window, state, &[media_id]);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
        tracing::info!(media_id, "[FileSelection: Deselect] Media item deselected");
    }

    /// Deselects all media items in the current Browse View (Ctrl+Shift+A / Esc).
    pub fn clear_selection(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t_start = Instant::now();
        BrowseFolders::clear_folder_selection(window, state);
        let (changed_ids, prev_count) = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let prev = st.file_selection.selected_ids().clone();
            let prev_count = prev.len();
            st.file_selection.clear();
            (prev.into_iter().collect::<Vec<MediaId>>(), prev_count)
        };

        Self::sync_selection_diff(window, state, &changed_ids);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        if prev_count > 0 {
            StatusBarBridge::info(window, format!("Deselected {} media file(s).", prev_count));
        }

        tracing::info!(
            cleared_count = prev_count,
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: ClearSelection] Cleared media selection in Browse view"
        );
    }

    /// Handles standard double-click on a media tile: opens images and videos in foreground Viewer Tabs if enabled in Settings.
    /// Resilient: Pending stubs that have not finished rendering display an informative message instead of throwing an error.
    pub fn handle_double_click(
        media_id: MediaId,
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        tab_manager: &Arc<ViewerTabManager>,
        window: &MainWindow,
        fullscreen: &MultiviewHandle,
    ) {
        let (is_image, is_video, is_pending, filename) = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.find(media_id) {
                (
                    matches!(item.record.media_type, MediaType::Image),
                    matches!(item.record.media_type, MediaType::Video),
                    item.is_pending,
                    item.record.filename.clone(),
                )
            } else if let Ok(Some(record)) = library.media().get(media_id) {
                (
                    matches!(record.media_type, MediaType::Image),
                    matches!(record.media_type, MediaType::Video),
                    record.source.is_some() && (record.file_size == 0 || record.width == 0),
                    record.filename,
                )
            } else {
                (false, false, false, String::new())
            }
        };

        if is_pending {
            let file_exists_and_ready = library
                .resolve_media_path(media_id)
                .ok()
                .and_then(|p| std::fs::metadata(&p).ok())
                .map(|m| m.is_file() && m.len() > 0)
                .unwrap_or(false);

            if !file_exists_and_ready {
                tracing::info!(
                    media_id,
                    filename = %LogObscurer::filename(&filename),
                    "[FileSelection: DoubleClickPending] Double-click on pending media stub; file not yet ready on disk"
                );
                StatusBarBridge::info(
                    window,
                    format!("'{}' is currently in the external render queue and not yet ready on disk.", filename),
                );
                return;
            }
        }

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
                fullscreen,
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

    /// Handles Ctrl + double-click on a media tile: restores original selection state and opens in a background viewer tab.
    /// Resilient: Pending stubs that have not finished rendering display an informative message instead of throwing an error.
    pub fn handle_ctrl_double_click(
        media_id: MediaId,
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        tab_manager: &Arc<ViewerTabManager>,
        window: &MainWindow,
    ) {
        let (is_image, is_video, is_pending, filename) = {
            let st = state.read();
            if let Some(item) = st.all_folder_media.find(media_id) {
                (
                    matches!(item.record.media_type, MediaType::Image),
                    matches!(item.record.media_type, MediaType::Video),
                    item.is_pending,
                    item.record.filename.clone(),
                )
            } else if let Ok(Some(record)) = library.media().get(media_id) {
                (
                    matches!(record.media_type, MediaType::Image),
                    matches!(record.media_type, MediaType::Video),
                    record.source.is_some() && (record.file_size == 0 || record.width == 0),
                    record.filename,
                )
            } else {
                (false, false, false, String::new())
            }
        };

        if is_pending {
            let file_exists_and_ready = library
                .resolve_media_path(media_id)
                .ok()
                .and_then(|p| std::fs::metadata(&p).ok())
                .map(|m| m.is_file() && m.len() > 0)
                .unwrap_or(false);

            if !file_exists_and_ready {
                tracing::info!(
                    media_id,
                    filename = %LogObscurer::filename(&filename),
                    "[FileSelection: CtrlDoubleClickPending] Ctrl+Double-click on pending media stub; file not yet ready on disk"
                );
                StatusBarBridge::info(
                    window,
                    format!("'{}' is currently in the external render queue and not yet ready on disk.", filename),
                );
                return;
            }
        }

        {
            let mut st = state.write();
            st.file_selection.toggle(media_id);
        }
        Self::sync_selection_diff(window, state, &[media_id]);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
        multiview_handle::on_user_selection_changed(Pace::Immediate);

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
                        m.full_path(),
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
