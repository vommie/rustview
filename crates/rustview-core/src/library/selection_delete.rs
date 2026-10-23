//! Deleting a selection of folders and files in one go (the combined "Delete Files & Folders" dialog).
//!
//! `SelectionDeletePlanner` turns the selected ids into a plan from the library alone (no UI): what is present on disk, what is already
//! missing, what is only listed twice (a file inside a selected folder, a folder inside another selected one) and what must never
//! be deleted. `SelectionDeleteExecutor` carries the plan out with the same operations and checks the single dialogs use:
//!
//! * present files: removed on disk (Trash or permanently), their records purged only when the removal worked;
//! * present folders: `FolderOperations::delete_folder` (disk first, records flagged `is_deleted` afterwards);
//! * missing files and folders: only the database is cleaned (`purge_media_from_db`, `purge_folder_from_db`);
//! * a folder that is flagged missing but is back on disk ("reborn") is restored and left alone, never purged.
//!
//! Every item is independent: one that fails is reported and the others still run; a failed item never loses its database record.

use super::error::LibraryError;
use super::models::{FolderId, MediaId};
use super::Library;
use crate::settings::LogObscurer;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// How present items leave the drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    Trash,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    pub id: MediaId,
    pub name: String,
    pub path: PathBuf,
    /// Gone from the drive already (or flagged so): only its record is removed.
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderState {
    /// On disk and not flagged: deleted on disk, flagged in the library.
    Present,
    /// Not on disk: only its records are removed.
    Missing,
    /// Flagged missing but on disk again: restored, not deleted.
    Reborn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPlan {
    pub id: FolderId,
    pub name: String,
    pub path: PathBuf,
    pub state: FolderState,
    pub subfolders: usize,
    pub media_files: usize,
    /// Of the subfolders / media above: how many are flagged missing on disk already.
    pub missing_subfolders: usize,
    pub missing_media: usize,
    /// The folder itself and every folder below it.
    pub affected: Vec<FolderId>,
}

/// What the selection comes to, ready for the dialog and for the executor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionDeletePlan {
    pub files: Vec<FilePlan>,
    pub folders: Vec<FolderPlan>,
    /// Selected folders that lie inside another selected folder (deleted with it).
    pub nested_folders: usize,
    /// Selected files that lie inside a selected folder (deleted with it).
    pub files_in_folders: usize,
    /// Selected folders that are protected (the library root).
    pub protected_folders: usize,
    /// Selected ids the library does not know any more.
    pub unknown_items: usize,
}

impl SelectionDeletePlan {
    pub fn present_files(&self) -> impl Iterator<Item = &FilePlan> {
        self.files.iter().filter(|f| !f.missing)
    }

    pub fn missing_files(&self) -> impl Iterator<Item = &FilePlan> {
        self.files.iter().filter(|f| f.missing)
    }

    pub fn folders_in(&self, state: FolderState) -> impl Iterator<Item = &FolderPlan> {
        self.folders.iter().filter(move |f| f.state == state)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.folders.is_empty()
    }

    /// Nothing is left on the drive to delete: the dialog only offers the database cleanup.
    pub fn is_database_only(&self) -> bool {
        !self.is_empty() && self.present_files().next().is_none() && self.folders_in(FolderState::Present).next().is_none()
    }

    /// Drops the folders that came back on disk (after they were restored) and returns them.
    pub fn take_reborn(&mut self) -> Vec<FolderPlan> {
        let (reborn, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.folders).into_iter().partition(|f| f.state == FolderState::Reborn);
        self.folders = rest;
        reborn
    }
}

pub struct SelectionDeletePlanner;

impl SelectionDeletePlanner {
    /// Builds the plan. `protected` folders (the library root) are never part of it.
    pub fn build(
        library: &Library,
        files: &[MediaId],
        folders: &[FolderId],
        protected: &HashSet<FolderId>,
    ) -> Result<SelectionDeletePlan, LibraryError> {
        let mut plan = SelectionDeletePlan::default();
        let missing_children = library.folder_operations().missing_children()?;

        // Folders: unique, known, not protected.
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for &id in folders {
            if !seen.insert(id) {
                continue;
            }
            if protected.contains(&id) {
                plan.protected_folders += 1;
                continue;
            }
            let Some(record) = library.folders().get_folder(id)? else {
                plan.unknown_items += 1;
                continue;
            };
            let metrics = library.folder_operations().get_metrics_with(id, &missing_children)?;
            let path = PathBuf::from(&record.path);
            let state = match (path.exists(), record.is_deleted) {
                (true, false) => FolderState::Present,
                (true, true) => FolderState::Reborn,
                (false, _) => FolderState::Missing,
            };
            let name = Path::new(&record.path)
                .file_name()
                .map_or_else(|| record.name.clone(), |n| n.to_string_lossy().to_string());
            candidates.push(FolderPlan {
                id,
                name,
                path,
                state,
                subfolders: metrics.subfolders_count,
                // The records of a folder that was never opened are not in the database yet; the counters of the tree know the files.
                media_files: metrics.media_files_count.max(record.recursive_media_count as usize),
                missing_subfolders: metrics.missing_subfolders,
                missing_media: metrics.missing_media,
                affected: metrics.affected_folder_ids,
            });
        }

        // A folder that lies inside another selected folder goes with it.
        let all_affected: Vec<(FolderId, HashSet<FolderId>)> =
            candidates.iter().map(|c| (c.id, c.affected.iter().copied().collect())).collect();
        for candidate in candidates {
            let inside_other = all_affected.iter().any(|(other, set)| *other != candidate.id && set.contains(&candidate.id));
            if inside_other {
                plan.nested_folders += 1;
            } else {
                plan.folders.push(candidate);
            }
        }
        let covered: HashSet<FolderId> = plan.folders.iter().flat_map(|f| f.affected.iter().copied()).collect();

        // Files: unique, known, and not inside a folder that is deleted anyway.
        let mut seen = HashSet::new();
        for &id in files {
            if !seen.insert(id) {
                continue;
            }
            let Some(record) = library.media().get(id)? else {
                plan.unknown_items += 1;
                continue;
            };
            if covered.contains(&record.folder_id) {
                plan.files_in_folders += 1;
                continue;
            }
            let path = library.resolve_media_path(id)?;
            let missing = record.is_deleted || !path.exists();
            plan.files.push(FilePlan { id, name: record.filename, path, missing });
        }

        Ok(plan)
    }
}

/// Why one item did not work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFailure {
    pub name: String,
    pub reason: String,
}

/// What the executor did, item by item.
#[derive(Debug, Clone, Default)]
pub struct SelectionDeleteReport {
    /// Files deleted on disk and purged from the database.
    pub files_deleted: Vec<MediaId>,
    /// Missing files removed from the database only.
    pub files_purged: Vec<MediaId>,
    pub file_failures: Vec<ItemFailure>,
    /// Folders deleted on disk (flagged in the database); the ids are the whole subtrees.
    pub folders_deleted: Vec<FolderId>,
    /// Missing folders removed from the database; the ids are the whole subtrees.
    pub folders_purged: Vec<FolderId>,
    /// Folders that were found on disk again and restored instead of being removed.
    pub folders_reborn: Vec<FolderPlan>,
    pub folder_failures: Vec<ItemFailure>,
    /// Media records of deleted/purged folders (viewers and the list must forget them).
    pub folder_media_flagged: Vec<MediaId>,
    pub folder_media_purged: Vec<MediaId>,
    pub top_level_folders_deleted: usize,
    pub top_level_folders_purged: usize,
}

impl SelectionDeleteReport {
    pub fn files_done(&self) -> usize {
        self.files_deleted.len() + self.files_purged.len()
    }

    pub fn folders_done(&self) -> usize {
        self.top_level_folders_deleted + self.top_level_folders_purged
    }

    pub fn failures(&self) -> usize {
        self.file_failures.len() + self.folder_failures.len()
    }

    /// Something changed in the library or on the drive.
    pub fn changed_anything(&self) -> bool {
        self.files_done() > 0 || self.folders_done() > 0 || !self.folders_reborn.is_empty()
    }

    /// Every folder id whose subtree left the library or was flagged.
    pub fn all_folder_ids(&self) -> Vec<FolderId> {
        self.folders_deleted.iter().chain(&self.folders_purged).copied().collect()
    }
}

pub struct SelectionDeleteExecutor;

impl SelectionDeleteExecutor {
    pub fn execute(library: &Library, plan: &SelectionDeletePlan, mode: DeleteMode) -> SelectionDeleteReport {
        let mut report = SelectionDeleteReport::default();
        Self::delete_files(library, plan, mode, &mut report);
        Self::delete_folders(library, plan, mode, &mut report);
        tracing::info!(
            files_deleted = report.files_deleted.len(),
            files_purged = report.files_purged.len(),
            folders_deleted = report.top_level_folders_deleted,
            folders_purged = report.top_level_folders_purged,
            reborn = report.folders_reborn.len(),
            failures = report.failures(),
            ?mode,
            "[SelectionDelete: Done] Finished deleting the selection"
        );
        report
    }

    /// Removes one file from the drive; a file that is gone already counts as removed.
    fn remove_from_drive(path: &Path, mode: DeleteMode) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        match mode {
            DeleteMode::Trash => trash::delete(path).map_err(|e| e.to_string()),
            DeleteMode::Permanent => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            },
        }
    }

    fn delete_files(library: &Library, plan: &SelectionDeletePlan, mode: DeleteMode, report: &mut SelectionDeleteReport) {
        let mut to_purge: Vec<(MediaId, bool)> = Vec::new();
        for file in &plan.files {
            if file.missing {
                to_purge.push((file.id, false));
                continue;
            }
            match Self::remove_from_drive(&file.path, mode) {
                Ok(()) => to_purge.push((file.id, true)),
                Err(reason) => {
                    tracing::warn!(path = %LogObscurer::path(&file.path), %reason, "[SelectionDelete: File] The file could not be removed; its record stays");
                    report.file_failures.push(ItemFailure { name: file.name.clone(), reason });
                }
            }
        }
        if to_purge.is_empty() {
            return;
        }
        let ids: Vec<MediaId> = to_purge.iter().map(|(id, _)| *id).collect();
        match library.file_operations().purge_media_from_db(&ids) {
            Ok(_) => {
                for (id, was_on_disk) in to_purge {
                    if was_on_disk {
                        report.files_deleted.push(id);
                    } else {
                        report.files_purged.push(id);
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "[SelectionDelete: File] The records could not be purged");
                for file in plan.files.iter().filter(|f| ids.contains(&f.id)) {
                    report.file_failures.push(ItemFailure { name: file.name.clone(), reason: format!("database: {e}") });
                }
            }
        }
    }

    fn delete_folders(library: &Library, plan: &SelectionDeletePlan, mode: DeleteMode, report: &mut SelectionDeleteReport) {
        let to_trash = mode == DeleteMode::Trash;
        for folder in &plan.folders {
            // The same check the single dialogs make right before they act: a folder that is back on disk is never purged.
            let exists_now = folder.path.exists();
            if folder.state != FolderState::Present && exists_now {
                match library.folder_operations().check_and_restore_if_exists(folder.id) {
                    Ok(_) => report.folders_reborn.push(folder.clone()),
                    Err(e) => report.folder_failures.push(ItemFailure { name: folder.name.clone(), reason: e.to_string() }),
                }
                continue;
            }

            let result = if exists_now {
                library.folder_operations().delete_folder(folder.id, to_trash).map(|r| (r, false))
            } else {
                // Gone from the drive (also when it vanished after the plan was made): the database is cleaned.
                library.folder_operations().purge_folder_from_db(folder.id).map(|r| (r, true))
            };

            match result {
                Ok((done, purged)) => {
                    if purged {
                        report.top_level_folders_purged += 1;
                        report.folders_purged.extend(done.deleted_folder_ids);
                        report.folder_media_purged.extend(done.deleted_media_ids);
                    } else {
                        report.top_level_folders_deleted += 1;
                        report.folders_deleted.extend(done.deleted_folder_ids);
                        report.folder_media_flagged.extend(done.deleted_media_ids);
                    }
                }
                Err(e) => {
                    tracing::error!(folder = %LogObscurer::path(&folder.path), error = %e, "[SelectionDelete: Folder] The folder could not be deleted");
                    report.folder_failures.push(ItemFailure { name: folder.name.clone(), reason: e.to_string() });
                }
            }
        }
    }
}
