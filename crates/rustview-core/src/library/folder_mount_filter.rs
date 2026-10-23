//! Mount point filtering for manual folder tree scans.
//!
//! - Resolves which mount points below the scan root must be ignored (reads the system mount table on every scan).
//! - Cleans up already persisted folder records that lie at or below an ignored mount point, atomically within
//!   the scan's write transaction. Records that media still refer to are kept (so every media path keeps resolving)
//!   but are detached from the tree; empty records are removed.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, FolderRecord};
use crate::settings::LogObscurer;
use crate::system::SystemMounts;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Records of persisted folders keyed by their stored path string.
pub type ExistingFolders = HashMap<String, (FolderId, FolderRecord)>;

/// Summary of the cleanup of persisted records below ignored mounts.
#[derive(Debug, Default, Clone)]
pub struct MountCleanupReport {
    pub detached_with_media: usize,
    pub removed_empty: usize,
}

pub struct FolderMountFilter;

impl FolderMountFilter {
    /// Reads the system mount table and returns the mount points below `root` that must be skipped.
    /// Returns an empty set if the option is disabled or the mount table cannot be read (scan continues unfiltered).
    pub fn resolve_ignored_mounts(root: &Path, enabled: bool) -> HashSet<PathBuf> {
        if !enabled {
            tracing::info!(
                root = %LogObscurer::path(root),
                "[MountFilter: Disabled] 'Ignore mounts inside root' is disabled. Scanning through all mount points."
            );
            return HashSet::new();
        }

        match SystemMounts::nested_mount_points(root) {
            Ok(mounts) => mounts,
            Err(e) => {
                tracing::warn!(
                    root = %LogObscurer::path(root),
                    error = %e,
                    "[MountFilter: ReadFailed] Could not read the system mount table. Scanning without mount filter."
                );
                HashSet::new()
            }
        }
    }

    /// Returns `true` if `path` equals an ignored mount point or lies below one.
    pub fn is_under_ignored_mount(path: &Path, ignored: &HashSet<PathBuf>) -> bool {
        path.ancestors().any(|ancestor| ignored.contains(ancestor))
    }

    /// Splits `existing` into records that stay subject to normal scan reconciliation (returned) and records at or
    /// below ignored mounts (handled here: detached if media refer to them, otherwise removed).
    pub fn cleanup_ignored_records(
        db: &LibraryDatabase,
        wtxn: &mut heed::RwTxn,
        existing: ExistingFolders,
        ignored: &HashSet<PathBuf>,
    ) -> Result<(MountCleanupReport, ExistingFolders), LibraryError> {
        if ignored.is_empty() {
            return Ok((MountCleanupReport::default(), existing));
        }

        let mut report = MountCleanupReport::default();
        let mut remaining: ExistingFolders = HashMap::with_capacity(existing.len());

        for (path_key, (id, mut rec)) in existing {
            if !Self::is_under_ignored_mount(Path::new(&path_key), ignored) {
                remaining.insert(path_key, (id, rec));
                continue;
            }

            let has_media = db
                .media_by_folder
                .get(wtxn, &id)?
                .map(|list| !list.is_empty())
                .unwrap_or(false);

            if has_media {
                Self::detach_record(db, wtxn, id, &mut rec)?;
                report.detached_with_media += 1;
            } else {
                Self::remove_record(db, wtxn, id, &rec)?;
                report.removed_empty += 1;
            }
        }

        tracing::info!(
            detached_with_media = report.detached_with_media,
            removed_empty = report.removed_empty,
            "[MountFilter: Cleanup] Cleaned persisted folder records below ignored mount points"
        );

        Ok((report, remaining))
    }

    /// Keeps the record (media still resolve their path via folder_id) but removes it from the tree hierarchy.
    fn detach_record(
        db: &LibraryDatabase,
        wtxn: &mut heed::RwTxn,
        id: FolderId,
        rec: &mut FolderRecord,
    ) -> Result<(), LibraryError> {
        rec.parent_id = None;
        rec.depth = 0;
        rec.subfolder_count = 0;
        rec.media_count = 0;
        rec.recursive_media_count = 0;

        db.folders.put(wtxn, &id, rec)?;
        db.folders_by_parent.delete(wtxn, &id)?;

        tracing::debug!(
            folder_id = id,
            path = %LogObscurer::path(Path::new(&rec.path)),
            "[MountFilter: Cleanup] Detached folder record with media below ignored mount from the tree"
        );
        Ok(())
    }

    /// Removes a record without media and all its index entries.
    fn remove_record(
        db: &LibraryDatabase,
        wtxn: &mut heed::RwTxn,
        id: FolderId,
        rec: &FolderRecord,
    ) -> Result<(), LibraryError> {
        db.folders.delete(wtxn, &id)?;
        if db.folders_by_path.get(wtxn, &rec.path)? == Some(id) {
            db.folders_by_path.delete(wtxn, &rec.path)?;
        }
        db.folders_by_parent.delete(wtxn, &id)?;
        db.media_by_folder.delete(wtxn, &id)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_paths_at_or_below_ignored_mounts() {
        let mut ignored = HashSet::new();
        ignored.insert(PathBuf::from("/home/u/mounts/nas"));

        assert!(FolderMountFilter::is_under_ignored_mount(
            Path::new("/home/u/mounts/nas"),
            &ignored
        ));
        assert!(FolderMountFilter::is_under_ignored_mount(
            Path::new("/home/u/mounts/nas/photos/2024"),
            &ignored
        ));
        assert!(!FolderMountFilter::is_under_ignored_mount(
            Path::new("/home/u/mounts/nas2"),
            &ignored
        ));
        assert!(!FolderMountFilter::is_under_ignored_mount(
            Path::new("/home/u/mounts"),
            &ignored
        ));
    }
}
