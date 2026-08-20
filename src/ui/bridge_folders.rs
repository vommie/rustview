use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::{FolderNodeView, MainWindow};
use crate::library::dot_hidden::DotHiddenCache;
use crate::library::folders::normalize_path;
use crate::library::models::{FolderId, FolderRecord};
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

            if let Some(pos) = memchr::memrchr(b'/', p_str.as_bytes()) {
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

        // 1. Explicit Folder Refresh via View Toolbar Refresh Button (Rule A)
        window.on_refresh_folder(move || {
            let (folder_id_opt, path_opt) = {
                let s = st.read();
                (s.current_folder_id, s.current_folder_path.clone())
            };
            if let (Some(id), Some(path)) = (folder_id_opt, path_opt) {
                tracing::info!(
                    folder_id = id,
                    path = %LogObscurer::path(&path),
                    "[FolderRefresh: Toolbar] User clicked Toolbar Refresh button (Rule A). Executing explicit full refresh & thumbnail generation."
                );
                Self::refresh_folder_explicit(id, path, &win, &lib, &thumb, &set, &st);
            }
        });

        let dialog = settings_dialog;
        window.on_open_settings(move || {
            dialog.borrow_mut().show();
        });

        // 2. Normal Folder Selection via Tree Node Click (Opens known folders with Zero-FS LMDB speed)
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
            window.set_status_text("Cannot refresh folders: No root directory configured.".into());
            return;
        };

        let normalized_root = normalize_path(root_dir);
        if !normalized_root.is_dir() {
            window.set_status_text("Configured root directory not found on filesystem.".into());
            return;
        }

        {
            let mut st = state.write();
            st.is_refreshing_folders = true;
            st.folder_scan_count = 0;
        }
        window.set_is_refreshing_folders(true);
        window.set_folder_scan_count(0);
        window.set_status_text("Scanning directory tree on filesystem...".into());

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
                                w.set_folder_scan_count(count as i32);
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
                    w.set_is_refreshing_folders(false);

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
                            w.set_status_text(status_msg.into());
                            Self::reload_tree(&w, &lib, &set, &st);
                            Self::scroll_to_selected(&w);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "[FolderTreeRefreshError] Failed to refresh folder tree");
                            w.set_status_text(format!("Folder scan failed: {}", e).into());
                        }
                    }
                });
            })
            .expect("Failed to spawn folder tree refresher background thread");
    }

    /// Standard folder navigation. Opens known folders instantly from LMDB without scanning.
    /// If the folder is completely empty in LMDB, initiates Condition C initial scan & generation.
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

    /// Explicit user-requested folder refresh (Rules A & B: Toolbar Refresh or Folder Context Menu Refresh).
    /// Executes full directory scan on disk, updates LMDB, and launches dedicated full-folder thumbnail generation for all items.
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

    /// Internal folder selection and refresh coordinator adhering to Rules A, B, and C.
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

        // Cancel previous thumbnail generation pipeline
        ctx.thumbnails.cancel_pipeline();

        // Reset Viewport Scroll to Top immediately on folder change/refresh
        ctx.window.invoke_scroll_media_to_top();

        let bench = Arc::new(FolderLoadBenchmark::new(
            folder_id,
            normalized_path.to_string_lossy().to_string(),
        ));

        let (request_token, cancel_flag) = {
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
            st.file_selection.clear();
            st.last_executed_query.clear();
            st.folder_thumbnails.clear();
            st.all_folder_media.clear();

            st.is_query_active = false;
            st.selected_rating = 0;
            st.filter.rating = None;
            st.filter.search_query.clear();
            st.filter.tag_ids.clear();
            st.tag_filter_query.clear();

            (st.active_folder_token, new_flag)
        };

        {
            let mut s = ctx.settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(normalized_path.clone());
                ctx.settings.save_quiet_async(&s);
            }
        }

        ctx.window.set_search_query("".into());
        ctx.window.set_selected_rating(0);
        ctx.window.set_tag_filter_text("".into());

        Self::reload_tree(ctx.window, ctx.library, ctx.settings, ctx.state);
        Self::scroll_to_selected(ctx.window);

        // Immediate Zero-Delay Database Fetch
        let t_id_start = Instant::now();
        let media_ids = {
            let rtxn = ctx.library.raw_db().env.read_txn().ok();
            if let Some(ref txn) = rtxn {
                ctx.library.raw_db().media_by_folder.get(txn, &folder_id).ok().flatten().unwrap_or_default()
            } else {
                Vec::new()
            }
        };
        bench.lmdb_id_fetch_time_us.store(t_id_start.elapsed().as_micros() as u64, Ordering::Relaxed);

        let t_rec_start = Instant::now();
        let mut known_media = Vec::with_capacity(media_ids.len());
        if let Ok(rtxn) = ctx.library.raw_db().env.read_txn() {
            for id in media_ids {
                if let Ok(Some(rec)) = ctx.library.raw_db().media.get(&rtxn, &id)
                    && !rec.is_deleted
                {
                    known_media.push(rec);
                }
            }
        }
        bench.lmdb_record_fetch_time_us.store(t_rec_start.elapsed().as_micros() as u64, Ordering::Relaxed);
        bench.total_records_fetched.store(known_media.len(), Ordering::Relaxed);

        let is_empty_in_db = known_media.is_empty();
        let should_scan_and_generate = is_explicit_refresh || is_empty_in_db;

        // Stage 1c: Parallel Multi-Core Pre-Indexing
        let t_pre = Instant::now();
        let indexed_media = BridgeMedia::preindex(ctx.library, known_media, &normalized_path);
        bench.preindex_time_us.store(t_pre.elapsed().as_micros() as u64, Ordering::Relaxed);

        {
            let mut s = ctx.state.write();
            s.all_folder_media = indexed_media;
        }

        // Render known media into view immediately
        BridgeMedia::reload(ctx.window, ctx.library, ctx.thumbnails, ctx.settings, ctx.state, false, false);
        BridgeInspector::update(ctx.window, ctx.state);
        BridgeTags::sync_tag_selection(ctx.window, ctx.library, ctx.state);
        Self::scroll_to_selected(ctx.window);

        if !should_scan_and_generate {
            tracing::info!(
                folder_id = folder_id,
                folder_name = %folder_name,
                known_items = ctx.state.read().all_folder_media.len(),
                "[FolderNavigation: Zero-FS Load] Folder opened from LMDB cache with 0ms disk scan. Refresh/Generation awaiting user action."
            );
            ctx.window.set_is_indexing_media(false);
            ctx.window.set_status_text("Ready.".into());
            return;
        }

        tracing::info!(
            folder_id = folder_id,
            folder_name = %folder_name,
            is_explicit_refresh = is_explicit_refresh,
            is_empty_in_db = is_empty_in_db,
            "[FolderRefresh: Execute] Executing filesystem scan and dedicated full-folder thumbnail generation..."
        );

        ctx.window.set_is_indexing_media(true);
        ctx.window.set_media_index_current(0);
        ctx.window.set_media_index_total(0);
        ctx.window.set_status_text(format!("Scanning media files in '{}'...", folder_name).into());

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
                let refresh_result = lib_scanner.refresher().refresh_folder_with_options(
                    folder_id,
                    auto_detect,
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

                let scan_elapsed = t_scan.elapsed().as_micros() as u64;
                bench_scanner.scan_executed.store(true, Ordering::Relaxed);
                bench_scanner.fs_scan_time_us.store(scan_elapsed, Ordering::Relaxed);

                if cancel_ref.load(Ordering::Acquire) {
                    return;
                }

                match refresh_result {
                    Ok(report) => {
                        bench_scanner.files_scanned.store(report.total_scanned, Ordering::Relaxed);
                        bench_scanner.new_files.store(report.new_media, Ordering::Relaxed);
                        bench_scanner.updated_files.store(report.updated_media, Ordering::Relaxed);
                        bench_scanner.unchanged_files.store(report.unchanged_media, Ordering::Relaxed);
                        bench_scanner.deleted_files.store(report.deleted_media, Ordering::Relaxed);
                        bench_scanner.restored_files.store(report.restored_media, Ordering::Relaxed);

                        let raw_media = lib_scanner.media().get_by_folder(folder_id).unwrap_or_default();
                        let active_count = raw_media.len();
                        let indexed_media = BridgeMedia::preindex(&lib_scanner, raw_media, &target_p_scanner);

                        let all_targets: Vec<PreparedTarget> = indexed_media
                            .iter()
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
                                Self::reload_tree(&w, &lib_scanner, &set_scanner, &st_scanner);

                                w.set_is_indexing_media(false);
                                w.set_status_text("Ready.".into());

                                if !all_targets.is_empty() {
                                    let config = ThumbnailConfig::from_performance_settings(&set_scanner.get().performance);
                                    tracing::info!(
                                        folder_id = folder_id,
                                        total_targets = all_targets.len(),
                                        "[ThumbnailPipeline: Dedicated Launch] Launching full-folder thumbnail pipeline for ALL items"
                                    );
                                    thumb_scanner.start_pipeline(folder_id, all_targets, config);
                                }
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

    /// Fast-Path node expand/collapse toggle leveraging in-memory cached tree hierarchy.
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

    /// Hierarchical folder tree rendering with zero disk syscalls when cached.
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

        let mut dot_hidden_cache = DotHiddenCache::new();
        let mut ctx = FolderFlattenContext {
            expanded_ids: &expanded_ids,
            selected_id: current_sel,
            show_deleted: app_settings.folders.show_deleted_folders,
            show_metrics: app_settings.folders.show_folder_metrics,
            show_hidden: app_settings.folders.show_hidden_folders,
            support_dot_hidden: app_settings.folders.support_dot_hidden_files,
            dot_hidden_cache: &mut dot_hidden_cache,
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
        st.invalidate_folder_tree_cache();
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
