//! The folder tree snapshot: scrolling to the selection, ensuring the snapshot, ancestors, the remembered folder and the root.

use super::BridgeFolders;
use crate::icons::Icon;
use crate::ui::MainWindow;
use crate::ui::state::UiState;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::folders::normalize_path;
use rustview_core::library::models::{FolderHierarchySnapshot, FolderId};
use rustview_core::settings::{GridAspectRatio, LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{ComponentHandle, Model, Timer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

impl BridgeFolders {
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

    /// Returns the cached `SPoT` snapshot or lazily loads it from the database (zero filesystem access).
    /// A failed load is logged and yields an empty snapshot that is NOT cached, so the next call retries.
    pub fn ensure_snapshot(library: &Library, state: &RwLock<UiState>) -> Arc<FolderHierarchySnapshot> {
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
    pub(super) fn collect_ancestor_ids(
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
    pub(super) fn resolve_remembered_folder(
        snapshot: &FolderHierarchySnapshot,
        last_path: &Path,
    ) -> Option<(FolderId, PathBuf)> {
        if !last_path.is_dir() {
            return None;
        }
        let normalized = normalize_path(last_path);
        snapshot.path_index.get(&normalized).map(|&id| (id, normalized.clone()))
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

            if let (Some(root_node), Some(target_p)) = (snapshot.get_node(root_id), target_path_opt.as_ref()) {
                keep_expanded_ids.extend(Self::collect_ancestor_ids(&snapshot, &root_node.path, target_p));
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
            Self::clear_view(window, state, "Configured Root Directory does not exist on disk.");
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
}
