//! UI-side orchestration after a successful folder move: patches the in-memory `SPoT` snapshot surgically, rebases
//! runtime state that still holds old paths, and refreshes only the affected parts of the folder tree.

use crate::ui::MainWindow;
use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::folder_tree::tree_sync::FolderTreeSync;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use parking_lot::RwLock;
use rustview_core::library::folder_move::{FolderMovePatch, FolderMoveReport};
use rustview_core::library::models::FolderId;
use rustview_core::query::FolderDir;
use rustview_core::settings::LogObscurer;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Safety limit for walking parent chains of the destination folder.
const MAX_CHAIN_WALK: usize = 10_000;

pub struct BridgeFolderMoveSync;

impl BridgeFolderMoveSync {
    /// Applies the effects of a committed folder move to snapshot, runtime state and views.
    pub fn apply(report: &FolderMoveReport, window: &MainWindow, ctx: &FolderDialogContext) {
        let t0 = Instant::now();
        let patch = &report.patch;

        tracing::info!(
            folder_id = patch.moved_folder_id,
            old_parent = ?patch.old_parent_id,
            new_parent = patch.new_parent_id,
            old_path = %LogObscurer::path(&patch.old_path),
            new_path = %LogObscurer::path(&patch.new_path),
            subtree_folders = patch.subtree_records.len(),
            affected_media = report.affected_media_count,
            "[FolderMoveSync: Start] Applying committed folder move to snapshot and views"
        );

        let snapshot_patched = Self::patch_snapshot(patch, &ctx.state);
        Self::expand_destination_chain(patch, ctx);
        let rebased_media = Self::rebase_runtime_state(patch, &ctx.state);
        Self::rebase_remembered_folder(patch, ctx);

        StatusBarBridge::success(window, "Folder moved successfully.");

        if let Some((current_id, current_path)) = Self::resolve_reselect_target(patch, ctx) {
            tracing::info!(
                current_folder_id = current_id,
                path = %LogObscurer::path(&current_path),
                "[FolderMoveSync: Reselect] Active folder is affected by the move, reloading it"
            );
            BridgeFolders::select_folder(
                current_id,
                current_path,
                window,
                &ctx.library,
                &ctx.thumbnails,
                &ctx.settings,
                &ctx.state,
            );
        } else {
            BridgeFolders::reload_tree(window, &ctx.library, &ctx.settings, &ctx.state);
            BridgeMedia::reload(
                window,
                &ctx.library,
                &ctx.thumbnails,
                &ctx.settings,
                &ctx.state,
                false,
                true,
            );
            FolderTreeSync::scroll_to_folder(window, patch.moved_folder_id);
        }

        StatusBarBridge::update_metrics(window, &ctx.state);

        tracing::info!(
            folder_id = patch.moved_folder_id,
            snapshot_patched,
            rebased_media,
            elapsed_us = t0.elapsed().as_micros() as u64,
            "[FolderMoveSync: Done] Folder move applied to views"
        );
    }

    /// Patches the cached snapshot in place. Returns `true` if the snapshot is consistent afterwards.
    /// A missing snapshot is fine (it will be lazily loaded from the already updated database); a failed patch
    /// discards the snapshot so the next access reloads it.
    fn patch_snapshot(patch: &FolderMovePatch, state: &RwLock<UiState>) -> bool {
        let outcome = {
            let mut st = state.write();
            st.folder_hierarchy
                .as_mut()
                .map(|arc_snapshot| Arc::make_mut(arc_snapshot).apply_folder_move(patch))
        };

        match outcome {
            None => {
                tracing::debug!(
                    "[FolderMoveSync: Snapshot] No snapshot cached, it will be loaded from the updated database"
                );
                true
            }
            Some(Ok(stats)) => {
                tracing::info!(
                    removed_nodes = stats.removed_nodes,
                    inserted_nodes = stats.inserted_nodes,
                    counters_updated = stats.counters_updated,
                    old_location_in_scope = stats.old_location_in_scope,
                    new_location_in_scope = stats.new_location_in_scope,
                    "[FolderMoveSync: Snapshot] Patched SPoT snapshot surgically"
                );
                true
            }
            Some(Err(e)) => {
                tracing::warn!(
                    error = %e,
                    "[FolderMoveSync: Snapshot] Snapshot patch failed, discarding snapshot for reload from database"
                );
                state.write().invalidate_folder_hierarchy();
                false
            }
        }
    }

    /// Expands the destination folder and all its ancestors so the moved folder becomes visible.
    fn expand_destination_chain(patch: &FolderMovePatch, ctx: &FolderDialogContext) {
        let snapshot = BridgeFolders::ensure_snapshot(&ctx.library, &ctx.state);

        let mut to_expand: Vec<FolderId> = vec![patch.new_parent_id];
        let mut current = snapshot.get_node(patch.new_parent_id).and_then(|n| n.parent_id);
        while let Some(parent_id) = current {
            to_expand.push(parent_id);
            if to_expand.len() >= MAX_CHAIN_WALK {
                break;
            }
            current = snapshot.get_node(parent_id).and_then(|n| n.parent_id);
        }

        let mut st = ctx.state.write();
        for id in to_expand {
            st.expanded_folder_ids.insert(id);
        }
        BridgeFolders::persist_expanded_folders(&ctx.settings, &st.expanded_folder_ids);
    }

    /// Rewrites runtime paths that still point below the old location. Returns the number of rebased media items.
    fn rebase_runtime_state(patch: &FolderMovePatch, state: &RwLock<UiState>) -> usize {
        let subtree_ids: HashSet<FolderId> = patch.subtree_records.iter().map(|r| r.id).collect();

        let mut st = state.write();

        let rebased_current = st
            .current_folder_path
            .as_ref()
            .and_then(|p| Self::rebase_path(p, patch));
        if let Some(new_current) = rebased_current {
            st.current_folder_path = Some(new_current);
        }

        let mut rebased_media = 0usize;
        let mut rebased_dirs: HashMap<FolderId, Arc<FolderDir>> = HashMap::new();
        for item in st.all_folder_media.iter_mut() {
            if !subtree_ids.contains(&item.record.folder_id) {
                continue;
            }
            let folder_dir = match rebased_dirs.get(&item.record.folder_id) {
                Some(dir) => Some(dir.clone()),
                None => Self::rebase_path(item.folder.path(), patch).map(|new_dir| {
                    let dir = FolderDir::new(new_dir);
                    rebased_dirs.insert(item.record.folder_id, dir.clone());
                    dir
                }),
            };
            if let Some(dir) = folder_dir {
                item.set_folder(dir);
                rebased_media += 1;
            }
        }

        rebased_media
    }

    /// Keeps the remembered "last folder" setting valid if it pointed into the moved subtree.
    fn rebase_remembered_folder(patch: &FolderMovePatch, ctx: &FolderDialogContext) {
        let mut app_settings = ctx.settings.get();
        let rebased = app_settings
            .general
            .last_folder_path
            .as_ref()
            .and_then(|p| Self::rebase_path(p, patch));

        if let Some(new_last) = rebased {
            tracing::debug!(
                new_last_folder = %LogObscurer::path(&new_last),
                "[FolderMoveSync: Settings] Rebased remembered last folder path"
            );
            app_settings.general.last_folder_path = Some(new_last);
            ctx.settings.save_quiet_async(&app_settings);
        }
    }

    /// Decides whether the active folder must be reloaded: it lies inside the moved subtree (paths changed), or the
    /// recursive (virtual) view includes the old or new location (its content changed).
    fn resolve_reselect_target(patch: &FolderMovePatch, ctx: &FolderDialogContext) -> Option<(FolderId, PathBuf)> {
        let (current_id, current_path, is_virtual) = {
            let st = ctx.state.read();
            (
                st.current_folder_id?,
                st.current_folder_path.clone()?,
                st.is_virtual_folder_mode,
            )
        };

        if patch.subtree_records.iter().any(|r| r.id == current_id) {
            return Some((current_id, current_path));
        }

        if !is_virtual {
            return None;
        }

        let snapshot = BridgeFolders::ensure_snapshot(&ctx.library, &ctx.state);
        let touches_view = snapshot.is_descendant_of(patch.new_parent_id, current_id)
            || patch
                .old_parent_id
                .is_some_and(|old_parent| snapshot.is_descendant_of(old_parent, current_id));

        touches_view.then_some((current_id, current_path))
    }

    /// Rebases `path` from the old location onto the new location (None if it is not below the old location).
    fn rebase_path(path: &std::path::Path, patch: &FolderMovePatch) -> Option<PathBuf> {
        path.strip_prefix(&patch.old_path).ok().map(|suffix| {
            if suffix.as_os_str().is_empty() {
                patch.new_path.clone()
            } else {
                patch.new_path.join(suffix)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn patch() -> FolderMovePatch {
        FolderMovePatch {
            old_path: PathBuf::from("/r/a/mm"),
            new_path: PathBuf::from("/r/b/mm"),
            ..FolderMovePatch::default()
        }
    }

    #[test]
    fn rebases_paths_inside_the_moved_subtree() {
        let p = patch();
        assert_eq!(
            BridgeFolderMoveSync::rebase_path(Path::new("/r/a/mm"), &p),
            Some(PathBuf::from("/r/b/mm"))
        );
        assert_eq!(
            BridgeFolderMoveSync::rebase_path(Path::new("/r/a/mm/y/img.jpg"), &p),
            Some(PathBuf::from("/r/b/mm/y/img.jpg"))
        );
    }

    #[test]
    fn leaves_unrelated_paths_untouched() {
        let p = patch();
        assert_eq!(BridgeFolderMoveSync::rebase_path(Path::new("/r/a/mmx"), &p), None);
        assert_eq!(BridgeFolderMoveSync::rebase_path(Path::new("/r/a"), &p), None);
    }
}
