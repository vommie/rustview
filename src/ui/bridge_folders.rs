use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::{FolderNodeView, MainWindow};
use crate::library::Library;
use crate::library::models::{FolderId, FolderRecord};
use crate::settings::{GridAspectRatio, SettingsDialogController, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
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
        let canonical_root = match root_dir.canonicalize() {
            Ok(p) => p,
            Err(_) => root_dir.to_path_buf(),
        };

        let mut subfolder_map: HashMap<PathBuf, Vec<FolderRecord>> = HashMap::new();

        for f in folders {
            let p = Path::new(&f.path);
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
            let p = Path::new(&f.path);
            p == canonical_root || p == root_dir
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
        let path_buf = PathBuf::from(current_path_str);

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
                path: node.record.path.clone().into(),
                depth,
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
            let folder_id = st.read().current_folder_id;
            if let Some(id) = folder_id {
                let _ = lib.refresher().refresh_folder(id);
                BridgeMedia::reload(&win, &lib, &thumb, &set, &st, true, true);
                win.set_status_text("Folder refreshed.".into());
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

        if !root_path.is_dir() {
            Self::clear_view(
                window,
                state,
                "Configured Root Directory does not exist on disk.",
            );
            return;
        }

        tracing::info!(root = ?root_path, "Synchronizing root directory...");

        let folder_record = match library.folders().get_folder_by_path(root_path) {
            Ok(Some(f)) => f,
            _ => match library.folders().add_folder(root_path) {
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
            && last_path.starts_with(root_path)
            && let Ok(Some(f)) = library.folders().get_folder_by_path(last_path)
        {
            (f.id, last_path.clone())
        } else {
            (folder_record.id, root_path.clone())
        };

        {
            let mut st = state.write();
            if s.folders.auto_reveal_remembered_folder {
                st.expanded_folder_ids.insert(folder_record.id);

                let mut current_ancestor = target_folder.1.parent();
                while let Some(parent_p) = current_ancestor {
                    if parent_p.starts_with(root_path) {
                        if let Ok(Some(parent_rec)) = library.folders().get_folder_by_path(parent_p)
                        {
                            st.expanded_folder_ids.insert(parent_rec.id);
                        }
                        if parent_p == root_path {
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

        let _ = library.refresher().refresh_folder(target_folder.0);

        let raw_media = library
            .media()
            .get_by_folder(target_folder.0)
            .unwrap_or_default();
        let indexed_media = BridgeMedia::preindex(library, raw_media, &target_folder.1);

        {
            let mut st = state.write();
            st.current_folder_id = Some(target_folder.0);
            st.current_folder_path = Some(target_folder.1);
            st.all_folder_media = indexed_media;
            st.folder_thumbnails.clear();
            st.active_indices.clear();
            st.file_selection.clear();
            st.last_executed_query.clear();
        }

        window.set_status_text("Ready.".into());

        Self::reload_tree(window, library, settings, state);
        BridgeMedia::reload(window, library, thumbnails, settings, state, true, false);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
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

        if !root_dir.is_dir() {
            window.set_status_text("Configured root directory not found on filesystem.".into());
            return;
        }

        window.set_is_refreshing_folders(true);
        window.set_folder_scan_count(0);
        window.set_status_text("Scanning directory tree on Linux filesystem...".into());

        let t_start = Instant::now();
        let refresher = library.folder_tree_refresher();

        let w_weak = window.as_weak();
        let report_res = refresher.refresh_tree(
            root_dir,
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

    pub fn select_folder(
        folder_id: FolderId,
        path: PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        tracing::info!(folder_id = folder_id, path = ?path, "User selected folder in sidebar");

        match library.refresher().refresh_folder(folder_id) {
            Ok(report) => {
                tracing::info!(
                    folder_id = folder_id,
                    scanned = report.total_scanned,
                    new = report.new_media,
                    updated = report.updated_media,
                    unchanged = report.unchanged_media,
                    deleted = report.deleted_media,
                    "Folder scan complete"
                );
            }
            Err(e) => {
                tracing::error!(folder_id = folder_id, error = %e, "Failed to scan folder");
            }
        }

        let raw_media = library.media().get_by_folder(folder_id).unwrap_or_default();
        let indexed_media = BridgeMedia::preindex(library, raw_media, &path);

        tracing::info!(
            folder_id = folder_id,
            count = indexed_media.len(),
            "Cached preformatted folder media records into RAM."
        );

        {
            let mut st = state.write();
            st.current_folder_id = Some(folder_id);
            st.current_folder_path = Some(path.clone());
            st.all_folder_media = indexed_media;
            st.folder_thumbnails.clear();
            st.active_indices.clear();
            st.file_selection.clear();
            st.last_executed_query.clear();
        }

        {
            let mut s = settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(path);
                settings.save_quiet(&s);
            }
        }

        Self::reload_tree(window, library, settings, state);
        BridgeMedia::reload(window, library, thumbnails, settings, state, true, true);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
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
        if let Ok(rtxn) = library.raw_db().env.read_txn() {
            for f in &folders {
                let count = library
                    .raw_db()
                    .media_by_folder
                    .get(&rtxn, &f.id)
                    .unwrap_or(None)
                    .map(|v| v.len())
                    .unwrap_or(0);
                media_counts.insert(f.id, count);
            }
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
        }
        window.set_folders(ModelRc::new(VecModel::from(vec![])));
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_items_metric_text("Items: 0".into());
        window.set_status_text(status_msg.into());
        BridgeInspector::update(window, state);
    }
}
