//! Surgical synchronization of the folder tree Slint model.
//!
//! Instead of replacing the whole model on every update, the rows projected from the `SPoT` snapshot are reconciled
//! against the existing `VecModel` using keyed row operations (remove / move / insert / update in place).
//! If the model is not a `VecModel` or the result does not match the desired rows, the model is replaced as a
//! safety net so the view can never end up inconsistent.

use crate::ui::browse_folders::BrowseFolders;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::folder_tree::projection::FolderProjection;
use crate::ui::state::UiState;
use crate::ui::{FolderNodeView, MainWindow};
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::FolderId;
use rustview_core::settings::SettingsManager;
use rustview_core::util::cast::{SaturatingAs, WrappingAs};
use slint::{Model, ModelRc, VecModel};
use std::collections::HashSet;
use std::time::Instant;

/// Counters describing what a row synchronization did (for diagnostics and tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RowSyncStats {
    pub inserted: usize,
    pub removed: usize,
    pub moved: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub full_replace: bool,
}

pub struct FolderTreeSync;

impl FolderTreeSync {
    /// Projects the tree rows from the in-memory `SPoT` snapshot (zero filesystem I/O) and syncs them into the window.
    pub fn refresh(window: &MainWindow, library: &Library, settings: &SettingsManager, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let app_settings = settings.get();

        let snapshot = BridgeFolders::ensure_snapshot(library, state);

        let current_sel = state.read().current_folder_id;
        let expanded_ids = state.read().expanded_folder_ids.clone();

        let rows = snapshot.project_tree(
            &expanded_ids,
            current_sel,
            app_settings.folders.show_deleted_folders,
            app_settings.folders.show_folder_metrics,
            app_settings.folders.show_hidden_folders,
        );

        let row_count = rows.len();
        let stats = Self::apply_rows(window, rows);

        // The Browse view lists subfolders of the open folder: they follow the tree.
        BrowseFolders::on_tree_changed(window, library, settings, state);

        tracing::debug!(
            rows = row_count,
            snapshot_nodes = snapshot.nodes.len(),
            inserted = stats.inserted,
            removed = stats.removed,
            moved = stats.moved,
            updated = stats.updated,
            unchanged = stats.unchanged,
            full_replace = stats.full_replace,
            elapsed_us = t0.elapsed().as_micros().saturating_as::<u64>(),
            "[FolderTree: Reload] Folder tree model refreshed from SPoT snapshot"
        );
    }

    /// Applies the desired rows to the window's folder model with minimal row operations.
    pub fn apply_rows(window: &MainWindow, rows: Vec<FolderNodeView>) -> RowSyncStats {
        let model_rc = window.get_folders();

        let Some(vec_model) = model_rc.as_any().downcast_ref::<VecModel<FolderNodeView>>() else {
            tracing::debug!("[FolderTree: Sync] Window model is not a VecModel, replacing model");
            Self::replace_model(window, rows);
            return RowSyncStats {
                full_replace: true,
                ..RowSyncStats::default()
            };
        };

        let mut stats = Self::reconcile(vec_model, &rows);

        if !Self::ids_match(vec_model, &rows) {
            tracing::warn!(
                "[FolderTree: Sync] Row reconciliation result differs from desired rows, replacing model as safety net"
            );
            Self::replace_model(window, rows);
            stats.full_replace = true;
        }

        stats
    }

    /// Scrolls the folder tree to the row of `folder_id`. Returns `false` if the folder has no visible row.
    pub fn scroll_to_folder(window: &MainWindow, folder_id: FolderId) -> bool {
        let model = window.get_folders();
        for idx in 0..model.row_count() {
            if let Some(row) = model.row_data(idx)
                && i64::from(row.id) == folder_id.wrapping_as::<i64>()
            {
                window.invoke_scroll_folder_to_row(idx.saturating_as::<i32>());
                return true;
            }
        }
        false
    }

    fn replace_model(window: &MainWindow, rows: Vec<FolderNodeView>) {
        window.set_folders(ModelRc::new(VecModel::from(rows)));
    }

    /// Keyed reconciliation: removes unknown rows, moves/inserts rows into place and updates changed rows in place.
    pub(crate) fn reconcile(model: &VecModel<FolderNodeView>, rows: &[FolderNodeView]) -> RowSyncStats {
        let mut stats = RowSyncStats::default();
        let wanted: HashSet<i32> = rows.iter().map(|r| r.id).collect();

        let mut current: Vec<i32> = (0..model.row_count())
            .filter_map(|i| model.row_data(i).map(|r| r.id))
            .collect();

        // Phase 1: remove rows that no longer exist (back to front keeps indices valid).
        for idx in (0..current.len()).rev() {
            if !wanted.contains(&current[idx]) {
                let _removed = model.remove(idx);
                current.remove(idx);
                stats.removed += 1;
            }
        }

        // Phase 2: walk the desired order and make every position match.
        for (target, row) in rows.iter().enumerate() {
            if current.get(target) == Some(&row.id) {
                Self::update_row_if_changed(model, target, row, &mut stats);
                continue;
            }

            let existing_offset = current.iter().skip(target).position(|id| *id == row.id);

            match existing_offset {
                Some(offset) => {
                    let from = target + offset;
                    let _moved_out = model.remove(from);
                    current.remove(from);
                    stats.moved += 1;
                }
                None => stats.inserted += 1,
            }

            model.insert(target, row.clone());
            current.insert(target, row.id);
        }

        // Phase 3: drop trailing leftovers (only possible with duplicate ids).
        while current.len() > rows.len() {
            let last = current.len() - 1;
            let _trailing = model.remove(last);
            current.pop();
            stats.removed += 1;
        }

        stats
    }

    fn update_row_if_changed(
        model: &VecModel<FolderNodeView>,
        idx: usize,
        row: &FolderNodeView,
        stats: &mut RowSyncStats,
    ) {
        match model.row_data(idx) {
            Some(existing) if Self::rows_equal(&existing, row) => stats.unchanged += 1,
            _ => {
                model.set_row_data(idx, row.clone());
                stats.updated += 1;
            }
        }
    }

    fn rows_equal(a: &FolderNodeView, b: &FolderNodeView) -> bool {
        a.id == b.id
            && a.name == b.name
            && a.depth == b.depth
            && a.path == b.path
            && a.has_children == b.has_children
            && a.is_expanded == b.is_expanded
            && a.is_selected == b.is_selected
            && a.is_deleted == b.is_deleted
            && a.subfolder_count == b.subfolder_count
            && a.file_count == b.file_count
    }

    fn ids_match(model: &VecModel<FolderNodeView>, rows: &[FolderNodeView]) -> bool {
        model.row_count() == rows.len()
            && rows
                .iter()
                .enumerate()
                .all(|(i, row)| model.row_data(i).is_some_and(|r| r.id == row.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i32, depth: i32) -> FolderNodeView {
        FolderNodeView {
            id,
            name: format!("folder{id}").into(),
            depth,
            path: format!("/r/{id}").into(),
            has_children: false,
            is_expanded: false,
            is_selected: false,
            is_deleted: false,
            subfolder_count: 0,
            file_count: 0,
        }
    }

    fn model_ids(model: &VecModel<FolderNodeView>) -> Vec<i32> {
        (0..model.row_count())
            .filter_map(|i| model.row_data(i).map(|r| r.id))
            .collect()
    }

    #[test]
    fn moving_a_subtree_uses_move_operations_only() {
        let model = VecModel::from(vec![row(1, 0), row(2, 1), row(4, 2), row(5, 3), row(3, 1)]);
        let desired = vec![row(1, 0), row(2, 1), row(3, 1), row(4, 2), row(5, 3)];

        let stats = FolderTreeSync::reconcile(&model, &desired);

        assert_eq!(model_ids(&model), vec![1, 2, 3, 4, 5]);
        assert_eq!(stats.moved, 1);
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.removed, 0);
    }

    #[test]
    fn inserts_removes_and_updates_rows() {
        let model = VecModel::from(vec![row(1, 0), row(2, 1), row(3, 1)]);
        let mut changed = row(1, 0);
        changed.is_expanded = true;
        let desired = vec![changed, row(3, 1), row(9, 1)];

        let stats = FolderTreeSync::reconcile(&model, &desired);

        assert_eq!(model_ids(&model), vec![1, 3, 9]);
        assert_eq!(stats.removed, 1);
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.updated, 1);
        assert_eq!(stats.unchanged, 1);
        assert!(FolderTreeSync::ids_match(&model, &desired));
    }

    #[test]
    fn identical_rows_cause_no_operations() {
        let model = VecModel::from(vec![row(1, 0), row(2, 1)]);
        let desired = vec![row(1, 0), row(2, 1)];

        let stats = FolderTreeSync::reconcile(&model, &desired);

        assert_eq!(stats.unchanged, 2);
        assert_eq!(stats.updated + stats.inserted + stats.removed + stats.moved, 0);
    }
}
