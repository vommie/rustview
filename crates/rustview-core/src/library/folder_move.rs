//! Atomic folder move (reparenting): staged LMDB changes, physical rename, commit with rollback.
//!
//! Ordering guarantees:
//! 1. All database changes are staged inside one write transaction (nothing is visible yet).
//! 2. The physical `rename` is executed. If it fails, the transaction is dropped (aborted) and nothing changed.
//! 3. The transaction is committed. If the commit fails, the rename is reverted on a best-effort basis.
//!
//! The returned `FolderMovePatch` contains the final, updated database records so the in-memory SPoT snapshot can be
//! patched surgically (see `folder_snapshot_patch.rs`) instead of being rebuilt.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folder_walker::read_dot_hidden_entries;
use super::models::{FolderId, FolderRecord};
use crate::settings::LogObscurer;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Safety limit for walking parent chains (protects against corrupted cyclic parent links).
const MAX_ANCESTOR_WALK: usize = 100_000;

/// Final database state after a successful move, used to patch the in-memory snapshot.
#[derive(Debug, Clone, Default)]
pub struct FolderMovePatch {
    pub moved_folder_id: FolderId,
    pub old_parent_id: Option<FolderId>,
    pub new_parent_id: FolderId,
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    /// Updated records of the moved folder and all folders below it (paths, depth, hidden flags rewritten).
    pub subtree_records: Vec<FolderRecord>,
    /// Updated records of the old and new parent chains (subfolder and recursive media counters).
    pub ancestor_records: Vec<FolderRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct FolderMoveReport {
    pub moved_folder_id: FolderId,
    pub affected_folders_count: usize,
    pub affected_media_count: usize,
    pub patch: FolderMovePatch,
}

/// Validated, immutable description of a requested move.
struct MoveContext {
    folder_id: FolderId,
    new_parent_id: FolderId,
    record: FolderRecord,
    parent_record: FolderRecord,
    src_path: PathBuf,
    dst_parent_path: PathBuf,
    dst_path: PathBuf,
    folder_name: String,
}

pub struct FolderMoveExecutor<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderMoveExecutor<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Moves `folder_id` (including its subtree) below `new_parent_id` on disk and in the database.
    pub fn execute(&self, folder_id: FolderId, new_parent_id: FolderId) -> Result<FolderMoveReport, LibraryError> {
        let t0 = Instant::now();
        let mut wtxn = self.db.env.write_txn()?;

        let mv = self.load_and_validate(&wtxn, folder_id, new_parent_id)?;

        tracing::info!(
            folder = %LogObscurer::folder(Some(folder_id), &mv.src_path),
            new_parent = %LogObscurer::folder(Some(new_parent_id), &mv.dst_parent_path),
            new_path = %LogObscurer::path(&mv.dst_path),
            "[FolderMove: Start] Staging folder move in database transaction"
        );

        let old_chain = self.ancestor_chain(&wtxn, mv.record.parent_id)?;
        let new_chain = self.ancestor_chain(&wtxn, Some(new_parent_id))?;

        let subtree = self.collect_subtree(&wtxn, &mv)?;
        let subtree_count = subtree.len();
        let (updated_subtree, affected_media_count) = self.rewrite_subtree(&mut wtxn, &mv, subtree)?;

        self.relink_parents(&mut wtxn, &mv)?;

        let moved_recursive = i64::from(mv.record.recursive_media_count);
        self.adjust_recursive_counts(&mut wtxn, &old_chain, -moved_recursive)?;
        self.adjust_recursive_counts(&mut wtxn, &new_chain, moved_recursive)?;
        let ancestor_records = self.collect_ancestor_records(&wtxn, &old_chain, &new_chain)?;

        tracing::debug!(
            folder_id,
            subtree_folders = subtree_count,
            affected_media_count,
            old_chain_len = old_chain.len(),
            new_chain_len = new_chain.len(),
            moved_recursive_media = moved_recursive,
            "[FolderMove: Staged] Database changes staged, executing physical rename"
        );

        Self::rename_on_disk(&mv.src_path, &mv.dst_path)?;
        self.commit_or_revert(wtxn, &mv)?;

        tracing::info!(
            folder_id,
            new_path = %LogObscurer::path(&mv.dst_path),
            subtree_folders = subtree_count,
            affected_media_count,
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "[FolderMove: Committed] Folder move committed to LMDB"
        );

        Ok(FolderMoveReport {
            moved_folder_id: folder_id,
            affected_folders_count: subtree_count,
            affected_media_count,
            patch: FolderMovePatch {
                moved_folder_id: folder_id,
                old_parent_id: mv.record.parent_id,
                new_parent_id,
                old_path: mv.src_path,
                new_path: mv.dst_path,
                subtree_records: updated_subtree,
                ancestor_records,
            },
        })
    }

    fn rejected(message: &str) -> LibraryError {
        LibraryError::FileOperationRenameFailed(message.to_string())
    }

    /// Loads both records and validates the move against database and filesystem state.
    fn load_and_validate(
        &self,
        rtxn: &heed::RoTxn,
        folder_id: FolderId,
        new_parent_id: FolderId,
    ) -> Result<MoveContext, LibraryError> {
        if folder_id == new_parent_id {
            return Err(Self::rejected("Cannot move a folder into itself"));
        }

        let record = self
            .db
            .folders
            .get(rtxn, &folder_id)?
            .ok_or(LibraryError::FolderNotFound(folder_id))?;
        let parent_record = self
            .db
            .folders
            .get(rtxn, &new_parent_id)?
            .ok_or(LibraryError::FolderNotFound(new_parent_id))?;

        let src_path = PathBuf::from(&record.path);
        let dst_parent_path = PathBuf::from(&parent_record.path);

        let folder_name_os = src_path
            .file_name()
            .ok_or_else(|| Self::rejected("Invalid source folder name"))?;
        let folder_name = folder_name_os.to_string_lossy().to_string();
        let dst_path = dst_parent_path.join(folder_name_os);

        if dst_parent_path.starts_with(&src_path) {
            return Err(Self::rejected(
                "Cannot move a folder into itself or its own subdirectories",
            ));
        }
        if src_path == dst_path {
            return Err(Self::rejected("The folder is already located in the target folder"));
        }
        if !src_path.is_dir() {
            return Err(Self::rejected("The source folder does not exist on disk"));
        }
        if !dst_parent_path.is_dir() {
            return Err(Self::rejected("The destination folder does not exist on disk"));
        }
        if dst_path.exists() {
            return Err(LibraryError::FileOperationAlreadyExists(folder_name));
        }

        let dst_path_str = dst_path.to_string_lossy().to_string();
        if self.db.folders_by_path.get(rtxn, &dst_path_str)?.is_some() {
            return Err(LibraryError::DuplicateFolder(dst_path_str));
        }

        Ok(MoveContext {
            folder_id,
            new_parent_id,
            record,
            parent_record,
            src_path,
            dst_parent_path,
            dst_path,
            folder_name,
        })
    }

    /// Collects the moved folder and every record located below its path (robust against stale parent indices).
    fn collect_subtree(
        &self,
        rtxn: &heed::RoTxn,
        mv: &MoveContext,
    ) -> Result<Vec<(FolderId, FolderRecord)>, LibraryError> {
        let mut subtree = Vec::new();
        for entry in self.db.folders.iter(rtxn)? {
            let (id, rec) = entry?;
            if Path::new(&rec.path).starts_with(&mv.src_path) {
                subtree.push((id, rec));
            }
        }

        if !subtree.iter().any(|(id, _)| *id == mv.folder_id) {
            return Err(LibraryError::FolderNotFound(mv.folder_id));
        }
        Ok(subtree)
    }

    /// Rewrites paths, depth and hidden flags of the whole subtree and its path index entries.
    fn rewrite_subtree(
        &self,
        wtxn: &mut heed::RwTxn,
        mv: &MoveContext,
        mut subtree: Vec<(FolderId, FolderRecord)>,
    ) -> Result<(Vec<FolderRecord>, usize), LibraryError> {
        // Parents first, so inherited hidden flags can be propagated top-down.
        subtree.sort_by_key(|(_, rec)| rec.depth);

        let old_hidden: HashMap<FolderId, bool> = subtree.iter().map(|(id, rec)| (*id, rec.is_hidden)).collect();
        let mut new_hidden: HashMap<FolderId, bool> = HashMap::with_capacity(subtree.len());

        let depth_delta = i64::from(mv.parent_record.depth) + 1 - i64::from(mv.record.depth);
        let dst_hidden_entries = read_dot_hidden_entries(&mv.dst_parent_path);

        // Remove all old path keys first so no intermediate state can collide.
        for (_, rec) in &subtree {
            self.db.folders_by_path.delete(wtxn, &rec.path)?;
        }

        let mut updated = Vec::with_capacity(subtree.len());
        let mut affected_media_count = 0usize;

        for (id, mut rec) in subtree {
            let is_root = id == mv.folder_id;
            let hidden = if is_root {
                rec.name.starts_with('.') || mv.parent_record.is_hidden || dst_hidden_entries.contains(&mv.folder_name)
            } else {
                Self::descendant_hidden(&rec, &old_hidden, &new_hidden)
            };
            new_hidden.insert(id, hidden);

            let new_path = Self::rebase_path(Path::new(&rec.path), &mv.src_path, &mv.dst_path);
            rec.path = new_path.to_string_lossy().to_string();
            rec.depth = (i64::from(rec.depth) + depth_delta).max(0) as u32;
            rec.is_hidden = hidden;
            if is_root {
                rec.parent_id = Some(mv.new_parent_id);
            }

            self.db.folders.put(wtxn, &id, &rec)?;
            self.db.folders_by_path.put(wtxn, &rec.path, &id)?;

            if let Some(media_ids) = self.db.media_by_folder.get(wtxn, &id)? {
                affected_media_count += media_ids.len();
            }
            updated.push(rec);
        }

        Ok((updated, affected_media_count))
    }

    /// Hidden = inherited from the (new) parent OR own reason (dot name / `.hidden` listing of the old parent).
    fn descendant_hidden(
        rec: &FolderRecord,
        old_hidden: &HashMap<FolderId, bool>,
        new_hidden: &HashMap<FolderId, bool>,
    ) -> bool {
        let Some(parent_id) = rec.parent_id else {
            return rec.is_hidden;
        };
        let (Some(&old_parent_hidden), Some(&new_parent_hidden)) =
            (old_hidden.get(&parent_id), new_hidden.get(&parent_id))
        else {
            return rec.is_hidden;
        };

        let own_reason = if old_parent_hidden {
            rec.name.starts_with('.')
        } else {
            rec.is_hidden
        };
        new_parent_hidden || own_reason
    }

    /// Replaces the `src` prefix of `path` with `dst` (returns `path` unchanged if it is not below `src`).
    fn rebase_path(path: &Path, src: &Path, dst: &Path) -> PathBuf {
        match path.strip_prefix(src) {
            Ok(suffix) if suffix.as_os_str().is_empty() => dst.to_path_buf(),
            Ok(suffix) => dst.join(suffix),
            Err(_) => path.to_path_buf(),
        }
    }

    /// Updates parent/children index entries and direct subfolder counters of both parents.
    fn relink_parents(&self, wtxn: &mut heed::RwTxn, mv: &MoveContext) -> Result<(), LibraryError> {
        if let Some(old_pid) = mv.record.parent_id {
            let mut siblings = self.db.folders_by_parent.get(wtxn, &old_pid)?.unwrap_or_default();
            siblings.retain(|&x| x != mv.folder_id);
            self.db.folders_by_parent.put(wtxn, &old_pid, &siblings)?;
            self.adjust_subfolder_count(wtxn, old_pid, -1)?;
        }

        let mut new_siblings = self
            .db
            .folders_by_parent
            .get(wtxn, &mv.new_parent_id)?
            .unwrap_or_default();
        if !new_siblings.contains(&mv.folder_id) {
            new_siblings.push(mv.folder_id);
            self.db.folders_by_parent.put(wtxn, &mv.new_parent_id, &new_siblings)?;
            self.adjust_subfolder_count(wtxn, mv.new_parent_id, 1)?;
        }

        Ok(())
    }

    fn adjust_subfolder_count(
        &self,
        wtxn: &mut heed::RwTxn,
        folder_id: FolderId,
        delta: i64,
    ) -> Result<(), LibraryError> {
        if let Some(mut rec) = self.db.folders.get(wtxn, &folder_id)? {
            rec.subfolder_count = (i64::from(rec.subfolder_count) + delta).max(0) as u32;
            self.db.folders.put(wtxn, &folder_id, &rec)?;
        }
        Ok(())
    }

    /// Returns `start` and all of its ancestors (nearest first), protected against cyclic parent links.
    fn ancestor_chain(&self, rtxn: &heed::RoTxn, start: Option<FolderId>) -> Result<Vec<FolderId>, LibraryError> {
        let mut chain = Vec::new();
        let mut visited: HashSet<FolderId> = HashSet::new();
        let mut current = start;

        while let Some(id) = current {
            if !visited.insert(id) || chain.len() >= MAX_ANCESTOR_WALK {
                tracing::warn!(
                    folder_id = id,
                    "[FolderMove: AncestorWalk] Cyclic or excessive parent chain detected, stopping walk"
                );
                break;
            }
            let Some(rec) = self.db.folders.get(rtxn, &id)? else {
                break;
            };
            chain.push(id);
            current = rec.parent_id;
        }

        Ok(chain)
    }

    /// Applies `delta` to the recursive media counter of every folder in `chain`.
    fn adjust_recursive_counts(
        &self,
        wtxn: &mut heed::RwTxn,
        chain: &[FolderId],
        delta: i64,
    ) -> Result<(), LibraryError> {
        if delta == 0 {
            return Ok(());
        }
        for id in chain {
            if let Some(mut rec) = self.db.folders.get(wtxn, id)? {
                rec.recursive_media_count = (i64::from(rec.recursive_media_count) + delta).max(0) as u32;
                self.db.folders.put(wtxn, id, &rec)?;
            }
        }
        Ok(())
    }

    /// Reads the final state of all folders in both chains (each folder once).
    fn collect_ancestor_records(
        &self,
        rtxn: &heed::RoTxn,
        old_chain: &[FolderId],
        new_chain: &[FolderId],
    ) -> Result<Vec<FolderRecord>, LibraryError> {
        let mut seen: HashSet<FolderId> = HashSet::new();
        let mut records = Vec::with_capacity(old_chain.len() + new_chain.len());

        for id in old_chain.iter().chain(new_chain.iter()) {
            if seen.insert(*id)
                && let Some(rec) = self.db.folders.get(rtxn, id)?
            {
                records.push(rec);
            }
        }

        Ok(records)
    }

    /// Physical rename with a friendly message for moves across file systems / mount points.
    fn rename_on_disk(src: &Path, dst: &Path) -> Result<(), LibraryError> {
        std::fs::rename(src, dst).map_err(|e| {
            tracing::error!(
                error = %e,
                src = %LogObscurer::path(src),
                dst = %LogObscurer::path(dst),
                "[FolderMove: RenameFailed] Physical folder rename failed, database transaction will be aborted"
            );
            if e.kind() == std::io::ErrorKind::CrossesDevices {
                LibraryError::FileOperationRenameFailed(
                    "Cannot move a folder across file systems or mount points. Move it manually instead.".to_string(),
                )
            } else {
                LibraryError::from(e)
            }
        })
    }

    /// Commits the staged transaction; if that fails the physical rename is reverted (best effort).
    fn commit_or_revert(&self, wtxn: heed::RwTxn<'_>, mv: &MoveContext) -> Result<(), LibraryError> {
        if let Err(commit_err) = wtxn.commit() {
            tracing::error!(
                error = %commit_err,
                "[FolderMove: CommitFailed] LMDB commit failed after physical rename, reverting rename"
            );
            match std::fs::rename(&mv.dst_path, &mv.src_path) {
                Ok(()) => tracing::warn!(
                    folder_id = mv.folder_id,
                    "[FolderMove: Reverted] Physical rename reverted after commit failure"
                ),
                Err(revert_err) => tracing::error!(
                    error = %revert_err,
                    folder_id = mv.folder_id,
                    src = %LogObscurer::path(&mv.src_path),
                    dst = %LogObscurer::path(&mv.dst_path),
                    "[FolderMove: RevertFailed] Could not revert physical rename. Folder now resides at the destination path; run a manual refresh."
                ),
            }
            return Err(LibraryError::from(commit_err));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: FolderId, parent: Option<FolderId>, name: &str, hidden: bool) -> FolderRecord {
        FolderRecord {
            id,
            parent_id: parent,
            name: name.to_string(),
            path: format!("/r/{}", name),
            depth: 0,
            is_hidden: hidden,
            is_deleted: false,
            subfolder_count: 0,
            media_count: 0,
            recursive_media_count: 0,
            added_at: 0,
            last_scanned_at: 0,
        }
    }

    #[test]
    fn rebase_path_replaces_prefix_only() {
        let src = Path::new("/r/a");
        let dst = Path::new("/r/b/a");
        assert_eq!(
            FolderMoveExecutor::rebase_path(Path::new("/r/a"), src, dst),
            PathBuf::from("/r/b/a")
        );
        assert_eq!(
            FolderMoveExecutor::rebase_path(Path::new("/r/a/x/y"), src, dst),
            PathBuf::from("/r/b/a/x/y")
        );
        assert_eq!(
            FolderMoveExecutor::rebase_path(Path::new("/r/ab"), src, dst),
            PathBuf::from("/r/ab")
        );
    }

    #[test]
    fn descendant_inherits_hidden_from_new_parent() {
        let rec = record(2, Some(1), "child", false);
        let old_hidden = HashMap::from([(1, false)]);
        let new_hidden = HashMap::from([(1, true)]);
        assert!(FolderMoveExecutor::descendant_hidden(&rec, &old_hidden, &new_hidden));
    }

    #[test]
    fn descendant_loses_inherited_hidden_but_keeps_dot_name() {
        let plain = record(2, Some(1), "child", true);
        let dotted = record(3, Some(1), ".cache", true);
        let old_hidden = HashMap::from([(1, true)]);
        let new_hidden = HashMap::from([(1, false)]);

        assert!(!FolderMoveExecutor::descendant_hidden(&plain, &old_hidden, &new_hidden));
        assert!(FolderMoveExecutor::descendant_hidden(&dotted, &old_hidden, &new_hidden));
    }
}
