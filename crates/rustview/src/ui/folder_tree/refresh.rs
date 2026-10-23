//! Refreshing the folder tree and its subtrees against the drive (explicit user actions only).

use super::{BridgeFolders, FolderNavigationContext};
use crate::ui::MainWindow;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::folders::{FolderScanOptions, normalize_path};
use rustview_core::library::models::{FolderId, MediaId, MediaRecord};
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

impl BridgeFolders {
    /// Full root scan: scans the complete directory tree starting from configured root path on disk,
    /// persists all relational records, and updates the in-memory `SPoT` snapshot.
    /// MANUAL ONLY: triggered by the toolbar refresh menu or folder context menu.
    pub fn refresh_folder_tree(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if BridgeMedia::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(
                window,
                "Cannot refresh folder tree: another operation is already in progress.",
            );
            return;
        }

        let app_settings = settings.get();
        let root_setting = app_settings.general.root_folder.clone();
        let Some(ref root_dir) = root_setting else {
            StatusBarBridge::warn(window, "Cannot refresh folders: No root directory configured.");
            return;
        };

        let normalized_root = normalize_path(root_dir);
        if !normalized_root.is_dir() {
            StatusBarBridge::error(window, "Configured root directory not found on filesystem.");
            return;
        }

        // Ensure the snapshot built after the scan is scoped to the root being scanned.
        library.set_folder_tree_root(Some(normalized_root.clone()));

        let scan_options = FolderScanOptions::from_general(&app_settings.general);

        {
            let mut st = state.write();
            st.is_refreshing_folders = true;
            st.folder_scan_count = 0;
        }
        window.set_is_system_busy(true);
        StatusBarBridge::update_folder_scan_progress(window, true, 0);
        StatusBarBridge::info(window, "Scanning entire directory tree on filesystem...");

        tracing::info!(
            root = %LogObscurer::path(&normalized_root),
            ignore_nested_mounts = scan_options.ignore_nested_mounts,
            "[FolderRefresh: TreeFull] Starting manual full folder tree scan"
        );

        let t_start = Instant::now();
        let w_weak = window.as_weak();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();

        std::thread::Builder::new()
            .name("folder-tree-scanner".into())
            .spawn(move || {
                let w_weak_progress = w_weak.clone();
                let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));

                let snapshot_res = lib.folders().scan_directory_tree(
                    &normalized_root,
                    scan_options,
                    Some(move |count| {
                        let mut last = last_progress_report.lock();
                        if last.elapsed().as_millis() >= 35 {
                            *last = Instant::now();
                            let _ = w_weak_progress.upgrade_in_event_loop(move |w| {
                                StatusBarBridge::update_folder_scan_progress(&w, true, count);
                            });
                        }
                    }),
                );

                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    {
                        let mut s = st.write();
                        s.is_refreshing_folders = false;
                    }
                    StatusBarBridge::update_folder_scan_progress(&w, false, 0);
                    w.set_is_system_busy(BridgeMedia::is_system_busy(&w, &thumb, &st));

                    match snapshot_res {
                        Ok(snapshot) => {
                            let elapsed_ms = t_start.elapsed().as_millis();
                            let total_dirs = snapshot.nodes.len();
                            st.write().set_folder_hierarchy(snapshot);

                            tracing::info!(
                                total_dirs,
                                elapsed_ms = elapsed_ms as u64,
                                "[FolderRefresh: TreeApplied] Applied rebuilt SPoT snapshot to UI state"
                            );

                            StatusBarBridge::success(
                                &w,
                                format!("Tree scan completed in {elapsed_ms}ms: {total_dirs} total dirs updated."),
                            );

                            let has_current_folder = st.read().current_folder_id.is_some();
                            if has_current_folder {
                                Self::reload_tree(&w, &lib, &set, &st);
                                Self::scroll_to_selected(&w);
                            } else {
                                // First scan after an empty/unscanned tree: navigate to the (now known) root.
                                Self::sync_root(&w, &lib, &thumb, &set, &st);
                            }
                            BridgeMedia::update_virtual_folder_button_state(&w, &lib, &st);
                            StatusBarBridge::update_metrics(&w, &st);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "[FolderTreeScanError] Failed to scan folder tree");
                            StatusBarBridge::error(&w, format!("Folder tree scan failed: {e}"));
                        }
                    }
                });
            })
            .expect("Failed to spawn folder tree scanner thread");
    }

    pub fn refresh_selected_folder_subtree(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (current_id_opt, current_path_opt) = {
            let st = state.read();
            (st.current_folder_id, st.current_folder_path.clone())
        };

        if let (Some(fid), Some(path)) = (current_id_opt, current_path_opt) {
            Self::refresh_folder_subtree(fid, path, window, library, thumbnails, settings, state);
        } else {
            Self::refresh_folder_tree(window, library, thumbnails, settings, state);
        }
    }

    /// Surgical targeted scan: Scans ONLY the selected subtree physically on disk,
    /// reconciles counts and records, rolls deltas up through ancestors in LMDB, and updates the in-memory `SPoT` snapshot.
    /// MANUAL ONLY: triggered by the toolbar refresh menu or folder context menu.
    pub fn refresh_folder_subtree(
        folder_id: FolderId,
        path: PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if BridgeMedia::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(
                window,
                "Cannot refresh folder: another operation is already in progress.",
            );
            return;
        }

        let folder_name = path.file_name().map_or_else(
            || path.to_string_lossy().to_string(),
            |n| n.to_string_lossy().to_string(),
        );

        let scan_options = FolderScanOptions::from_general(&settings.get().general);

        {
            let mut st = state.write();
            st.is_refreshing_folders = true;
            st.folder_scan_count = 0;
        }
        window.set_is_system_busy(true);
        StatusBarBridge::update_folder_scan_progress(window, true, 0);
        StatusBarBridge::info(
            window,
            format!("Surgically rescanning subtree under '{folder_name}'..."),
        );

        tracing::info!(
            folder_id,
            path = %LogObscurer::path(&path),
            ignore_nested_mounts = scan_options.ignore_nested_mounts,
            "[FolderRefresh: TreeRelative] Starting manual subtree scan"
        );

        let t_start = Instant::now();
        let w_weak = window.as_weak();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();

        std::thread::Builder::new()
            .name(format!("folder-subtree-scanner-{folder_id}"))
            .spawn(move || {
                let w_weak_progress = w_weak.clone();
                let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));

                let snapshot_res = lib.folders().scan_subtree(
                    folder_id,
                    scan_options,
                    Some(move |count| {
                        let mut last = last_progress_report.lock();
                        if last.elapsed().as_millis() >= 35 {
                            *last = Instant::now();
                            let _ = w_weak_progress.upgrade_in_event_loop(move |w| {
                                StatusBarBridge::update_folder_scan_progress(&w, true, count);
                            });
                        }
                    }),
                );

                let _ = w_weak.upgrade_in_event_loop(move |w| {
                    {
                        let mut s = st.write();
                        s.is_refreshing_folders = false;
                    }
                    StatusBarBridge::update_folder_scan_progress(&w, false, 0);
                    w.set_is_system_busy(BridgeMedia::is_system_busy(&w, &thumb, &st));

                    match snapshot_res {
                        Ok(snapshot) => {
                            let elapsed_ms = t_start.elapsed().as_millis();
                            tracing::info!(
                                folder_id,
                                total_nodes = snapshot.nodes.len(),
                                elapsed_ms = elapsed_ms as u64,
                                "[FolderRefresh: SubtreeApplied] Applied rebuilt SPoT snapshot to UI state"
                            );
                            st.write().set_folder_hierarchy(snapshot);

                            StatusBarBridge::success(
                                &w,
                                format!("Subtree '{folder_name}' rescanned in {elapsed_ms}ms."),
                            );

                            Self::reload_tree(&w, &lib, &set, &st);
                            Self::scroll_to_selected(&w);
                            BridgeMedia::update_virtual_folder_button_state(&w, &lib, &st);
                            StatusBarBridge::update_metrics(&w, &st);
                        }
                        Err(e) => {
                            tracing::error!(folder_id, error = %e, "[SubtreeScanError] Failed to scan subtree");
                            StatusBarBridge::error(&w, format!("Subtree scan failed: {e}"));
                        }
                    }
                });
            })
            .expect("Failed to spawn folder subtree scanner thread");
    }

    pub fn refresh_folder_explicit(
        folder_id: FolderId,
        path: PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if BridgeMedia::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(window, "Cannot refresh: another operation is already in progress.");
            return;
        }

        Self::select_folder_internal(
            folder_id,
            path,
            FolderNavigationContext {
                window,
                library,
                thumbnails,
                settings,
                state,
            },
            true,
        );
    }

    pub fn refresh_folder_metadata_only(
        folder_id: FolderId,
        path: PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if BridgeMedia::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(window, "Cannot refresh: another operation is already in progress.");
            return;
        }

        let normalized_path = normalize_path(&path);
        let folder_name = normalized_path.file_name().map_or_else(
            || normalized_path.to_string_lossy().to_string(),
            |n| n.to_string_lossy().to_string(),
        );

        let raw_media = library.media().get_all_by_folder(folder_id).unwrap_or_default();
        if raw_media.is_empty() {
            StatusBarBridge::info(
                window,
                format!("No media files in folder '{folder_name}' to refresh metadata."),
            );
            return;
        }

        let targets: Vec<(MediaId, PathBuf, MediaRecord)> = raw_media
            .into_iter()
            .map(|rec| {
                let full_p = normalized_path.join(&rec.filename);
                (rec.id, full_p, rec)
            })
            .collect();

        BridgeMedia::execute_metadata_only_refresh(
            targets,
            window,
            library,
            thumbnails,
            state,
            &format!("folder '{folder_name}'"),
        );
    }
}
