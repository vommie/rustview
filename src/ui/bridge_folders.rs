use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::{FolderNodeView, MainWindow};
use crate::library::folders::normalize_path;
use crate::library::models::{FolderId, FolderRecord};
use crate::library::Library;
use crate::settings::{GridAspectRatio, LogObscurer, SettingsDialogController, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{ComponentHandle, ModelRc, Timer, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct FolderTreeNode {
    pub record: FolderRecord,
    pub subfolders_count: usize,
    pub media_files_count: usize,
    pub children: Vec<FolderTreeNode>,
}

pub struct FolderTreeBuilder;

impl FolderTreeBuilder {
    pub fn build_hierarchy(
        root_dir: &Path,
        folders: &[FolderRecord],
        media_counts: &HashMap<FolderId, usize>,
    ) -> Vec<FolderTreeNode> {
        let canonical_root = normalize_path(root_dir);

        let mut subfolder_map: HashMap<PathBuf, Vec<FolderRecord>> = HashMap::new();

        for f in folders {
            let p = normalize_path(Path::new(&f.path));
            if let Some(parent) = p.parent() {
                subfolder_map
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(f.clone());
            }
        }

        for list in subfolder_map.values_mut() {
            list.sort_by_key(|a| a.path.to_lowercase());
        }

        let root_record = folders.iter().find(|f| {
            let p = normalize_path(Path::new(&f.path));
            p == canonical_root
        });

        match root_record {
            Some(root_rec) => {
                let children = Self::build_subtrees(&root_rec.path, &subfolder_map, media_counts);
                let subfolders_count = children.len();
                let media_files_count = media_counts.get(&root_rec.id).copied().unwrap_or(0);

                vec![FolderTreeNode {
                    record: root_rec.clone(),
                    subfolders_count,
                    media_files_count,
                    children,
                }]
            }
            None => Vec::new(),
        }
    }

    fn build_subtrees(
        current_path_str: &str,
        subfolder_map: &HashMap<PathBuf, Vec<FolderRecord>>,
        media_counts: &HashMap<FolderId, usize>,
    ) -> Vec<FolderTreeNode> {
        let mut nodes = Vec::new();
        let path_buf = normalize_path(Path::new(current_path_str));

        if let Some(children) = subfolder_map.get(&path_buf) {
            for child in children {
                let sub_children = Self::build_subtrees(&child.path, subfolder_map, media_counts);
                let subfolders_count = sub_children.len();
                let media_files_count = media_counts.get(&child.id).copied().unwrap_or(0);

                nodes.push(FolderTreeNode {
                    record: child.clone(),
                    subfolders_count,
                    media_files_count,
                    children: sub_children,
                });
            }
        }

        nodes
    }

    pub fn flatten(
        nodes: &[FolderTreeNode],
        depth: i32,
        expanded_ids: &HashSet<FolderId>,
        selected_id: Option<FolderId>,
        show_deleted: bool,
        show_metrics: bool,
        output: &mut Vec<FolderNodeView>,
    ) {
        for node in nodes {
            if node.record.is_deleted && !show_deleted {
                continue;
            }

            let has_children = !node.children.is_empty();
            let is_expanded = expanded_ids.contains(&node.record.id);
            let is_selected = selected_id == Some(node.record.id);

            let path_obj = Path::new(&node.record.path);
            let raw_name = path_obj
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| node.record.path.clone());

            let formatted_name = if show_metrics {
                format!(
                    "{} ({}, {})",
                    raw_name, node.subfolders_count, node.media_files_count
                )
            } else {
                raw_name
            };

            output.push(FolderNodeView {
                id: node.record.id as i32,
                name: formatted_name.into(),
                depth,
                path: node.record.path.clone().into(),
                has_children,
                is_expanded,
                is_selected,
                is_deleted: node.record.is_deleted,
                subfolder_count: node.subfolders_count as i32,
                file_count: node.media_files_count as i32,
            });

            if has_children && is_expanded {
                Self::flatten(
                    &node.children,
                    depth + 1,
                    expanded_ids,
                    selected_id,
                    show_deleted,
                    show_metrics,
                    output,
                );
            }
        }
    }
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
            let (folder_id_opt, path_opt) = {
                let s = st.read();
                (s.current_folder_id, s.current_folder_path.clone())
            };
            if let (Some(id), Some(path)) = (folder_id_opt, path_opt) {
                tracing::info!(
                    folder = %LogObscurer::folder(Some(id), &path),
                    "[FolderPanel] Asynchronous folder refresh triggered"
                );
                Self::select_folder(id, path, &win, &lib, &thumb, &set, &st);
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
            Self::toggle_expand(id as u64, &win, &lib, &set, &st);
        });

        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_refresh_clicked(move || {
            tracing::info!("[FolderPanel: Toolbar] Folder tree structure refresh triggered via panel toolbar");
            Self::refresh_folder_tree(&win, &lib, &set, &st);
        });

        let lib = library;
        let thumb = thumbnails;
        let set = settings;
        let st = state;
        let win = window.clone_strong();
        window.on_favorite_selected(move |path_str| {
            let path = PathBuf::from(path_str.as_str());
            if let Ok(Some(folder)) = lib.folders().get_folder_by_path(&path) {
                Self::select_folder(folder.id, path, &win, &lib, &thumb, &set, &st);
            }
        });
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

        let Some(ref root_path) = s.general.root_folder else {
            Self::clear_view(
                window,
                state,
                "No Root Directory configured. Open Settings to select a root folder.",
            );
            return;
        };

        let normalized_root = normalize_path(root_path);
        if !normalized_root.is_dir() {
            Self::clear_view(
                window,
                state,
                "Configured Root Directory does not exist on disk.",
            );
            return;
        }

        tracing::info!(root = %LogObscurer::path(&normalized_root), "Synchronizing root directory...");

        let folder_record = match library.folders().get_folder_by_path(&normalized_root) {
            Ok(Some(f)) => f,
            _ => match library.folders().add_folder(&normalized_root) {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to register root directory");
                    return;
                }
            },
        };

        let target_folder = if s.general.remember_last_folder
            && let Some(ref last_path) = s.general.last_folder_path
            && last_path.is_dir()
            && normalize_path(last_path).starts_with(&normalized_root)
            && let Ok(Some(f)) = library.folders().get_folder_by_path(last_path)
        {
            (f.id, normalize_path(last_path))
        } else {
            (folder_record.id, normalized_root.clone())
        };

        {
            let mut st = state.write();
            if s.folders.auto_reveal_remembered_folder {
                st.expanded_folder_ids.insert(folder_record.id);

                let mut current_ancestor = target_folder.1.parent();
                while let Some(parent_p) = current_ancestor {
                    if parent_p.starts_with(&normalized_root) {
                        if let Ok(Some(parent_rec)) = library.folders().get_folder_by_path(parent_p)
                        {
                            st.expanded_folder_ids.insert(parent_rec.id);
                        }
                        if parent_p == normalized_root {
                            break;
                        }
                        current_ancestor = parent_p.parent();
                    } else {
                        break;
                    }
                }
            } else if st.expanded_folder_ids.is_empty() {
                st.expanded_folder_ids.insert(folder_record.id);
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
    }

    pub fn refresh_folder_tree(
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let root_setting = settings.get().general.root_folder;
        let Some(ref root_dir) = root_setting else {
            window.set_status_text("Cannot refresh folders: No root directory configured.".into());
            return;
        };

        let normalized_root = normalize_path(root_dir);
        if !normalized_root.is_dir() {
            window.set_status_text("Configured root directory not found on filesystem.".into());
            return;
        }

        tracing::info!(
            root = %LogObscurer::path(&normalized_root),
            "[FolderPanel: Toolbar] Initiating high-speed recursive folder refresh from panel toolbar"
        );

        window.set_is_refreshing_folders(true);
        window.set_folder_scan_count(0);
        window.set_status_text("Scanning directory tree on Linux filesystem...".into());

        let t_start = Instant::now();
        let refresher = library.folder_tree_refresher();

        let w_weak = window.as_weak();
        let report_res = refresher.refresh_tree(
            &normalized_root,
            Some(move |count| {
                if let Some(w) = w_weak.upgrade() {
                    w.set_folder_scan_count(count as i32);
                }
            }),
        );

        window.set_is_refreshing_folders(false);

        match report_res {
            Ok(report) => {
                let status_msg = format!(
                    "Folder scan completed in {}ms: {} total dirs, {} new added, {} deleted, {} restored",
                    t_start.elapsed().as_millis(),
                    report.total_scanned_dirs,
                    report.new_folders_added,
                    report.folders_marked_deleted,
                    report.folders_restored
                );
                tracing::info!(report = ?report, %status_msg, "Folder tree refresh completed successfully");
                window.set_status_text(status_msg.into());
                Self::reload_tree(window, library, settings, state);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to refresh folder tree");
                window.set_status_text(format!("Folder scan failed: {}", e).into());
            }
        }
    }

    /// 4-Stage Asynchronous Folder Navigation Workflow.
    /// Resets query filters, Tag & Rate panel selections, and search fields while strictly preserving regex toggle button states.
    pub fn select_folder(
        folder_id: FolderId,
        path: PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t0 = Instant::now();
        let normalized_path = normalize_path(&path);
        let folder_name = normalized_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| normalized_path.to_string_lossy().to_string());

        tracing::info!(
            folder = %LogObscurer::folder(Some(folder_id), &normalized_path),
            "[FolderNavigation: Stage 1/4] Immediate UI transition started"
        );

        // Cancel previous thumbnail pipeline and background scanner immediately
        thumbnails.cancel_pipeline();

        // Stage 1: Atomically reset query state, clear search text, and reset Tag & Rate selections while preserving active regex button states
        let (request_token, cancel_flag) = {
            let mut st = state.write();

            if let Some(prev_flag) = st.active_folder_cancel_flag.take() {
                prev_flag.store(true, Ordering::Release);
            }
            let new_flag = Arc::new(AtomicBool::new(false));
            st.active_folder_cancel_flag = Some(new_flag.clone());

            st.active_folder_token = st.active_folder_token.wrapping_add(1);
            st.current_folder_id = Some(folder_id);
            st.current_folder_path = Some(normalized_path.clone());
            st.active_indices.clear();
            st.file_selection.clear();
            st.last_executed_query.clear();
            st.folder_thumbnails.clear();
            st.all_folder_media.clear();

            // Reset query & filter state (preserving active regex mode toggle)
            st.is_query_active = false;
            st.selected_rating = 0;
            st.filter.rating = None;
            st.filter.search_query.clear();
            st.filter.tag_ids.clear();
            st.tag_filter_query.clear();

            (st.active_folder_token, new_flag)
        };

        {
            let mut s = settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(normalized_path.clone());
                settings.save_quiet(&s);
            }
        }

        // Reset UI text & rating inputs while preserving regex button states
        window.set_search_query("".into());
        window.set_selected_rating(0);
        window.set_tag_filter_text("".into());

        // Instantly update tree highlight, clear grid, and set initial transition status
        Self::reload_tree(window, library, settings, state);
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_items_metric_text("Loading...".into());
        window.set_status_text(format!("Opening '{}'...", folder_name).into());
        window.set_is_indexing_media(false);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);

        // Stage 2: Defer known media loading & initial grid render to next event loop tick
        let w_weak = window.as_weak();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        let target_path = normalized_path.clone();

        Timer::single_shot(std::time::Duration::from_millis(15), move || {
            let Some(win) = w_weak.upgrade() else { return; };

            let is_still_current = {
                let s = st.read();
                s.current_folder_id == Some(folder_id) && s.active_folder_token == request_token
            };
            if !is_still_current { return; }

            // Load already-known active media from LMDB (includes all migrated ratings & tag flags)
            let known_media = lib.media().get_by_folder(folder_id).unwrap_or_default();
            let indexed_media = BridgeMedia::preindex(&lib, known_media, &target_path);

            {
                let mut s = st.write();
                s.all_folder_media = indexed_media;
            }

            // Render grid with placeholder tiles
            BridgeMedia::reload(&win, &lib, &thumb, &set, &st, false, false);
            BridgeInspector::update(&win, &st);
            BridgeTags::sync_tag_selection(&win, &lib, &st);

            win.set_is_indexing_media(true);
            win.set_media_index_current(0);
            win.set_media_index_total(0);
            win.set_status_text(format!("Scanning media files in '{}'...", folder_name).into());

            // Stage 3 & 4: Spawn background filesystem scanner & hash-check, then launch thumbnail pipeline
            let w_weak_scanner = win.as_weak();
            let lib_scanner = lib.clone();
            let thumb_scanner = thumb.clone();
            let set_scanner = set.clone();
            let st_scanner = st.clone();
            let target_p_scanner = target_path.clone();
            let cancel_ref = cancel_flag.clone();

            std::thread::Builder::new()
                .name(format!("media-scanner-fld-{}", folder_id))
                .spawn(move || {
                    let w_weak_progress = w_weak_scanner.clone();
                    let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));

                    let refresh_result = lib_scanner.refresher().refresh_folder_with_progress(
                        folder_id,
                        Some(cancel_ref.clone()),
                        Some(move |cur, tot| {
                            let mut last = last_progress_report.lock();
                            if last.elapsed().as_millis() >= 35 || cur == tot {
                                *last = Instant::now();
                                let _ = w_weak_progress.upgrade_in_event_loop(move |w| {
                                    w.set_media_index_current(cur as i32);
                                    w.set_media_index_total(tot as i32);
                                });
                            }
                        }),
                    );

                    if cancel_ref.load(Ordering::Acquire) {
                        tracing::debug!(folder_id = folder_id, "[FolderNavigation] Aborted stale media scan after cancellation");
                        return;
                    }

                    match refresh_result {
                        Ok(report) => {
                            tracing::info!(
                                folder_id = folder_id,
                                total_scanned = report.total_scanned,
                                new = report.new_media,
                                updated = report.updated_media,
                                unchanged = report.unchanged_media,
                                deleted = report.deleted_media,
                                restored = report.restored_media,
                                elapsed_ms = t0.elapsed().as_millis(),
                                "[FolderNavigation: Stage 3/4] Media scan & DB sync complete. Launching thumbnail pipeline..."
                            );

                            let raw_media = lib_scanner.media().get_by_folder(folder_id).unwrap_or_default();
                            let indexed_media = BridgeMedia::preindex(&lib_scanner, raw_media, &target_p_scanner);

                            let _ = w_weak_scanner.upgrade_in_event_loop(move |w| {
                                let is_still_current = {
                                    let s = st_scanner.read();
                                    s.current_folder_id == Some(folder_id) && s.active_folder_token == request_token
                                };

                                if is_still_current {
                                    {
                                        let mut s = st_scanner.write();
                                        s.all_folder_media = indexed_media;
                                    }

                                    w.set_is_indexing_media(false);
                                    w.set_status_text("Ready.".into());

                                    // Stage 4: Render final media view AND start thumbnail pipeline
                                    BridgeMedia::reload(&w, &lib_scanner, &thumb_scanner, &set_scanner, &st_scanner, true, false);
                                    BridgeInspector::update(&w, &st_scanner);
                                    BridgeTags::sync_tag_selection(&w, &lib_scanner, &st_scanner);
                                    Self::reload_tree(&w, &lib_scanner, &set_scanner, &st_scanner);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!(folder_id = folder_id, error = %e, "[FolderNavigation] Background media scan failed");
                            let _ = w_weak_scanner.upgrade_in_event_loop(move |w| {
                                let is_still_current = {
                                    let s = st_scanner.read();
                                    s.current_folder_id == Some(folder_id) && s.active_folder_token == request_token
                                };
                                if is_still_current {
                                    w.set_is_indexing_media(false);
                                    w.set_status_text(format!("Media scan error: {}", e).into());
                                }
                            });
                        }
                    }
                })
                .expect("Failed to spawn media scanner thread");
        });
    }

    pub fn persist_expanded_folders(settings: &SettingsManager, expanded_set: &HashSet<FolderId>) {
        let mut cur_settings = settings.get();
        if !cur_settings.folders.persist_folder_tree_state {
            return;
        }

        let mut ids: Vec<u64> = expanded_set.iter().copied().collect();
        ids.sort();
        if cur_settings.folders.expanded_folder_ids != ids {
            cur_settings.folders.expanded_folder_ids = ids;
            settings.save_quiet(&cur_settings);
            tracing::debug!(
                expanded_count = cur_settings.folders.expanded_folder_ids.len(),
                "[FolderTree: Persistence] Saved expanded folder IDs to persistent settings"
            );
        }
    }

    pub fn toggle_expand(
        folder_id: FolderId,
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let is_now_expanded = {
            let mut st = state.write();
            let expanded = if st.expanded_folder_ids.contains(&folder_id) {
                st.expanded_folder_ids.remove(&folder_id);
                false
            } else {
                st.expanded_folder_ids.insert(folder_id);
                true
            };
            Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
            expanded
        };

        tracing::info!(
            folder_id = folder_id,
            is_expanded = is_now_expanded,
            "[FolderTree: NodeToggle] Toggled folder node expansion"
        );

        Self::reload_tree(window, library, settings, state);
    }

    pub fn reload_tree(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let t0 = Instant::now();
        let app_settings = settings.get();
        let root_setting = app_settings.general.root_folder;

        let Some(ref root_dir) = root_setting else {
            window.set_folders(ModelRc::new(VecModel::from(vec![])));
            return;
        };

        let folders = library.folders().list_folders().unwrap_or_default();
        let (current_sel, expanded_ids) = {
            let st = state.read();
            (st.current_folder_id, st.expanded_folder_ids.clone())
        };

        let mut media_counts: HashMap<FolderId, usize> = HashMap::new();
        for f in &folders {
            let count = library.media().get_active_count(f.id).unwrap_or(0);
            media_counts.insert(f.id, count);
        }

        let tree = FolderTreeBuilder::build_hierarchy(root_dir, &folders, &media_counts);

        let mut folder_views = Vec::new();
        FolderTreeBuilder::flatten(
            &tree,
            0,
            &expanded_ids,
            current_sel,
            app_settings.folders.show_deleted_folders,
            app_settings.folders.show_folder_metrics,
            &mut folder_views,
        );

        tracing::debug!(
            total_folders_in_db = folders.len(),
            visible_rows = folder_views.len(),
            expanded_parents = expanded_ids.len(),
            show_deleted = app_settings.folders.show_deleted_folders,
            show_metrics = app_settings.folders.show_folder_metrics,
            elapsed_us = t0.elapsed().as_micros(),
            "[FolderTree: Zero-FS Projection] Rendered folder tree to Slint UI"
        );

        window.set_folders(ModelRc::new(VecModel::from(folder_views)));
    }

    pub fn clear_view(window: &MainWindow, state: &Arc<RwLock<UiState>>, status_msg: &str) {
        {
            let mut st = state.write();
            st.current_folder_id = None;
            st.current_folder_path = None;
            st.all_folder_media.clear();
            st.folder_thumbnails.clear();
            st.active_indices.clear();
            st.file_selection.clear();
            st.media_row_map.clear();
            st.last_executed_query.clear();
            st.expanded_folder_ids.clear();
            st.is_query_active = false;
            st.selected_rating = 0;
            st.filter.rating = None;
            st.filter.tag_ids.clear();
            st.tag_filter_query.clear();
        }
        window.set_folders(ModelRc::new(VecModel::from(vec![])));
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_items_metric_text("Items: 0".into());
        window.set_status_text(status_msg.into());
        window.set_tag_filter_text("".into());
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
        Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
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
