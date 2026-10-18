use super::bridge_favorites::BridgeFavorites;
use super::bridge_folder_tree_sync::FolderTreeSync;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::{BridgeMedia, MediaReloadOptions, PendingList};
use super::browse_folders::BrowseFolders;
use super::bridge_state::{LastActivePanel, UiState};
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::list_loader;
use super::virtual_folder_wording;
use super::models::rgb8_to_shared_buffer;
use super::MainWindow;
use crate::icons::Icon;
use crate::library::folders::{normalize_path, FolderScanOptions};
use crate::library::models::{FolderHierarchySnapshot, FolderId, MediaId, MediaRecord};
use crate::library::refresh::RefreshReport;
use crate::library::Library;
use crate::query::{IndexedMediaRecord, MediaStore, lay_out};
use crate::settings::{GridAspectRatio, LogObscurer, SettingsDialogController, SettingsManager};
use crate::telemetry::FolderLoadBenchmark;
use crate::thumbnails::{ThumbnailConfig, ThumbnailManager};
use parking_lot::RwLock;
use slint::{ComponentHandle, Model, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub struct FolderNavigationContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
}

/// What a folder selection carries from the click to the moment its list is shown.
struct FolderSelection {
    folder_id: FolderId,
    normalized_path: PathBuf,
    folder_name: String,
    is_virtual_folder: bool,
    virtual_count: usize,
    target_folder_ids: Vec<FolderId>,
    cancel_flag: Arc<AtomicBool>,
    bench: Arc<FolderLoadBenchmark>,
    is_explicit_refresh: bool,
    request_token: usize,
}

struct LoadedFolder {
    indexed: Vec<IndexedMediaRecord>,
    is_empty_in_db: bool,
    /// The records tagged with an external source application: the only ones the external stub completion has to look at.
    stub_candidates: Vec<MediaRecord>,
}

pub struct BridgeFolders;

impl BridgeFolders {
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
        settings_dialog: Rc<RefCell<SettingsDialogController>>,
    ) {
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();

        window.on_refresh_folder(move || {
            if BridgeMedia::is_system_busy(&win, &thumb, &st) {
                StatusBarBridge::warn(&win, "Cannot refresh: another operation is already in progress.");
                return;
            }

            let (folder_id_opt, path_opt) = {
                let s = st.read();
                (s.current_folder_id, s.current_folder_path.clone())
            };
            if let (Some(id), Some(path)) = (folder_id_opt, path_opt) {
                tracing::info!(
                    folder_id = id,
                    path = %LogObscurer::path(&path),
                    "[FolderRefresh: Toolbar] Executing explicit full media refresh."
                );
                Self::refresh_folder_explicit(id, path, &win, &lib, &thumb, &set, &st);
            }
        });

        let dialog = settings_dialog;
        window.on_open_settings(move || {
            dialog.borrow_mut().show();
        });

        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_selected(move |id, path| {
            st.write().last_active_panel = LastActivePanel::Folder;
            Self::select_folder(
                id as u64,
                PathBuf::from(path.as_str()),
                &win,
                &lib,
                &thumb,
                &set,
                &st,
            );
        });

        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_toggle_expand(move |id| {
            st.write().last_active_panel = LastActivePanel::Folder;
            Self::toggle_expand(id as u64, &win, &lib, &set, &st);
        });

        let lib_all = library.clone();
        let thumb_all = thumbnails.clone();
        let set_all = settings.clone();
        let st_all = state.clone();
        let win_all = window.clone_strong();
        window.on_folder_refresh_all_clicked(move || {
            st_all.write().last_active_panel = LastActivePanel::Folder;
            if BridgeMedia::is_system_busy(&win_all, &thumb_all, &st_all) {
                StatusBarBridge::warn(&win_all, "Cannot refresh folder tree: another operation is already in progress.");
                return;
            }
            tracing::info!("[FolderRefresh: Manual] Toolbar 'Refresh Tree (Full)' triggered by user");
            Self::refresh_folder_tree(&win_all, &lib_all, &thumb_all, &set_all, &st_all);
        });

        let lib_rel = library.clone();
        let thumb_rel = thumbnails.clone();
        let set_rel = settings.clone();
        let st_rel = state.clone();
        let win_rel = window.clone_strong();
        window.on_folder_refresh_relative_clicked(move || {
            st_rel.write().last_active_panel = LastActivePanel::Folder;
            if BridgeMedia::is_system_busy(&win_rel, &thumb_rel, &st_rel) {
                StatusBarBridge::warn(&win_rel, "Cannot refresh folder tree: another operation is already in progress.");
                return;
            }
            tracing::info!("[FolderRefresh: Manual] Toolbar 'Refresh Tree (Relative)' triggered by user");
            Self::refresh_selected_folder_subtree(&win_rel, &lib_rel, &thumb_rel, &set_rel, &st_rel);
        });

        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_collapse_all_clicked(move || {
            st.write().last_active_panel = LastActivePanel::Folder;
            Self::collapse_all_except_active(&win, &lib, &set, &st);
        });

        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        let lib = library;
        window.on_toggle_hidden_folders(move |show_hidden| {
            st.write().last_active_panel = LastActivePanel::Folder;
            let mut app_settings = set.get();
            app_settings.folders.show_hidden_folders = show_hidden;
            set.save_quiet_async(&app_settings);
            Self::reload_tree(&win, &lib, &set, &st);
            Self::scroll_to_selected(&win);
        });
    }

    pub fn scroll_to_selected(window: &MainWindow) {
        let model = window.get_folders();
        let count = model.row_count();
        for idx in 0..count {
            if let Some(row) = model.row_data(idx)
                && row.is_selected
            {
                window.invoke_scroll_folder_to_row(idx as i32);
                break;
            }
        }
    }

    /// Returns the cached SPoT snapshot or lazily loads it from the database (zero filesystem access).
    /// A failed load is logged and yields an empty snapshot that is NOT cached, so the next call retries.
    pub fn ensure_snapshot(
        library: &Library,
        state: &RwLock<UiState>,
    ) -> Arc<FolderHierarchySnapshot> {
        let existing = state.read().folder_hierarchy.clone();
        if let Some(snap) = existing {
            return snap;
        }

        let t0 = Instant::now();
        match library.folders().load_hierarchy_snapshot() {
            Ok(loaded) => {
                let arc_snap = Arc::new(loaded);
                state.write().folder_hierarchy = Some(arc_snap.clone());
                tracing::debug!(
                    nodes = arc_snap.nodes.len(),
                    roots = arc_snap.root_ids.len(),
                    elapsed_us = t0.elapsed().as_micros() as u64,
                    "[FolderTree: SnapshotLazyLoad] Loaded SPoT snapshot into UI state"
                );
                arc_snap
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "[FolderTree: SnapshotLoadFailed] Failed to load folder hierarchy snapshot. Showing empty tree."
                );
                Arc::new(FolderHierarchySnapshot::default())
            }
        }
    }

    /// Collects the ids of all ancestors of `target` that lie at or below `root`, using the snapshot path index.
    fn collect_ancestor_ids(
        snapshot: &FolderHierarchySnapshot,
        root: &Path,
        target: &Path,
    ) -> Vec<FolderId> {
        let mut ids = Vec::new();
        let mut current = target.parent();

        while let Some(parent_p) = current {
            if !parent_p.starts_with(root) {
                break;
            }
            if let Some(&id) = snapshot.path_index.get(parent_p) {
                ids.push(id);
            }
            if parent_p == root {
                break;
            }
            current = parent_p.parent();
        }

        ids
    }

    /// Resolves the remembered last folder to a node of the snapshot (None if unknown or missing on disk).
    fn resolve_remembered_folder(
        snapshot: &FolderHierarchySnapshot,
        last_path: &Path,
    ) -> Option<(FolderId, PathBuf)> {
        if !last_path.is_dir() {
            return None;
        }
        let normalized = normalize_path(last_path);
        snapshot
            .path_index
            .get(&normalized)
            .map(|&id| (id, normalized.clone()))
    }

    pub fn collapse_all_except_active(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let target_path_opt = state.read().current_folder_path.clone();
        let snapshot = Self::ensure_snapshot(library, state);

        let mut keep_expanded_ids: HashSet<FolderId> = HashSet::new();

        if let Some(&root_id) = snapshot.root_ids.first() {
            keep_expanded_ids.insert(root_id);

            if let (Some(root_node), Some(target_p)) =
                (snapshot.get_node(root_id), target_path_opt.as_ref())
            {
                keep_expanded_ids.extend(Self::collect_ancestor_ids(
                    &snapshot,
                    &root_node.path,
                    target_p,
                ));
            }
        }

        {
            let mut st = state.write();
            st.expanded_folder_ids.retain(|id| keep_expanded_ids.contains(id));
            for id in &keep_expanded_ids {
                st.expanded_folder_ids.insert(*id);
            }
            Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
        }

        Self::reload_tree(window, library, settings, state);
        Self::scroll_to_selected(window);
    }

    pub fn sync_root(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let s = settings.get();
        window.set_thumb_size(s.appearance.default_thumbnail_size as i32);
        let ratio = GridAspectRatio::from_str_name(&s.appearance.thumbnail_aspect_ratio);
        window.set_thumb_aspect_idx(ratio.to_index());
        window.set_show_hidden_folders(s.folders.show_hidden_folders);

        let Some(ref root_path) = s.general.root_folder else {
            tracing::info!("[FolderTree: NoRoot] No root directory configured. Showing empty folder tree.");
            library.set_folder_tree_root(None);
            Self::clear_view(
                window,
                state,
                "No Root Directory configured. Open Settings to select a root folder.",
            );
            return;
        };

        let normalized_root = normalize_path(root_path);
        if !normalized_root.is_dir() {
            tracing::warn!(
                root = %LogObscurer::path(&normalized_root),
                "[FolderTree: RootMissing] Configured root directory does not exist on disk. Showing empty folder tree."
            );
            library.set_folder_tree_root(None);
            Self::clear_view(
                window,
                state,
                "Configured Root Directory does not exist on disk.",
            );
            return;
        }

        // Scope the SPoT snapshot to the configured root and rebuild it from the database only (no filesystem scan).
        library.set_folder_tree_root(Some(normalized_root.clone()));
        state.write().invalidate_folder_hierarchy();
        let snapshot = Self::ensure_snapshot(library, state);

        let Some(&root_id) = snapshot.root_ids.first() else {
            tracing::info!(
                root = %LogObscurer::path(&normalized_root),
                "[FolderTree: Unscanned] Root directory is not in the folder database yet. Showing empty folder tree until a manual refresh."
            );
            Self::clear_view_with_empty_state(
                window,
                state,
                "Folder tree has not been scanned yet. Use Refresh > Refresh Tree (Full) in the Folders toolbar.",
                "Folder Tree Not Scanned",
                Icon::Refresh,
                "Folder tree not scanned yet",
            );
            return;
        };

        let target_folder = if s.general.remember_last_folder {
            s.general
                .last_folder_path
                .as_deref()
                .and_then(|p| Self::resolve_remembered_folder(&snapshot, p))
        } else {
            None
        }
        .unwrap_or_else(|| (root_id, normalized_root.clone()));

        {
            let mut st = state.write();
            if s.folders.auto_reveal_remembered_folder {
                st.expanded_folder_ids.insert(root_id);
                for id in Self::collect_ancestor_ids(&snapshot, &normalized_root, &target_folder.1) {
                    st.expanded_folder_ids.insert(id);
                }
            } else if st.expanded_folder_ids.is_empty() {
                st.expanded_folder_ids.insert(root_id);
            }

            Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
        }

        Self::select_folder(
            target_folder.0,
            target_folder.1,
            window,
            library,
            thumbnails,
            settings,
            state,
        );

        let win_weak_scroll = window.as_weak();
        Timer::single_shot(std::time::Duration::from_millis(60), move || {
            if let Some(w) = win_weak_scroll.upgrade() {
                Self::scroll_to_selected(&w);
            }
        });
    }

    /// Full root scan: scans the complete directory tree starting from configured root path on disk,
    /// persists all relational records, and updates the in-memory SPoT snapshot.
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
                                format!("Tree scan completed in {}ms: {} total dirs updated.", elapsed_ms, total_dirs),
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
                            StatusBarBridge::error(&w, format!("Folder tree scan failed: {}", e));
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
    /// reconciles counts and records, rolls deltas up through ancestors in LMDB, and updates the in-memory SPoT snapshot.
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

        let folder_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

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
            format!("Surgically rescanning subtree under '{}'...", folder_name),
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
            .name(format!("folder-subtree-scanner-{}", folder_id))
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
                                format!("Subtree '{}' rescanned in {}ms.", folder_name, elapsed_ms),
                            );

                            Self::reload_tree(&w, &lib, &set, &st);
                            Self::scroll_to_selected(&w);
                            BridgeMedia::update_virtual_folder_button_state(&w, &lib, &st);
                            StatusBarBridge::update_metrics(&w, &st);
                        }
                        Err(e) => {
                            tracing::error!(folder_id, error = %e, "[SubtreeScanError] Failed to scan subtree");
                            StatusBarBridge::error(&w, format!("Subtree scan failed: {}", e));
                        }
                    }
                });
            })
            .expect("Failed to spawn folder subtree scanner thread");
    }

    pub fn select_folder(
        folder_id: FolderId,
        path: PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        {
            let mut st = state.write();
            let from = st.current_folder_id;
            st.nav_history.record(from, folder_id);
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
            false,
        );
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
            StatusBarBridge::warn(
                window,
                "Cannot refresh: another operation is already in progress.",
            );
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
            StatusBarBridge::warn(
                window,
                "Cannot refresh: another operation is already in progress.",
            );
            return;
        }

        let normalized_path = normalize_path(&path);
        let folder_name = normalized_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| normalized_path.to_string_lossy().to_string());

        let raw_media = library.media().get_all_by_folder(folder_id).unwrap_or_default();
        if raw_media.is_empty() {
            StatusBarBridge::info(
                window,
                format!("No media files in folder '{}' to refresh metadata.", folder_name),
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
            &format!("folder '{}'", folder_name),
        );
    }

    fn select_folder_internal(
        folder_id: FolderId,
        path: PathBuf,
        ctx: FolderNavigationContext<'_>,
        is_explicit_refresh: bool,
    ) {
        let normalized_path = normalize_path(&path);
        let folder_name = normalized_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| normalized_path.to_string_lossy().to_string());

        ctx.thumbnails.cancel_pipeline();
        if !is_explicit_refresh {
            ctx.window.invoke_scroll_media_to_top();
        }

        let bench = Arc::new(FolderLoadBenchmark::new(
            folder_id,
            normalized_path.to_string_lossy().to_string(),
        ));

        let (request_token, cancel_flag, is_virtual_folder) = {
            let mut st = ctx.state.write();

            if let Some(prev_flag) = st.active_folder_cancel_flag.take() {
                prev_flag.store(true, Ordering::Release);
            }
            let new_flag = Arc::new(AtomicBool::new(false));
            st.active_folder_cancel_flag = Some(new_flag.clone());

            st.active_folder_bench = Some(bench.clone());
            st.active_folder_token = st.active_folder_token.wrapping_add(1);
            st.current_folder_id = Some(folder_id);
            st.current_folder_path = Some(normalized_path.clone());
            st.active_indices.clear();
            st.mark_list_bytes_dirty();
            st.all_folder_media.clear();
            // Opening a folder again puts its subfolders in name order and drops the highlight of a folder row.
            st.folder_rows.reset_tail();
            st.folder_rows.clear_selection();

            if !is_explicit_refresh {
                st.file_selection.clear();
                st.last_executed_query.clear();
                st.is_query_active = false;
                st.selected_rating = 0;
                st.filter.rating = None;
                st.filter.search_query.clear();
                st.filter.tag_ids.clear();
                st.tag_filter_query.clear();
                st.is_virtual_folder_mode = false;
                st.virtual_folder_count = 0;
            }

            (st.active_folder_token, new_flag, st.is_virtual_folder_mode)
        };

        {
            let mut s = ctx.settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(normalized_path.clone());
                ctx.settings.save_quiet_async(&s);
            }
        }

        if !is_explicit_refresh {
            ctx.window.set_search_query("".into());
            ctx.window.set_selected_rating(0);
            ctx.window.set_tag_filter_text("".into());
        }

        Self::reload_tree(ctx.window, ctx.library, ctx.settings, ctx.state);
        Self::scroll_to_selected(ctx.window);

        BridgeFavorites::set_selected_path(Some(&normalized_path));

        let target_folder_ids = if is_virtual_folder {
            ctx.library.folders().get_descendant_folder_ids(folder_id).unwrap_or_else(|_| vec![folder_id])
        } else {
            vec![folder_id]
        };

        let virtual_count = target_folder_ids.len();
        {
            let mut st = ctx.state.write();
            st.virtual_folder_count = virtual_count;
        }
        StatusBarBridge::update_virtual_folder(ctx.window, is_virtual_folder, virtual_count);

        let selection = FolderSelection {
            folder_id,
            normalized_path,
            folder_name,
            is_virtual_folder,
            virtual_count,
            target_folder_ids,
            cancel_flag,
            bench,
            is_explicit_refresh,
            request_token,
        };

        let media_ids = Self::fetch_media_ids(ctx.library, &selection);
        if media_ids.len() > list_loader::ASYNC_ABOVE_RECORDS {
            Self::load_folder_deferred(&ctx, selection, media_ids);
            return;
        }

        let loaded = Self::load_folder_records(ctx.library, &selection, media_ids);
        let (is_empty_in_db, stub_candidates) = (loaded.is_empty_in_db, loaded.stub_candidates);
        {
            let mut s = ctx.state.write();
            s.all_folder_media.replace(loaded.indexed);
        }

        BridgeMedia::reload(ctx.window, ctx.library, ctx.thumbnails, ctx.settings, ctx.state, false, false);
        Self::after_folder_list_ready(&ctx, selection, is_empty_in_db, stub_candidates);
    }

    /// The ids of the media of the selected folder(s); a cheap index read, it decides whether the list is built on the worker.
    fn fetch_media_ids(library: &Library, selection: &FolderSelection) -> Vec<MediaId> {
        let t_id_start = Instant::now();
        let mut media_ids = Vec::new();
        if let Ok(rtxn) = library.raw_db().env.read_txn() {
            for fid in &selection.target_folder_ids {
                if let Ok(Some(ids)) = library.raw_db().media_by_folder.get(&rtxn, fid) {
                    media_ids.extend(ids);
                }
            }
        }
        selection.bench.lmdb_id_fetch_time_us.store(t_id_start.elapsed().as_micros() as u64, Ordering::Relaxed);
        media_ids
    }

    /// Reads the records of `media_ids` and indexes them for the view (no UI access, safe on any thread).
    fn load_folder_records(library: &Library, selection: &FolderSelection, media_ids: Vec<MediaId>) -> LoadedFolder {
        let bench = &selection.bench;
        let t_rec_start = Instant::now();
        let mut known_media = Vec::with_capacity(media_ids.len());
        let mut seen_ids = HashSet::with_capacity(media_ids.len());

        if let Ok(rtxn) = library.raw_db().env.read_txn() {
            for id in media_ids {
                if seen_ids.insert(id)
                    && let Ok(Some(rec)) = library.raw_db().media.get(&rtxn, &id)
                {
                    known_media.push(rec);
                }
            }
        }
        bench.lmdb_record_fetch_time_us.store(t_rec_start.elapsed().as_micros() as u64, Ordering::Relaxed);
        bench.total_records_fetched.store(known_media.len(), Ordering::Relaxed);

        let is_empty_in_db = known_media.is_empty();
        let stub_candidates: Vec<MediaRecord> = known_media.iter().filter(|r| r.source.is_some()).cloned().collect();
        let t_pre = Instant::now();
        let indexed = if selection.is_virtual_folder {
            BridgeMedia::preindex_records(library, known_media)
        } else {
            BridgeMedia::preindex(library, known_media, &selection.normalized_path)
        };
        bench.preindex_time_us.store(t_pre.elapsed().as_micros() as u64, Ordering::Relaxed);
        LoadedFolder { indexed, is_empty_in_db, stub_candidates }
    }

    /// A large folder: the view shows an empty list while a worker thread loads, filters and sorts the records; the UI thread only
    /// swaps the finished list in. A newer folder selection supersedes this one.
    fn load_folder_deferred(ctx: &FolderNavigationContext<'_>, selection: FolderSelection, media_ids: Vec<MediaId>) {
        let job = list_loader::begin_folder_load();
        tracing::info!(
            job,
            folder_id = selection.folder_id,
            records = media_ids.len(),
            "[ListLoader: FolderLoad] Large folder; loading it on the worker thread"
        );
        BridgeMedia::show_empty_list(ctx.window, ctx.thumbnails, ctx.state);
        StatusBarBridge::info(ctx.window, format!("Loading {} items...", media_ids.len()));
        let started = Instant::now();

        let weak = ctx.window.as_weak();
        let (library, thumbnails, settings, state) = (ctx.library.clone(), ctx.thumbnails.clone(), ctx.settings.clone(), ctx.state.clone());
        list_loader::worker().submit(move || {
            if !list_loader::folder_load_is_current(job) || selection.cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let loaded = Self::load_folder_records(&library, &selection, media_ids);
            let (is_empty_in_db, stub_candidates) = (loaded.is_empty_in_db, loaded.stub_candidates);
            let store = MediaStore::from_records(loaded.indexed);
            if !list_loader::folder_load_is_current(job) || selection.cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let (mut request, query) = BridgeMedia::capture_request(&state);
            request.refine_from = None;
            let layout = lay_out(&library, &store, &request);
            let worker_ms = started.elapsed().as_secs_f64() * 1000.0;

            let _ = weak.upgrade_in_event_loop(move |window| {
                let still_selected = list_loader::folder_load_is_current(job) && state.read().active_folder_token == selection.request_token;
                if !still_selected {
                    tracing::debug!(job, "[ListLoader: FolderLoad] Superseded by a newer selection; the result is dropped");
                    return;
                }
                let records = store.len();
                let t_commit = Instant::now();
                let pending = PendingList { store: Some(store), layout, query };
                BridgeMedia::commit_layout(&window, &library, &thumbnails, &settings, &state, pending, MediaReloadOptions::new(false, false));
                tracing::info!(
                    job,
                    records,
                    worker_ms = format!("{worker_ms:.1}"),
                    commit_ms = format!("{:.1}", t_commit.elapsed().as_secs_f64() * 1000.0),
                    total_ms = format!("{:.1}", started.elapsed().as_secs_f64() * 1000.0),
                    "[ListLoader: FolderLoad] Folder list ready"
                );
                let ctx = FolderNavigationContext { window: &window, library: &library, thumbnails: &thumbnails, settings: &settings, state: &state };
                Self::after_folder_list_ready(&ctx, selection, is_empty_in_db, stub_candidates);
            });
        });
    }

    /// What follows once the list of the selected folder is shown: inspector and status, then, when the folder is new to the library
    /// or the user asked for a refresh, the scan of the drive (a refresh is the only thing that reads the folder from disk).
    fn after_folder_list_ready(
        ctx: &FolderNavigationContext<'_>,
        selection: FolderSelection,
        is_empty_in_db: bool,
        stub_candidates: Vec<MediaRecord>,
    ) {
        let FolderSelection {
            folder_id,
            normalized_path,
            folder_name,
            is_virtual_folder,
            virtual_count,
            cancel_flag,
            bench,
            is_explicit_refresh,
            request_token,
            ..
        } = selection;

        BrowseFolders::apply_pending_first_select(ctx.window, ctx.state, folder_id);
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeTags::sync_tag_selection(ctx.window, ctx.library, ctx.state);
        BridgeMedia::update_virtual_folder_button_state(ctx.window, ctx.library, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        Self::scroll_to_selected(ctx.window);

        let should_scan_and_generate = is_explicit_refresh || is_empty_in_db;
        if !should_scan_and_generate {
            if ctx.settings.get().api.auto_complete_external_stubs {
                let (ready_stubs, still_pending, sources) = ctx
                    .library
                    .external_media_completer()
                    .find_pending_stubs_in_records(&stub_candidates);

                if !ready_stubs.is_empty() {
                    let stubs_count = ready_stubs.len();
                    tracing::info!(
                        folder_id = folder_id,
                        stubs_count,
                        sources = ?sources,
                        still_pending,
                        "[FolderNavigation: StubsDetected] Auto-completing external stubs on folder access..."
                    );

                    {
                        let mut st = ctx.state.write();
                        st.is_refreshing_media = true;
                    }
                    ctx.window.set_is_system_busy(true);

                    ctx.thumbnails.tracker().set_generate_total(stubs_count);
                    StatusBarBridge::info(ctx.window, format!("Auto-completing {} external file(s)...", stubs_count));

                    let w_weak = ctx.window.as_weak();
                    let lib_arc = ctx.library.clone();
                    let thumb_arc = ctx.thumbnails.clone();
                    let set_arc = ctx.settings.clone();
                    let st_arc = ctx.state.clone();
                    let config = ThumbnailConfig::from_performance_settings(&ctx.settings.get().performance);

                    std::thread::Builder::new()
                        .name(format!("folder-stubs-completer-{}", folder_id))
                        .spawn(move || {
                            let w_weak_progress = w_weak.clone();
                            let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));
                            let tracker_ref = thumb_arc.tracker();

                            let progress_cb = Arc::new(move |cur: usize, tot: usize| {
                                tracker_ref.increment_generate_done();
                                let snap = tracker_ref.snapshot();
                                let mut last = last_progress_report.lock();
                                if last.elapsed().as_millis() >= 35 || cur == tot {
                                    *last = Instant::now();
                                    let _ = w_weak_progress.upgrade_in_event_loop(move |w| {
                                        StatusBarBridge::update_thumbnail_progress(&w, &snap);
                                    });
                                }
                            });

                            let store = thumb_arc.store();
                            let (report, completed_items) = lib_arc
                                .external_media_completer()
                                .complete_candidates(
                                    ready_stubs,
                                    &config,
                                    store,
                                    Some(move |cur, tot| progress_cb(cur, tot)),
                                );

                            let _ = w_weak.upgrade_in_event_loop(move |w| {
                                {
                                    let mut st = st_arc.write();
                                    st.is_refreshing_media = false;

                                    for item in &completed_items {
                                        if item.primary_width > 0 && item.primary_height > 0 && !item.primary_pixels.is_empty() {
                                            let shared_buf = rgb8_to_shared_buffer(
                                                &item.primary_pixels,
                                                item.primary_width,
                                                item.primary_height,
                                            );
                                            st.thumbs.insert(item.media_id, shared_buf);
                                        }

                                        st.mark_list_bytes_dirty();

                                        if let Some(target_indexed) = st.all_folder_media.find_mut(item.media_id) {
                                            target_indexed.set_record(item.updated_record.clone());
                                        }
                                    }
                                }

                                thumb_arc.tracker().reset();
                                BridgeMedia::reload(&w, &lib_arc, &thumb_arc, &set_arc, &st_arc, false, false);
                                BridgeInspector::update(&w, &st_arc);
                                BridgeTags::sync_tag_selection(&w, &lib_arc, &st_arc);
                                let busy = BridgeMedia::is_system_busy(&w, &thumb_arc, &st_arc);
                                w.set_is_system_busy(busy);
                                StatusBarBridge::update_metrics(&w, &st_arc);
                                StatusBarBridge::success(&w, format!("Auto-completed {} external media item(s).", report.completed_count));
                            });
                        })
                        .ok();
                    return;
                }
            }

            StatusBarBridge::update_media_index_progress(ctx.window, false, 0, 0);
            StatusBarBridge::info(ctx.window, "Ready.");
            return;
        }

        StatusBarBridge::update_media_index_progress(ctx.window, true, 0, 0);
        StatusBarBridge::info(
            ctx.window,
            if is_virtual_folder {
                format!("Scanning media files in '{}' (Recursive: {} folders)...", folder_name, virtual_count)
            } else {
                format!("Scanning media files in '{}'...", folder_name)
            },
        );

        let w_weak_scanner = ctx.window.as_weak();
        let lib_scanner = ctx.library.clone();
        let thumb_scanner = ctx.thumbnails.clone();
        let set_scanner = ctx.settings.clone();
        let st_scanner = ctx.state.clone();
        let target_p_scanner = normalized_path.clone();
        let cancel_ref = cancel_flag.clone();
        let bench_scanner = bench.clone();

        std::thread::Builder::new()
            .name(format!("media-scanner-fld-{}", folder_id))
            .spawn(move || {
                let w_weak_progress = w_weak_scanner.clone();
                let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));

                let t_scan = Instant::now();
                let auto_detect = set_scanner.get().formats.auto_detect_extensionless_media;

                let scan_folder_ids = if is_virtual_folder {
                    lib_scanner.folders().get_descendant_folder_ids(folder_id).unwrap_or_else(|_| vec![folder_id])
                } else {
                    vec![folder_id]
                };

                let mut combined_report = RefreshReport::default();

                for fid in &scan_folder_ids {
                    if cancel_ref.load(Ordering::Acquire) {
                        break;
                    }

                    let w_weak_p = w_weak_progress.clone();
                    let last_rep = last_progress_report.clone();

                    let rep_res = lib_scanner.refresher().refresh_folder_with_options(
                        *fid,
                        auto_detect,
                        Some(cancel_ref.clone()),
                        Some(move |cur, tot| {
                            let mut last = last_rep.lock();
                            if last.elapsed().as_millis() >= 35 || cur == tot {
                                *last = Instant::now();
                                let _ = w_weak_p.upgrade_in_event_loop(move |w| {
                                    StatusBarBridge::update_media_index_progress(&w, true, cur, tot);
                                });
                            }
                        }),
                    );

                    if let Ok(rep) = rep_res {
                        combined_report.total_scanned += rep.total_scanned;
                        combined_report.new_media += rep.new_media;
                        combined_report.updated_media += rep.updated_media;
                        combined_report.unchanged_media += rep.unchanged_media;
                        combined_report.deleted_media += rep.deleted_media;
                        combined_report.restored_media += rep.restored_media;
                        combined_report.corrupted_media += rep.corrupted_media;
                    }
                }

                let scan_elapsed = t_scan.elapsed().as_micros() as u64;
                bench_scanner.scan_executed.store(true, Ordering::Relaxed);
                bench_scanner.fs_scan_time_us.store(scan_elapsed, Ordering::Relaxed);

                if cancel_ref.load(Ordering::Acquire) {
                    return;
                }

                bench_scanner.files_scanned.store(combined_report.total_scanned, Ordering::Relaxed);
                bench_scanner.new_files.store(combined_report.new_media, Ordering::Relaxed);
                bench_scanner.updated_files.store(combined_report.updated_media, Ordering::Relaxed);
                bench_scanner.unchanged_files.store(combined_report.unchanged_media, Ordering::Relaxed);
                bench_scanner.deleted_files.store(combined_report.deleted_media, Ordering::Relaxed);
                bench_scanner.restored_files.store(combined_report.restored_media, Ordering::Relaxed);

                let mut media_ids = Vec::new();
                if let Ok(rtxn) = lib_scanner.raw_db().env.read_txn() {
                    for fid in &scan_folder_ids {
                        if let Ok(Some(ids)) = lib_scanner.raw_db().media_by_folder.get(&rtxn, fid) {
                            media_ids.extend(ids);
                        }
                    }
                }

                let mut known = Vec::with_capacity(media_ids.len());
                let mut seen = HashSet::new();
                if let Ok(rtxn) = lib_scanner.raw_db().env.read_txn() {
                    for id in media_ids {
                        if seen.insert(id)
                            && let Ok(Some(rec)) = lib_scanner.raw_db().media.get(&rtxn, &id)
                        {
                            known.push(rec);
                        }
                    }
                }

                let indexed_media = if is_virtual_folder {
                    BridgeMedia::preindex_records(&lib_scanner, known)
                } else {
                    BridgeMedia::preindex(&lib_scanner, known, &target_p_scanner)
                };

                let pipeline_items = BridgeMedia::pipeline_items(&indexed_media);
                let store = MediaStore::from_records(indexed_media);
                let (mut request, query) = BridgeMedia::capture_request(&st_scanner);
                request.refine_from = None;
                let layout = lay_out(&lib_scanner, &store, &request);

                let _ = w_weak_scanner.upgrade_in_event_loop(move |w| {
                    let is_still_current = {
                        let s = st_scanner.read();
                        s.current_folder_id == Some(folder_id) && s.active_folder_token == request_token
                    };

                    if is_still_current {
                        let pending = PendingList { store: Some(store), layout, query };
                        BridgeMedia::commit_layout(&w, &lib_scanner, &thumb_scanner, &set_scanner, &st_scanner, pending, MediaReloadOptions::new(false, false));
                        BridgeInspector::update(&w, &st_scanner);
                        BridgeTags::sync_tag_selection(&w, &lib_scanner, &st_scanner);
                        BridgeMedia::update_virtual_folder_button_state(&w, &lib_scanner, &st_scanner);
                        Self::reload_tree(&w, &lib_scanner, &set_scanner, &st_scanner);

                        StatusBarBridge::update_media_index_progress(&w, false, 0, 0);
                        StatusBarBridge::info(&w, "Ready.");
                        StatusBarBridge::update_metrics(&w, &st_scanner);

                        if !pipeline_items.is_empty() {
                            let config = ThumbnailConfig::from_performance_settings(&set_scanner.get().performance);
                            thumb_scanner.start_pipeline(folder_id, pipeline_items, config);
                        }
                    }
                });
            })
            .expect("Failed to spawn media scanner thread");
    }

    pub fn persist_expanded_folders(settings: &SettingsManager, expanded_set: &HashSet<FolderId>) {
        let mut cur_settings = settings.get();
        if !cur_settings.folders.persist_folder_tree_state {
            return;
        }

        let mut ids: Vec<u64> = expanded_set.iter().copied().collect();
        ids.sort_unstable();
        if cur_settings.folders.expanded_folder_ids != ids {
            cur_settings.folders.expanded_folder_ids = ids;
            settings.save_quiet_async(&cur_settings);
        }
    }

    pub fn toggle_expand(
        folder_id: FolderId,
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        {
            let mut st = state.write();
            if st.expanded_folder_ids.contains(&folder_id) {
                st.expanded_folder_ids.remove(&folder_id);
            } else {
                st.expanded_folder_ids.insert(folder_id);
            }
            Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
        }

        Self::reload_tree(window, library, settings, state);
    }

    /// Renders the folder tree from the in-memory Single Point of Truth snapshot with zero filesystem I/O.
    /// The Slint model is updated surgically (row-level insert/remove/move/update) instead of being replaced.
    /// Without a configured/registered root the snapshot is empty and an empty tree is shown.
    pub fn reload_tree(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        FolderTreeSync::refresh(window, library, settings, state);
    }

    /// A folder was restored in the database behind the cached snapshot: the snapshot is read again, the tree, the folder rows of the
    /// Browse view and the Inspector show the folder as present.
    pub fn reload_tree_after_restore(window: &MainWindow, library: &Library, settings: &SettingsManager, state: &RwLock<UiState>) {
        state.write().invalidate_folder_hierarchy();
        Self::reload_tree(window, library, settings, state);
        BridgeInspector::update(window, state);
    }

    /// Clears the whole browsing view for the "no root directory" situation.
    pub fn clear_view(window: &MainWindow, state: &Arc<RwLock<UiState>>, status_msg: &str) {
        state.write().invalidate_folder_hierarchy();
        Self::clear_view_with_empty_state(
            window,
            state,
            status_msg,
            "No Root Directory",
            Icon::Settings,
            "No root directory selected",
        );
    }

    /// Clears folder tree, media grid and selection state and shows an empty-state message.
    /// Does not touch the cached SPoT snapshot.
    pub fn clear_view_with_empty_state(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        status_msg: &str,
        empty_title: &str,
        empty_icon: Icon,
        virtual_folder_disabled_reason: &str,
    ) {
        {
            let mut st = state.write();
            st.current_folder_id = None;
            st.current_folder_path = None;
            st.all_folder_media.clear();
            st.active_indices.clear();
            st.mark_list_bytes_dirty();
            st.file_selection.clear();
            st.media_row_map.clear();
            st.last_executed_query.clear();
            st.expanded_folder_ids.clear();
            st.is_query_active = false;
            st.selected_rating = 0;
            st.filter.rating = None;
            st.filter.tag_ids.clear();
            st.tag_filter_query.clear();
            st.is_virtual_folder_mode = false;
            st.virtual_folder_count = 0;
        }
        window.set_folders(ModelRc::new(VecModel::from(vec![])));
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_tag_filter_text("".into());
        window.set_is_virtual_folder(false);
        window.set_is_virtual_folder_btn_enabled(false);
        window.set_virtual_folder_tooltip(
            virtual_folder_wording::tooltip(false, Some(virtual_folder_disabled_reason)).into(),
        );
        window.set_virtual_folder_count(0);

        window.set_empty_state_active(true);
        window.set_empty_state_title(empty_title.into());
        window.set_empty_state_description(status_msg.into());
        window.set_empty_state_icon(empty_icon.to_slint());

        BridgeFavorites::set_selected_path(None);
        StatusBarBridge::info(window, status_msg);
        StatusBarBridge::update_metrics(window, state);
        BridgeInspector::update(window, state);
    }

    pub fn prune_expanded_folders(
        deleted_ids: &[FolderId],
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let mut st = state.write();
        for id in deleted_ids {
            st.expanded_folder_ids.remove(id);
        }
        st.invalidate_folder_hierarchy();
        Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
    }

    pub fn find_visible_successor_folder(
        deleted_id: FolderId,
        affected_subtree_ids: &[FolderId],
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) -> Option<(FolderId, PathBuf)> {
        let snapshot = Self::ensure_snapshot(library, state);

        let affected_set: HashSet<FolderId> = affected_subtree_ids.iter().copied().collect();
        let target_node = snapshot.get_node(deleted_id)?;
        let parent_id = target_node.parent_id;

        let siblings = snapshot.get_children(parent_id);
        let current_pos = siblings.iter().position(|&id| id == deleted_id)?;

        for &sib_id in siblings.iter().skip(current_pos + 1) {
            if !affected_set.contains(&sib_id)
                && let Some(node) = snapshot.get_node(sib_id)
                && !node.is_deleted
            {
                return Some((node.id, node.path.clone()));
            }
        }

        for &sib_id in siblings[..current_pos].iter().rev() {
            if !affected_set.contains(&sib_id)
                && let Some(node) = snapshot.get_node(sib_id)
                && !node.is_deleted
            {
                return Some((node.id, node.path.clone()));
            }
        }

        if let Some(pid) = parent_id
            && !affected_set.contains(&pid)
            && let Some(node) = snapshot.get_node(pid)
        {
            return Some((node.id, node.path.clone()));
        }

        let app_settings = settings.get();
        let root_setting = app_settings.general.root_folder.as_ref()?;
        let normalized_root = normalize_path(root_setting);

        library
            .folders()
            .get_folder_by_path(&normalized_root)
            .ok()
            .flatten()
            .map(|f| (f.id, normalize_path(Path::new(&f.path))))
    }

    pub fn navigate_to_successor_if_affected(
        successor_target: Option<(FolderId, PathBuf)>,
        affected_folder_ids: &[FolderId],
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let is_current_affected = state
            .read()
            .current_folder_id
            .map(|id| affected_folder_ids.contains(&id))
            .unwrap_or(false);

        if is_current_affected {
            if let Some((succ_id, succ_path)) = successor_target {
                tracing::info!(
                    successor_id = succ_id,
                    successor_path = %LogObscurer::path(&succ_path),
                    "[FolderNavigation: Successor] Navigating active view to resolved successor folder."
                );
                Self::select_folder(
                    succ_id,
                    succ_path,
                    window,
                    library,
                    thumbnails,
                    settings,
                    state,
                );
            } else {
                Self::fallback_to_root_if_affected(
                    affected_folder_ids,
                    window,
                    library,
                    thumbnails,
                    settings,
                    state,
                );
            }
        } else {
            Self::reload_tree(window, library, settings, state);
            Self::scroll_to_selected(window);
            StatusBarBridge::update_metrics(window, state);
        }
    }

    pub fn fallback_to_root_if_affected(
        affected_folder_ids: &[FolderId],
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let is_current = state
            .read()
            .current_folder_id
            .map(|id| affected_folder_ids.contains(&id))
            .unwrap_or(false);

        if !is_current {
            return;
        }

        let root_setting = settings.get().general.root_folder;
        let Some(ref root_p) = root_setting else {
            return;
        };

        if let Ok(Some(root_rec)) = library.folders().get_folder_by_path(root_p) {
            tracing::info!(
                root_path = %LogObscurer::path(root_p),
                "[FolderNavigation: RootFallback] Falling back to root directory after deletion"
            );
            Self::select_folder(
                root_rec.id,
                normalize_path(root_p),
                window,
                library,
                thumbnails,
                settings,
                state,
            );
        }
    }
}
