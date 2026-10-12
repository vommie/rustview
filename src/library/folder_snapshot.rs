//! Builds the in-memory Single Point of Truth (SPoT) folder hierarchy snapshot from LMDB.
//!
//! Design rules:
//! - The snapshot is scoped to the configured root directory (`LibraryDatabase::tree_root`).
//!   Without a root, or with a root that is not registered in the database, the snapshot is empty.
//! - The hierarchy is built exclusively from the persisted `parent_id` links, walking from the root
//!   downwards (BFS). Orphaned, cyclic, duplicate or out-of-scope records never appear in the tree.
//! - There is zero filesystem access in this module.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{CachedFolderNode, FolderHierarchySnapshot, FolderId};
use crate::settings::LogObscurer;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

const MAX_LOGGED_SAMPLES: usize = 5;

type NodeMap = HashMap<FolderId, CachedFolderNode>;
type PathIndex = HashMap<PathBuf, FolderId>;
type ChildLists = HashMap<FolderId, Vec<FolderId>>;

/// Diagnostic counters collected while building a snapshot.
#[derive(Debug, Default, Clone)]
pub struct SnapshotBuildStats {
    pub total_records: usize,
    pub scoped_records: usize,
    pub undecodable_records: usize,
    pub duplicate_paths: usize,
    pub orphan_nodes: usize,
    pub unreachable_nodes: usize,
    pub reachable_nodes: usize,
    pub max_depth: u32,
    pub orphan_samples: Vec<(FolderId, String)>,
}

pub struct FolderSnapshotBuilder<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderSnapshotBuilder<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Builds the snapshot in a single read transaction pass over the folders table.
    pub fn build(&self) -> Result<FolderHierarchySnapshot, LibraryError> {
        let t0 = Instant::now();

        let Some(root_path) = self.db.tree_root() else {
            tracing::info!(
                "[FolderSnapshot: Empty] No root directory registered for the folder tree. Returning empty snapshot."
            );
            return Ok(FolderHierarchySnapshot::default());
        };

        let root_str = root_path.to_string_lossy().to_string();
        let prefix = scope_prefix(&root_str);
        let mut stats = SnapshotBuildStats::default();

        let t_read = Instant::now();
        let (mut nodes, path_index) = self.read_scoped_nodes(&root_str, &prefix, &mut stats)?;
        let read_us = t_read.elapsed().as_micros();

        let Some(root_id) = path_index.get(&root_path).copied() else {
            tracing::info!(
                root = %LogObscurer::path(&root_path),
                total_records = stats.total_records,
                scoped_records = stats.scoped_records,
                elapsed_us = t0.elapsed().as_micros(),
                "[FolderSnapshot: RootUnregistered] Root directory is not registered in the folder database yet. Returning empty snapshot. Run 'Refresh Tree (Full)' to build the tree."
            );
            return Ok(FolderHierarchySnapshot::default());
        };

        let t_link = Instant::now();
        let mut child_lists = link_children(&nodes, root_id, &mut stats);
        let (reachable, max_depth) = collect_reachable(root_id, &child_lists);
        stats.reachable_nodes = reachable.len();
        stats.unreachable_nodes = nodes.len().saturating_sub(reachable.len());
        stats.max_depth = max_depth;
        let link_us = t_link.elapsed().as_micros();

        let t_sort = Instant::now();
        child_lists.retain(|pid, _| reachable.contains(pid));
        for list in child_lists.values_mut() {
            sort_children_by_name(list, &nodes);
        }
        let sort_us = t_sort.elapsed().as_micros();

        let mut final_nodes: NodeMap = HashMap::with_capacity(reachable.len());
        for id in &reachable {
            if let Some(mut node) = nodes.remove(id) {
                node.children = child_lists.get(id).cloned().unwrap_or_default();
                final_nodes.insert(*id, node);
            }
        }

        let mut children_map: HashMap<Option<FolderId>, Vec<FolderId>> =
            HashMap::with_capacity(child_lists.len() + 1);
        for (pid, list) in child_lists {
            children_map.insert(Some(pid), list);
        }
        children_map.insert(None, vec![root_id]);

        let mut path_index = path_index;
        path_index.retain(|_, id| final_nodes.contains_key(id));

        Self::log_stats(&stats, root_id, read_us, link_us, sort_us, t0);

        Ok(FolderHierarchySnapshot {
            nodes: final_nodes,
            children_map,
            path_index,
            root_ids: vec![root_id],
        })
    }

    /// Single pass over the folders table: keeps only in-scope records and drops duplicate paths.
    fn read_scoped_nodes(
        &self,
        root_str: &str,
        prefix: &str,
        stats: &mut SnapshotBuildStats,
    ) -> Result<(NodeMap, PathIndex), LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut nodes: NodeMap = HashMap::new();
        let mut path_index: PathIndex = HashMap::new();

        for entry in self.db.folders.iter(&rtxn)? {
            let (id, rec) = match entry {
                Ok(v) => v,
                Err(e) => {
                    stats.undecodable_records += 1;
                    if stats.undecodable_records <= MAX_LOGGED_SAMPLES {
                        tracing::warn!(
                            error = %e,
                            "[FolderSnapshot: Undecodable] Skipping undecodable folder record"
                        );
                    }
                    continue;
                }
            };

            stats.total_records += 1;

            if !is_in_scope(&rec.path, root_str, prefix) {
                continue;
            }
            stats.scoped_records += 1;

            let path_buf = PathBuf::from(&rec.path);
            if let Some(&kept_id) = path_index.get(&path_buf) {
                stats.duplicate_paths += 1;
                if stats.duplicate_paths <= MAX_LOGGED_SAMPLES {
                    tracing::warn!(
                        dropped_id = id,
                        kept_id,
                        path = %LogObscurer::path(&path_buf),
                        "[FolderSnapshot: DuplicatePath] Multiple folder records share one path. Keeping the lowest id in the tree."
                    );
                }
                continue;
            }

            path_index.insert(path_buf.clone(), id);
            nodes.insert(
                id,
                CachedFolderNode {
                    id,
                    parent_id: rec.parent_id,
                    name: rec.name,
                    path: path_buf,
                    depth: rec.depth,
                    is_hidden: rec.is_hidden,
                    is_deleted: rec.is_deleted,
                    subfolder_count: rec.subfolder_count,
                    media_count: rec.media_count,
                    recursive_media_count: rec.recursive_media_count,
                    children: Vec::new(),
                },
            );
        }

        Ok((nodes, path_index))
    }

    fn log_stats(
        stats: &SnapshotBuildStats,
        root_id: FolderId,
        read_us: u128,
        link_us: u128,
        sort_us: u128,
        t0: Instant,
    ) {
        let elapsed_us = t0.elapsed().as_micros();

        tracing::debug!(
            total_nodes = stats.reachable_nodes,
            root_nodes = 1usize,
            elapsed_us,
            "[FolderHierarchy: Snapshot] Loaded complete in-memory SPoT folder hierarchy snapshot"
        );

        tracing::info!(
            root_id,
            total_records = stats.total_records,
            scoped_records = stats.scoped_records,
            reachable_nodes = stats.reachable_nodes,
            max_depth = stats.max_depth,
            orphan_nodes = stats.orphan_nodes,
            unreachable_nodes = stats.unreachable_nodes,
            duplicate_paths = stats.duplicate_paths,
            undecodable_records = stats.undecodable_records,
            read_us = read_us as u64,
            link_us = link_us as u64,
            sort_us = sort_us as u64,
            elapsed_us = elapsed_us as u64,
            "[FolderSnapshot: Built] Folder hierarchy snapshot built from database"
        );

        if stats.orphan_nodes > 0 || stats.unreachable_nodes > 0 {
            tracing::warn!(
                orphan_nodes = stats.orphan_nodes,
                unreachable_nodes = stats.unreachable_nodes,
                "[FolderSnapshot: Orphans] Folder records below the root are not linked to the root via parent_id and are hidden from the tree. Run 'Refresh Tree (Full)' to rebuild the hierarchy."
            );
            for (id, path) in &stats.orphan_samples {
                tracing::warn!(
                    folder_id = *id,
                    path = %path,
                    "[FolderSnapshot: OrphanSample] Orphaned folder record (no valid parent link)"
                );
            }
        }
    }
}

/// Returns `true` if `path` equals the root or lies strictly below it (string prefix at a separator boundary).
pub(crate) fn is_in_scope(path: &str, root: &str, prefix: &str) -> bool {
    path == root || path.starts_with(prefix)
}

/// Builds the prefix used for scope checks (root path with exactly one trailing slash).
pub(crate) fn scope_prefix(root: &str) -> String {
    if root.ends_with('/') {
        root.to_string()
    } else {
        format!("{}/", root)
    }
}

/// Groups scoped nodes by their persisted parent. Nodes without a valid in-scope parent are counted as orphans.
fn link_children(nodes: &NodeMap, root_id: FolderId, stats: &mut SnapshotBuildStats) -> ChildLists {
    let mut lists: ChildLists = HashMap::new();

    for (&id, node) in nodes {
        if id == root_id {
            continue;
        }

        match node.parent_id {
            Some(pid) if pid != id && nodes.contains_key(&pid) => {
                lists.entry(pid).or_default().push(id);
            }
            _ => {
                stats.orphan_nodes += 1;
                if stats.orphan_samples.len() < MAX_LOGGED_SAMPLES {
                    stats
                        .orphan_samples
                        .push((id, LogObscurer::path(&node.path).to_string()));
                }
            }
        }
    }

    lists
}

/// BFS from the root over the child lists. Returns all reachable ids and the maximum relative depth.
fn collect_reachable(root_id: FolderId, lists: &ChildLists) -> (HashSet<FolderId>, u32) {
    let mut reachable: HashSet<FolderId> = HashSet::with_capacity(lists.len() + 1);
    let mut queue: VecDeque<(FolderId, u32)> = VecDeque::new();
    let mut max_depth = 0u32;

    reachable.insert(root_id);
    queue.push_back((root_id, 0));

    while let Some((id, depth)) = queue.pop_front() {
        max_depth = max_depth.max(depth);
        if let Some(children) = lists.get(&id) {
            for &child in children {
                if reachable.insert(child) {
                    queue.push_back((child, depth + 1));
                }
            }
        }
    }

    (reachable, max_depth)
}

/// Sorts siblings case-insensitively by name with the id as a deterministic tie-breaker.
fn sort_children_by_name(list: &mut [FolderId], nodes: &NodeMap) {
    list.sort_by_cached_key(|id| {
        (
            nodes
                .get(id)
                .map(|n| n.name.to_lowercase())
                .unwrap_or_default(),
            *id,
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_matches_root_and_descendants_only() {
        let root = "/home/user/pics";
        let prefix = scope_prefix(root);
        assert!(is_in_scope("/home/user/pics", root, &prefix));
        assert!(is_in_scope("/home/user/pics/a/b", root, &prefix));
        assert!(!is_in_scope("/home/user/pics_2", root, &prefix));
        assert!(!is_in_scope("/home/user", root, &prefix));
    }

    #[test]
    fn scope_prefix_handles_filesystem_root() {
        assert_eq!(scope_prefix("/"), "/");
        assert_eq!(scope_prefix("/a"), "/a/");
    }
}
