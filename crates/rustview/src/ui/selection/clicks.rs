//! Clicks in the Browse view: select, extend, double and middle click.

use super::{BridgeFileSelection, DoubleClickKind, FileClickContext, FileClickEvent, LastClickRecord};
use crate::system::SystemFileHandler;
use crate::ui::MainWindow;
use crate::ui::browse_folders::BrowseItem;
use crate::ui::file_dialogs::missing::{MissingFileDialogController, MissingFileItem};
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::multiview::handle::{self as multiview_handle, Pace};
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tags::BridgeTags;
use crate::viewer::ViewerTabManager;
use parking_lot::{Mutex, RwLock};
use rustview_core::library::Library;
use rustview_core::library::models::MediaId;
use rustview_core::media::MediaType;
use rustview_core::settings::{LogObscurer, SettingsManager};
use slint::ComponentHandle;
use std::sync::Arc;
use std::time::{Duration, Instant};

impl BridgeFileSelection {
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

        let delta = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            st.browse_click(BrowseItem::Media(media_id), false, false)
        };
        Self::sync_selection_delta(window, state, &delta);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        fullscreen.dismiss();
        if !fullscreen.open(settings, state, window.as_weak()) {
            StatusBarBridge::warn(
                window,
                "This file cannot be shown in fullscreen (deleted, pending or corrupted).",
            );
        }
    }

    /// Handles media tile clicks with modifier keys, double-click timing, the live Multiview follow, and surgical UI updates.
    pub fn handle_click(event: FileClickEvent, ctx: FileClickContext<'_>) {
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
        let (delta, sel_count) = Self::mutate_selection(event, ctx.state);
        let changed_ids = &delta.media;
        let t_math = t_start.elapsed();

        // 3. Fast In-Place Slint Model Mutation with Viewport Translation
        let t_sync_start = Instant::now();
        Self::sync_selection_delta(ctx.window, ctx.state, &delta);
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
                .is_some_and(|m| m.is_file() && m.len() > 0);

            if !file_exists_and_ready {
                tracing::info!(
                    media_id,
                    filename = %LogObscurer::filename(&filename),
                    "[FileSelection: DoubleClickPending] Double-click on pending media stub; file not yet ready on disk"
                );
                StatusBarBridge::info(
                    window,
                    format!("'{filename}' is currently in the external render queue and not yet ready on disk."),
                );
                return;
            }
        }

        let open_images_in_tab = settings.get().viewer.images.open_in_tab;
        let open_videos_in_tab = settings.get().viewer.videos.open_in_tab;

        let should_open_in_tab = (is_image && open_images_in_tab) || (is_video && open_videos_in_tab);

        if should_open_in_tab {
            tracing::info!(
                media_id,
                is_image,
                is_video,
                "[FileSelection: DoubleClick] Opening media in foreground viewer tab"
            );
            BridgeViewerTabs::open_media_tab(media_id, window, tab_manager, library, settings, state, fullscreen);
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
                .is_some_and(|m| m.is_file() && m.len() > 0);

            if !file_exists_and_ready {
                tracing::info!(
                    media_id,
                    filename = %LogObscurer::filename(&filename),
                    "[FileSelection: CtrlDoubleClickPending] Ctrl+Double-click on pending media stub; file not yet ready on disk"
                );
                StatusBarBridge::info(
                    window,
                    format!("'{filename}' is currently in the external render queue and not yet ready on disk."),
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

        let should_open_in_tab = (is_image && open_images_in_tab) || (is_video && open_videos_in_tab);

        if should_open_in_tab {
            tracing::info!(
                media_id,
                is_image,
                is_video,
                "[FileSelection: CtrlDoubleClick] Opening media in background viewer tab (Ctrl+DoubleClick, selection reverted)"
            );
            BridgeViewerTabs::open_media_tab_background(media_id, window, tab_manager, library, settings, state);
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

    pub fn open_media(media_id: MediaId, state: &Arc<RwLock<UiState>>, library: &Arc<Library>, window: &MainWindow) {
        let full_path_opt = {
            let st = state.read();
            st.all_folder_media
                .iter()
                .find(|m| m.record.id == media_id)
                .map(|m| (m.record.filename.clone(), m.full_path(), m.record.is_corrupted))
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
                        .is_some_and(|m| m.is_corrupted);
                    (name, p, corrupt)
                }
                Err(e) => {
                    tracing::error!(
                        media_id = media_id,
                        error = %e,
                        "Failed to resolve media path from database"
                    );
                    StatusBarBridge::error(window, format!("Media record not found in database: #{media_id}"));
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
            StatusBarBridge::error(window, format!("Failed to open file: {e}"));
        } else {
            let status_msg = if is_corrupted {
                format!("Opened file '{filename}' (Flagged: Corrupted) with system application.")
            } else {
                format!("Opened '{filename}' with system default application.")
            };
            StatusBarBridge::success(window, status_msg);
        }
    }
}
