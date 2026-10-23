//! Surgical in-memory updates of the `SPoT` folder hierarchy snapshot.
//!
//! A folder move only touches the moved subtree and the counters of the two parent chains. Instead of reloading all
//! folder records, the moved subtree is detached from its old position and re-attached from the final database
//! records returned by the move, so the snapshot always mirrors the database exactly.

use super::folder_move::FolderMovePatch;
use super::models::{CachedFolderNode, FolderHierarchySnapshot, FolderId, FolderRecord};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::PathBuf;

/// Counters describing what a patch changed (for diagnostics).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SnapshotPatchStats {
    pub removed_nodes: usize,
    pub inserted_nodes: usize,
    pub counters_updated: usize,
    pub old_location_in_scope: bool,
    pub new_location_in_scope: bool,
}

/// Reasons why a snapshot cannot be patched. The caller must invalidate the snapshot in that case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotPatchError {
    RootMoveRejected(FolderId),
    MissingMovedRecord(FolderId),
}

impl fmt::Display for SnapshotPatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMoveRejected(id) => {
                write!(f, "folder {id} is the tree root and cannot be moved")
            }
            Self::MissingMovedRecord(id) => {
                write!(f, "moved folder {id} is missing from the patch records")
            }
        }
    }
}

impl std::error::Error for SnapshotPatchError {}

/// Sibling ordering key. Must mirror the ordering used by `FolderSnapshotBuilder` (case-insensitive name, then id).
fn sibling_sort_key(name: &str, id: FolderId) -> (String, FolderId) {
    (name.to_lowercase(), id)
}

impl FolderHierarchySnapshot {
    /// Applies a folder move to the snapshot. On error the snapshot may be partially modified and must be discarded.
    pub fn apply_folder_move(&mut self, patch: &FolderMovePatch) -> Result<SnapshotPatchStats, SnapshotPatchError> {
        if self.root_ids.contains(&patch.moved_folder_id) {
            return Err(SnapshotPatchError::RootMoveRejected(patch.moved_folder_id));
        }

        let mut stats = SnapshotPatchStats {
            old_location_in_scope: self.nodes.contains_key(&patch.moved_folder_id),
            new_location_in_scope: self.nodes.contains_key(&patch.new_parent_id),
            ..SnapshotPatchStats::default()
        };

        stats.removed_nodes = self.detach_subtree(patch.moved_folder_id);

        if stats.new_location_in_scope {
            stats.inserted_nodes = self.attach_subtree(patch)?;
        }

        stats.counters_updated = self.refresh_counters(&patch.ancestor_records);
        Ok(stats)
    }

    /// Adds freshly created folders (`records` from the top of a created path down) to the snapshot. Records that are known
    /// already only refresh their flags; a record whose parent is not in the snapshot (outside the tree's root) is skipped.
    /// Returns the number of inserted nodes.
    pub fn apply_folders_created(&mut self, records: &[FolderRecord]) -> usize {
        let mut inserted = 0usize;
        for rec in records {
            if let Some(node) = self.nodes.get_mut(&rec.id) {
                node.is_hidden = rec.is_hidden;
                node.is_deleted = rec.is_deleted;
                continue;
            }
            let Some(parent_id) = rec.parent_id.filter(|pid| self.nodes.contains_key(pid)) else {
                continue;
            };
            let path = PathBuf::from(&rec.path);
            self.path_index.insert(path.clone(), rec.id);
            self.nodes.insert(
                rec.id,
                CachedFolderNode {
                    id: rec.id,
                    parent_id: rec.parent_id,
                    name: rec.name.clone(),
                    path,
                    depth: rec.depth,
                    is_hidden: rec.is_hidden,
                    is_deleted: rec.is_deleted,
                    subfolder_count: 0,
                    media_count: rec.media_count,
                    recursive_media_count: rec.recursive_media_count,
                    children: Vec::new(),
                },
            );
            self.insert_child_sorted(parent_id, rec.id);
            if let Some(parent) = self.nodes.get_mut(&parent_id) {
                parent.subfolder_count = parent.children.len() as u32;
            }
            inserted += 1;
        }
        inserted
    }

    /// Removes a subtree (nodes, path index, child links) from the snapshot. Returns the number of removed nodes.
    fn detach_subtree(&mut self, root: FolderId) -> usize {
        if !self.nodes.contains_key(&root) {
            return 0;
        }

        let subtree_ids = self.collect_subtree_ids(root);
        if let Some(parent_id) = self.nodes.get(&root).and_then(|n| n.parent_id) {
            self.remove_child_link(parent_id, root);
        }

        let mut removed = 0usize;
        for id in subtree_ids {
            self.children_map.remove(&Some(id));
            if let Some(node) = self.nodes.remove(&id) {
                if self.path_index.get(&node.path) == Some(&id) {
                    self.path_index.remove(&node.path);
                }
                removed += 1;
            }
        }
        removed
    }

    fn remove_child_link(&mut self, parent: FolderId, child: FolderId) {
        let now_empty = match self.children_map.get_mut(&Some(parent)) {
            Some(list) => {
                list.retain(|&c| c != child);
                list.is_empty()
            }
            None => false,
        };
        if now_empty {
            self.children_map.remove(&Some(parent));
        }

        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            parent_node.children.retain(|&c| c != child);
        }
    }

    /// Rebuilds the moved subtree from the final database records and links it below the new parent.
    /// Only records reachable from the moved folder via `parent_id` are inserted (mirrors the snapshot builder).
    fn attach_subtree(&mut self, patch: &FolderMovePatch) -> Result<usize, SnapshotPatchError> {
        let by_id: HashMap<FolderId, &FolderRecord> = patch.subtree_records.iter().map(|r| (r.id, r)).collect();
        if !by_id.contains_key(&patch.moved_folder_id) {
            return Err(SnapshotPatchError::MissingMovedRecord(patch.moved_folder_id));
        }

        let mut children_of: HashMap<FolderId, Vec<FolderId>> = HashMap::new();
        for rec in &patch.subtree_records {
            if rec.id == patch.moved_folder_id {
                continue;
            }
            if let Some(pid) = rec.parent_id
                && by_id.contains_key(&pid)
            {
                children_of.entry(pid).or_default().push(rec.id);
            }
        }
        for list in children_of.values_mut() {
            list.sort_by_cached_key(|id| sibling_sort_key(&by_id[id].name, *id));
        }

        let reachable = Self::reachable_record_ids(patch.moved_folder_id, &children_of);

        for id in &reachable {
            let rec = by_id[id];
            let children = children_of.get(id).cloned().unwrap_or_default();
            let path = PathBuf::from(&rec.path);

            if self.path_index.insert(path.clone(), *id).is_some() {
                tracing::warn!(
                    folder_id = *id,
                    "[FolderSnapshot: PatchCollision] Path index entry was overwritten while attaching moved subtree"
                );
            }
            if !children.is_empty() {
                self.children_map.insert(Some(*id), children.clone());
            }
            self.nodes.insert(
                *id,
                CachedFolderNode {
                    id: *id,
                    parent_id: rec.parent_id,
                    name: rec.name.clone(),
                    path,
                    depth: rec.depth,
                    is_hidden: rec.is_hidden,
                    is_deleted: rec.is_deleted,
                    subfolder_count: rec.subfolder_count,
                    media_count: rec.media_count,
                    recursive_media_count: rec.recursive_media_count,
                    children,
                },
            );
        }

        self.insert_child_sorted(patch.new_parent_id, patch.moved_folder_id);
        Ok(reachable.len())
    }

    /// BFS over record children starting at `root` (the root is always included first).
    fn reachable_record_ids(root: FolderId, children_of: &HashMap<FolderId, Vec<FolderId>>) -> Vec<FolderId> {
        let mut order = Vec::new();
        let mut visited: HashSet<FolderId> = HashSet::new();
        let mut queue: VecDeque<FolderId> = VecDeque::new();

        visited.insert(root);
        queue.push_back(root);

        while let Some(id) = queue.pop_front() {
            order.push(id);
            if let Some(children) = children_of.get(&id) {
                for &child in children {
                    if visited.insert(child) {
                        queue.push_back(child);
                    }
                }
            }
        }

        order
    }

    /// Inserts `child` into the sorted children list of `parent` (node list and children map stay in sync).
    fn insert_child_sorted(&mut self, parent: FolderId, child: FolderId) {
        let Some(child_key) = self.nodes.get(&child).map(|n| sibling_sort_key(&n.name, n.id)) else {
            return;
        };

        let position = match self.nodes.get(&parent) {
            Some(parent_node) => parent_node.children.partition_point(|cid| {
                self.nodes
                    .get(cid)
                    .map(|n| sibling_sort_key(&n.name, n.id))
                    .is_some_and(|key| key < child_key)
            }),
            None => return,
        };

        let updated_children = match self.nodes.get_mut(&parent) {
            Some(parent_node) => {
                if !parent_node.children.contains(&child) {
                    parent_node.children.insert(position, child);
                }
                parent_node.children.clone()
            }
            None => return,
        };
        self.children_map.insert(Some(parent), updated_children);
    }

    /// Copies counters of the given records onto matching snapshot nodes. Returns the number of updated nodes.
    fn refresh_counters(&mut self, records: &[FolderRecord]) -> usize {
        let mut updated = 0usize;
        for rec in records {
            if let Some(node) = self.nodes.get_mut(&rec.id) {
                node.subfolder_count = rec.subfolder_count;
                node.media_count = rec.media_count;
                node.recursive_media_count = rec.recursive_media_count;
                updated += 1;
            }
        }
        updated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: FolderId, parent: Option<FolderId>, name: &str, path: &str) -> FolderRecord {
        FolderRecord {
            id,
            parent_id: parent,
            name: name.to_string(),
            path: path.to_string(),
            depth: 0,
            is_hidden: false,
            is_deleted: false,
            subfolder_count: 0,
            media_count: 0,
            recursive_media_count: 0,
            added_at: 0,
            last_scanned_at: 0,
        }
    }

    /// Builds a snapshot with root id 1 from records listed in sibling-sorted order.
    fn snapshot_from(records: &[FolderRecord]) -> FolderHierarchySnapshot {
        let mut snap = FolderHierarchySnapshot::default();
        for r in records {
            snap.nodes.insert(
                r.id,
                CachedFolderNode {
                    id: r.id,
                    parent_id: r.parent_id,
                    name: r.name.clone(),
                    path: PathBuf::from(&r.path),
                    depth: r.depth,
                    is_hidden: false,
                    is_deleted: false,
                    subfolder_count: 0,
                    media_count: 0,
                    recursive_media_count: 0,
                    children: Vec::new(),
                },
            );
            snap.path_index.insert(PathBuf::from(&r.path), r.id);
        }
        for r in records {
            if let Some(p) = r.parent_id
                && let Some(parent_node) = snap.nodes.get_mut(&p)
            {
                parent_node.children.push(r.id);
            }
        }
        for (id, node) in snap.nodes.iter() {
            if !node.children.is_empty() {
                snap.children_map.insert(Some(*id), node.children.clone());
            }
        }
        snap.children_map.insert(None, vec![1]);
        snap.root_ids = vec![1];
        snap
    }

    fn base_records() -> Vec<FolderRecord> {
        vec![
            rec(1, None, "r", "/r"),
            rec(2, Some(1), "a", "/r/a"),
            rec(3, Some(1), "b", "/r/b"),
            rec(4, Some(2), "mm", "/r/a/mm"),
            rec(5, Some(4), "y", "/r/a/mm/y"),
            rec(6, Some(3), "aa", "/r/b/aa"),
            rec(7, Some(3), "zz", "/r/b/zz"),
        ]
    }

    fn move_patch() -> FolderMovePatch {
        FolderMovePatch {
            moved_folder_id: 4,
            old_parent_id: Some(2),
            new_parent_id: 3,
            old_path: PathBuf::from("/r/a/mm"),
            new_path: PathBuf::from("/r/b/mm"),
            subtree_records: vec![rec(4, Some(3), "mm", "/r/b/mm"), rec(5, Some(4), "y", "/r/b/mm/y")],
            ancestor_records: Vec::new(),
        }
    }

    #[test]
    fn move_reparents_subtree_and_rekeys_paths() {
        let mut snap = snapshot_from(&base_records());
        let stats = snap.apply_folder_move(&move_patch()).expect("patch must apply");

        assert_eq!(stats.removed_nodes, 2);
        assert_eq!(stats.inserted_nodes, 2);
        assert!(snap.get_children(Some(2)).is_empty());
        assert_eq!(snap.nodes[&4].parent_id, Some(3));
        assert!(snap.path_index.contains_key(&PathBuf::from("/r/b/mm/y")));
        assert!(!snap.path_index.contains_key(&PathBuf::from("/r/a/mm")));
        assert!(snap.is_descendant_of(5, 3));
        assert!(!snap.is_descendant_of(5, 2));
    }

    #[test]
    fn moved_folder_is_inserted_in_sorted_position() {
        let mut snap = snapshot_from(&base_records());
        snap.apply_folder_move(&move_patch()).expect("patch must apply");

        // aa (6) < mm (4) < zz (7)
        assert_eq!(snap.get_children(Some(3)), &[6, 4, 7]);
        assert_eq!(snap.nodes[&3].children, vec![6, 4, 7]);
    }

    #[test]
    fn moving_the_root_is_rejected() {
        let mut snap = snapshot_from(&base_records());
        let patch = FolderMovePatch {
            moved_folder_id: 1,
            new_parent_id: 3,
            ..FolderMovePatch::default()
        };
        assert_eq!(
            snap.apply_folder_move(&patch),
            Err(SnapshotPatchError::RootMoveRejected(1))
        );
    }

    #[test]
    fn counters_of_ancestors_are_refreshed() {
        let mut snap = snapshot_from(&base_records());
        let mut patch = move_patch();
        let mut parent = rec(3, Some(1), "b", "/r/b");
        parent.subfolder_count = 3;
        parent.recursive_media_count = 9;
        patch.ancestor_records = vec![parent];

        let stats = snap.apply_folder_move(&patch).expect("patch must apply");
        assert_eq!(stats.counters_updated, 1);
        assert_eq!(snap.nodes[&3].subfolder_count, 3);
        assert_eq!(snap.nodes[&3].recursive_media_count, 9);
    }
}
