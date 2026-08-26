use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::BridgeTags;
use super::{FolderNodeView, MainWindow};
use crate::library::dot_hidden::DotHiddenCache;
use crate::library::folders::normalize_path;
use crate::library::models::{FolderId, FolderRecord, MediaId, MediaRecord};
use crate::library::refresh::RefreshReport;
use crate::library::Library;
use crate::settings::{GridAspectRatio, LogObscurer, SettingsDialogController, SettingsManager};
use crate::telemetry::FolderLoadBenchmark;
use crate::thumbnails::{PreparedTarget, ThumbnailConfig, ThumbnailManager};
use parking_lot::RwLock;
use slint::{ComponentHandle, Model, ModelRc, Timer, VecModel};
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

/// Context parameters passed during the recursive tree flattening pass.
pub struct FolderFlattenContext<'a> {
    pub expanded_ids: &'a HashSet<FolderId>,
    pub selected_id: Option<FolderId>,
    pub show_deleted: bool,
    pub show_metrics: bool,
    pub show_hidden: bool,
    pub support_dot_hidden: bool,
    pub dot_hidden_cache: &'a mut DotHiddenCache,
}

/// Grouped dependencies bundle for folder navigation and selection workflows.
pub struct FolderNavigationContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
}

struct ParentNodeInfo<'a> {
    parent: &'a FolderTreeNode,
    deleted_child_idx: usize,
}

pub struct FolderTreeBuilder;

impl FolderTreeBuilder {
    /// Builds the hierarchical tree structure in O(N) using SIMD path splitting and sibling-only sorting with zero disk syscalls.
    pub fn build_hierarchy(
        root_dir: &Path,
        folders: &[FolderRecord],
        media_counts: &HashMap<FolderId, usize>,
    ) -> Vec<FolderTreeNode> {
        let canonical_root_str = root_dir.to_string_lossy().trim_end_matches(['/', '\\']).to_string();

        let mut subfolder_map: HashMap<&str, Vec<usize>> = HashMap::with_capacity(folders.len());
        let mut root_idx = None;

        for (idx, f) in folders.iter().enumerate() {
            let p_str = f.path.trim_end_matches(['/', '\\']);
            if p_str == canonical_root_str {
                root_idx = Some(idx);
            }

            if let Some(pos) = memchr::memrchr2(b'/', b'\\', p_str.as_bytes()) {
                let parent_str = if pos == 0 { "/" } else { &p_str[..pos] };
                subfolder_map.entry(parent_str).or_default().push(idx);
            }
        }

        match root_idx {
            Some(r_idx) => {
                let root_rec = &folders[r_idx];
                let children = Self::build_subtrees(&root_rec.path, folders, &subfolder_map, media_counts);
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
        folders: &[FolderRecord],
        subfolder_map: &HashMap<&str, Vec<usize>>,
        media_counts: &HashMap<FolderId, usize>,
    ) -> Vec<FolderTreeNode> {
        let clean_path = current_path_str.trim_end_matches(['/', '\\']);
        let mut nodes = Vec::new();

        if let Some(child_indices) = subfolder_map.get(clean_path) {
            let mut sorted_indices = child_indices.clone();
            if sorted_indices.len() > 1 {
                sorted_indices.sort_unstable_by(|&a, &b| folders[a].path.cmp(&folders[b].path));
            }

            nodes.reserve(sorted_indices.len());
            for idx in sorted_indices {
                let child_rec = &folders[idx];
                let sub_children = Self::build_subtrees(&child_rec.path, folders, subfolder_map, media_counts);
                let subfolders_count = sub_children.len();
                let media_files_count = media_counts.get(&child_rec.id).copied().unwrap_or(0);

                nodes.push(FolderTreeNode {
                    record: child_rec.clone(),
                    subfolders_count,
                    media_files_count,
                    children: sub_children,
                });
            }
        }

        nodes
    }

    /// Fast in-memory flattening of expanded tree nodes for Slint ModelRc rendering with dot-folder and Linux .hidden file support.
    pub fn flatten(
        nodes: &[FolderTreeNode],
        depth: i32,
        ctx: &mut FolderFlattenContext<'_>,
        output: &mut Vec<FolderNodeView>,
    ) {
        for node in nodes {
            if node.record.is_deleted && !ctx.show_deleted {
                continue;
            }

            let path_obj = Path::new(&node.record.path);
            let raw_name = path_obj
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| node.record.path.clone());

            let is_hidden_folder = ctx.dot_hidden_cache.is_hidden(path_obj, &raw_name, ctx.support_dot_hidden);

            if is_hidden_folder && !ctx.show_hidden {
                continue;
            }

            let has_children = !node.children.is_empty();
            let is_expanded = ctx.expanded_ids.contains(&node.record.id);
            let is_selected = ctx.selected_id == Some(node.record.id);

            let formatted_name = if ctx.show_metrics {
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
                    ctx,
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
                    folder_id = id,
                    path = %LogObscurer::path(&path),
                    is_virtual_folder = st.read().is_virtual_folder_mode,
                    "[FolderRefresh: Toolbar] User clicked Toolbar Refresh button. Executing explicit full refresh & thumbnail generation."
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

        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        window.on_folder_collapse_all_clicked(move || {
            Self::collapse_all_except_active(&win, &lib, &set, &st);
        });

        let set = settings.clone();
        let st = state.clone();
        let win = window.clone_strong();
        let lib = library.clone();
        window.on_toggle_hidden_folders(move |show_hidden| {
            let mut app_settings = set.get();
            app_settings.folders.show_hidden_folders = show_hidden;
            set.save_quiet_async(&app_settings);
            Self::reload_tree(&win, &lib, &set, &st);
            Self::scroll_to_selected(&win);
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

    pub fn collapse_all_except_active(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let target_path_opt = state.read().current_folder_path.clone();

        let app_settings = settings.get();
        let root_path_opt = app_settings.general.root_folder.as_ref().map(|p| normalize_path(p));

        let mut keep_expanded_ids: HashSet<FolderId> = HashSet::new();

        if let Some(ref root_p) = root_path_opt {
            if let Ok(Some(root_rec)) = library.folders().get_folder_by_path(root_p) {
                keep_expanded_ids.insert(root_rec.id);
            }

            if let Some(ref target_p) = target_path_opt {
                let mut curr_ancestor = target_p.parent();
                while let Some(parent_p) = curr_ancestor {
                    if parent_p.starts_with(root_p) {
                        if let Ok(Some(parent_rec)) = library.folders().get_folder_by_path(parent_p) {
                            keep_expanded_ids.insert(parent_rec.id);
                        }
                        if parent_p == root_p.as_path() {
                            break;
                        }
                        curr_ancestor = parent_p.parent();
                    } else {
                        break;
                    }
                }
            }
        }

        {
            let mut st = state.write();
            st.expanded_folder_ids.retain(|id| keep_expanded_ids.contains(id));
            for id in &keep_expanded_ids {
                st.expanded_folder_ids.insert(*id);
            }
            Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
        };

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

        let win_weak_scroll = window.as_weak();
        Timer::single_shot(std::time::Duration::from_millis(60), move || {
            if let Some(w) = win_weak_scroll.upgrade() {
                Self::scroll_to_selected(&w);
            }
        });
    }

    pub fn refresh_folder_tree(
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let root_setting = settings.get().general.root_folder;
        let Some(ref root_dir) = root_setting else {
            StatusBarBridge::warn(window, "Cannot refresh folders: No root directory configured.");
            return;
        };

        let normalized_root = normalize_path(root_dir);
        if !normalized_root.is_dir() {
            StatusBarBridge::error(window, "Configured root directory not found on filesystem.");
            return;
        }

        {
            let mut st = state.write();
            st.is_refreshing_folders = true;
            st.folder_scan_count = 0;
        }
        StatusBarBridge::update_folder_scan_progress(window, true, 0);
        StatusBarBridge::info(window, "Scanning directory tree on filesystem...");

        let t_start = Instant::now();
        let w_weak = window.as_weak();
        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();

        std::thread::Builder::new()
            .name("folder-tree-refresher".into())
            .spawn(move || {
                let refresher = lib.folder_tree_refresher();
                let w_weak_progress = w_weak.clone();
                let last_progress_report = Arc::new(parking_lot::Mutex::new(Instant::now()));

                let report_res = refresher.refresh_tree(
                    &normalized_root,
                    Some(move |count| {
                        let mut last = last_progress_report.lock();
                        if last.elapsed().as_millis() >= 40 {
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
                        s.invalidate_folder_tree_cache();
                    }
                    StatusBarBridge::update_folder_scan_progress(&w, false, 0);

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
                            StatusBarBridge::success(&w, status_msg);
                            Self::reload_tree(&w, &lib, &set, &st);
                            Self::scroll_to_selected(&w);
                            BridgeMedia::update_virtual_folder_button_state(&w, &lib, &st);
                            StatusBarBridge::update_metrics(&w, &st);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "[FolderTreeRefreshError] Failed to refresh folder tree");
                            StatusBarBridge::error(&w, format!("Folder scan failed: {}", e));
                        }
                    }
                });
            })
            .expect("Failed to spawn folder tree refresher background thread");
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
        state: &Arc<RwLock<UiState>>,
    ) {
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

        tracing::info!(
            folder_id = folder_id,
            folder_name = %folder_name,
            total_items = targets.len(),
            "[FolderMetadataRefresh] Starting fast metadata-only refresh for folder..."
        );

        BridgeMedia::execute_metadata_only_refresh(
            targets,
            window,
            library,
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
            st.folder_thumbnails.clear();
            st.all_folder_media.clear();

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

        // Collect target folder IDs (either single active folder or recursive subtree)
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

        // Immediate Zero-Delay Database Fetch
        let t_id_start = Instant::now();
        let mut media_ids = Vec::new();
        if let Ok(rtxn) = ctx.library.raw_db().env.read_txn() {
            for fid in &target_folder_ids {
                if let Ok(Some(ids)) = ctx.library.raw_db().media_by_folder.get(&rtxn, fid) {
                    media_ids.extend(ids);
                }
            }
        }
        bench.lmdb_id_fetch_time_us.store(t_id_start.elapsed().as_micros() as u64, Ordering::Relaxed);

        let t_rec_start = Instant::now();
        let mut known_media = Vec::with_capacity(media_ids.len());
        let mut seen_ids = HashSet::new();

        if let Ok(rtxn) = ctx.library.raw_db().env.read_txn() {
            for id in media_ids {
                if seen_ids.insert(id)
                    && let Ok(Some(rec)) = ctx.library.raw_db().media.get(&rtxn, &id)
                {
                    known_media.push(rec);
                }
            }
        }
        bench.lmdb_record_fetch_time_us.store(t_rec_start.elapsed().as_micros() as u64, Ordering::Relaxed);
        bench.total_records_fetched.store(known_media.len(), Ordering::Relaxed);

        let is_empty_in_db = known_media.is_empty();
        let should_scan_and_generate = is_explicit_refresh || is_empty_in_db;

        let t_pre = Instant::now();
        let indexed_media = if is_virtual_folder {
            BridgeMedia::preindex_records(ctx.library, known_media)
        } else {
            BridgeMedia::preindex(ctx.library, known_media, &normalized_path)
        };
        bench.preindex_time_us.store(t_pre.elapsed().as_micros() as u64, Ordering::Relaxed);

        {
            let mut s = ctx.state.write();
            s.all_folder_media = indexed_media;
        }

        BridgeMedia::reload(ctx.window, ctx.library, ctx.thumbnails, ctx.settings, ctx.state, false, false);
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeTags::sync_tag_selection(ctx.window, ctx.library, ctx.state);
        BridgeMedia::update_virtual_folder_button_state(ctx.window, ctx.library, ctx.state);
        StatusBarBridge::update_metrics(ctx.window, ctx.state);
        Self::scroll_to_selected(ctx.window);

        if !should_scan_and_generate {
            tracing::info!(
                folder_id = folder_id,
                folder_name = %folder_name,
                is_virtual_folder = is_virtual_folder,
                known_items = ctx.state.read().all_folder_media.len(),
                "[FolderNavigation: Zero-FS Load] Folder opened from LMDB cache with 0ms disk scan."
            );
            StatusBarBridge::update_media_index_progress(ctx.window, false, 0, 0);
            StatusBarBridge::info(ctx.window, "Ready.");
            return;
        }

        tracing::info!(
            folder_id = folder_id,
            folder_name = %folder_name,
            is_virtual_folder = is_virtual_folder,
            is_explicit_refresh = is_explicit_refresh,
            is_empty_in_db = is_empty_in_db,
            "[FolderRefresh: Execute] Executing filesystem scan and dedicated thumbnail generation..."
        );

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

                if is_virtual_folder {
                    let _ = lib_scanner.folder_tree_refresher().refresh_tree(&target_p_scanner, None::<fn(usize)>);
                }

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

                let active_count = known.iter().filter(|m| !m.is_deleted).count();
                let indexed_media = if is_virtual_folder {
                    BridgeMedia::preindex_records(&lib_scanner, known)
                } else {
                    BridgeMedia::preindex(&lib_scanner, known, &target_p_scanner)
                };

                let all_targets: Vec<PreparedTarget> = indexed_media
                    .iter()
                    .filter(|item| !item.is_deleted)
                    .map(|item| PreparedTarget {
                        pipeline_id: 0,
                        folder_id: item.record.folder_id,
                        media_id: item.record.id,
                        file_path: item.full_path.clone(),
                    })
                    .collect();

                let _ = w_weak_scanner.upgrade_in_event_loop(move |w| {
                    let is_still_current = {
                        let s = st_scanner.read();
                        s.current_folder_id == Some(folder_id) && s.active_folder_token == request_token
                    };

                    if is_still_current {
                        {
                            let mut s = st_scanner.write();
                            s.all_folder_media = indexed_media;
                            s.update_folder_media_count_in_cache(folder_id, active_count);
                        }

                        BridgeMedia::reload(&w, &lib_scanner, &thumb_scanner, &set_scanner, &st_scanner, false, false);
                        BridgeInspector::update(&w, &st_scanner);
                        BridgeTags::sync_tag_selection(&w, &lib_scanner, &st_scanner);
                        BridgeMedia::update_virtual_folder_button_state(&w, &lib_scanner, &st_scanner);
                        Self::reload_tree(&w, &lib_scanner, &set_scanner, &st_scanner);

                        StatusBarBridge::update_media_index_progress(&w, false, 0, 0);
                        StatusBarBridge::info(&w, "Ready.");
                        StatusBarBridge::update_metrics(&w, &st_scanner);

                        if !all_targets.is_empty() {
                            let config = ThumbnailConfig::from_performance_settings(&set_scanner.get().performance);
                            tracing::info!(
                                folder_id = folder_id,
                                total_targets = all_targets.len(),
                                "[ThumbnailPipeline: Launch] Launching thumbnail pipeline for refreshed items"
                            );
                            thumb_scanner.start_pipeline(folder_id, all_targets, config);
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
            };
            Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
        };

        Self::reload_tree(window, library, settings, state);
    }

    pub fn reload_tree(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let app_settings = settings.get();
        let root_setting = app_settings.general.root_folder;

        let Some(ref root_dir) = root_setting else {
            window.set_folders(ModelRc::new(VecModel::from(vec![])));
            return;
        };

        let (current_sel, expanded_ids, cached_tree_opt) = {
            let st = state.read();
            (
                st.current_folder_id,
                st.expanded_folder_ids.clone(),
                st.cached_folder_tree.clone(),
            )
        };

        let tree = match cached_tree_opt {
            Some(cached) => cached,
            None => {
                let folders = library.folders().list_folders().unwrap_or_default();
                let media_counts = library.folders().get_all_media_counts().unwrap_or_default();
                let built_tree = FolderTreeBuilder::build_hierarchy(root_dir, &folders, &media_counts);

                {
                    let mut st = state.write();
                    st.cached_folder_tree = Some(built_tree.clone());
                    st.cached_media_counts = media_counts;
                }

                built_tree
            }
        };

        let mut st_guard = state.write();
        let mut ctx = FolderFlattenContext {
            expanded_ids: &expanded_ids,
            selected_id: current_sel,
            show_deleted: app_settings.folders.show_deleted_folders,
            show_metrics: app_settings.folders.show_folder_metrics,
            show_hidden: app_settings.folders.show_hidden_folders,
            support_dot_hidden: app_settings.folders.support_dot_hidden_files,
            dot_hidden_cache: &mut st_guard.dot_hidden_cache,
        };

        let mut folder_views = Vec::with_capacity(tree.len() * 4);
        FolderTreeBuilder::flatten(
            &tree,
            0,
            &mut ctx,
            &mut folder_views,
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
            st.invalidate_folder_tree_cache();
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
        window.set_virtual_folder_tooltip("No root directory selected".into());
        window.set_virtual_folder_count(0);

        window.set_empty_state_active(true);
        window.set_empty_state_title("No Root Directory".into());
        window.set_empty_state_description(status_msg.into());
        window.set_empty_state_icon("settings".into());

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
        st.invalidate_folder_tree_cache();
        Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
    }

    fn find_parent_in_tree<'a>(
        nodes: &'a [FolderTreeNode],
        target_id: FolderId,
    ) -> Option<ParentNodeInfo<'a>> {
        for node in nodes {
            for (idx, child) in node.children.iter().enumerate() {
                if child.record.id == target_id {
                    return Some(ParentNodeInfo {
                        parent: node,
                        deleted_child_idx: idx,
                    });
                }
            }
            if let Some(info) = Self::find_parent_in_tree(&node.children, target_id) {
                return Some(info);
            }
        }
        None
    }

    pub fn find_visible_successor_folder(
        deleted_id: FolderId,
        affected_subtree_ids: &[FolderId],
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) -> Option<(FolderId, PathBuf)> {
        let t0 = Instant::now();
        let app_settings = settings.get();
        let root_setting = app_settings.general.root_folder.as_ref()?;
        let normalized_root = normalize_path(root_setting);

        let cached_tree_opt = state.read().cached_folder_tree.clone();

        let tree = match cached_tree_opt {
            Some(cached) => cached,
            None => {
                let folders = library.folders().list_folders().unwrap_or_default();
                let media_counts = library.folders().get_all_media_counts().unwrap_or_default();
                FolderTreeBuilder::build_hierarchy(&normalized_root, &folders, &media_counts)
            }
        };

        if tree.is_empty() {
            return None;
        }

        let affected_set: HashSet<FolderId> = affected_subtree_ids.iter().copied().collect();
        let mut st_guard = state.write();
        let dot_hidden_cache = &mut st_guard.dot_hidden_cache;

        let show_deleted = app_settings.folders.show_deleted_folders;
        let show_hidden = app_settings.folders.show_hidden_folders;
        let support_dot_hidden = app_settings.folders.support_dot_hidden_files;

        let is_valid_sibling = |child: &FolderTreeNode, cache: &mut DotHiddenCache| -> bool {
            if affected_set.contains(&child.record.id) {
                return false;
            }
            if child.record.is_deleted && !show_deleted {
                return false;
            }
            let path_obj = Path::new(&child.record.path);
            let raw_name = path_obj
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| child.record.path.clone());
            if !show_hidden && cache.is_hidden(path_obj, &raw_name, support_dot_hidden) {
                return false;
            }
            true
        };

        if let Some(parent_info) = Self::find_parent_in_tree(&tree, deleted_id) {
            for child in parent_info.parent.children.iter().skip(parent_info.deleted_child_idx + 1) {
                if is_valid_sibling(child, dot_hidden_cache) {
                    let succ_id = child.record.id;
                    let succ_path = PathBuf::from(&child.record.path);
                    tracing::info!(
                        deleted_id = deleted_id,
                        successor_id = succ_id,
                        successor_path = %LogObscurer::path(&succ_path),
                        strategy = "NextSiblingInNode",
                        elapsed_us = t0.elapsed().as_micros(),
                        "[FolderSuccessor: Resolved] Selected next visible sibling in parent node"
                    );
                    return Some((succ_id, succ_path));
                }
            }

            for child in parent_info.parent.children[..parent_info.deleted_child_idx].iter().rev() {
                if is_valid_sibling(child, dot_hidden_cache) {
                    let succ_id = child.record.id;
                    let succ_path = PathBuf::from(&child.record.path);
                    tracing::info!(
                        deleted_id = deleted_id,
                        successor_id = succ_id,
                        successor_path = %LogObscurer::path(&succ_path),
                        strategy = "PrecedingSiblingInNode",
                        elapsed_us = t0.elapsed().as_micros(),
                        "[FolderSuccessor: Resolved] Selected preceding visible sibling in parent node"
                    );
                    return Some((succ_id, succ_path));
                }
            }

            let parent_id = parent_info.parent.record.id;
            let parent_path = PathBuf::from(&parent_info.parent.record.path);
            tracing::info!(
                deleted_id = deleted_id,
                successor_id = parent_id,
                successor_path = %LogObscurer::path(&parent_path),
                strategy = "ParentNode",
                elapsed_us = t0.elapsed().as_micros(),
                "[FolderSuccessor: Resolved] No siblings left in node. Selected parent node."
            );
            return Some((parent_id, parent_path));
        }

        let root_fallback = library
            .folders()
            .get_folder_by_path(&normalized_root)
            .ok()
            .flatten()
            .map(|f| (f.id, normalize_path(Path::new(&f.path))));

        tracing::info!(
            deleted_id = deleted_id,
            strategy = "RootFallback",
            elapsed_us = t0.elapsed().as_micros(),
            "[FolderSuccessor: Resolved] Reached root level. Falling back to root directory."
        );

        root_fallback
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
            tracing::debug!(
                "[FolderNavigation: Successor] Active folder was not in deleted subtree. Maintaining active selection."
            );
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
