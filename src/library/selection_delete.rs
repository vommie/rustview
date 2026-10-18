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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct Fixture {
        _dir: TempDir,
        root: PathBuf,
        library: Library,
    }

    fn write_jpeg(path: &Path, shade: u8) {
        let image = image::RgbImage::from_pixel(8, 8, image::Rgb([shade, 90, 200]));
        image.save_with_format(path, image::ImageFormat::Jpeg).unwrap();
    }

    /// root/{a(2 files, sub(1 file)), b(1 file), c(empty), d(1 file)}, root/loose1.jpg, root/loose2.jpg, root/loose3.jpg; all scanned.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("media");
        for sub in ["a/sub", "b", "c", "d"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        let files = [
            "loose1.jpg", "loose2.jpg", "loose3.jpg", "a/a1.jpg", "a/a2.jpg", "a/sub/s1.jpg", "b/b1.jpg", "d/d1.jpg",
        ];
        for (i, f) in files.iter().enumerate() {
            write_jpeg(&root.join(f), (i * 20) as u8);
        }
        let library = Library::open(&dir.path().join("lib")).unwrap();
        let root_rec = library.folders().add_folder(&root).unwrap();
        library
            .folders()
            .scan_subtree(root_rec.id, super::super::FolderScanOptions::default(), None::<fn(usize)>)
            .unwrap();
        // Media of every folder
        let all = library.folders().get_descendant_folder_ids(root_rec.id).unwrap();
        for id in all {
            library.refresher().refresh_folder(id).unwrap();
        }
        Fixture { _dir: dir, root, library }
    }

    impl Fixture {
        fn folder(&self, rel: &str) -> FolderId {
            let path = if rel.is_empty() { self.root.clone() } else { self.root.join(rel) };
            self.library.folders().get_folder_by_path(&path).unwrap().unwrap_or_else(|| panic!("no folder {rel}")).id
        }

        fn media(&self, rel: &str) -> MediaId {
            let path = self.root.join(rel);
            let folder = self.folder(path.parent().unwrap().strip_prefix(&self.root).unwrap().to_str().unwrap());
            self.library
                .media()
                .get_all_by_folder(folder)
                .unwrap()
                .into_iter()
                .find(|m| m.filename == path.file_name().unwrap().to_string_lossy())
                .unwrap_or_else(|| panic!("no media {rel}"))
                .id
        }

        fn plan(&self, files: &[&str], folders: &[&str]) -> SelectionDeletePlan {
            let files: Vec<MediaId> = files.iter().map(|f| self.media(f)).collect();
            let folders: Vec<FolderId> = folders.iter().map(|f| self.folder(f)).collect();
            let protected = HashSet::from([self.folder("")]);
            SelectionDeletePlanner::build(&self.library, &files, &folders, &protected).unwrap()
        }
    }

    #[test]
    fn fixture_is_indexed() {
        let fx = fixture();
        assert_eq!(fx.library.media().get_all_by_folder(fx.folder("a")).unwrap().len(), 2);
        assert_eq!(fx.library.media().get_all_by_folder(fx.folder("")).unwrap().len(), 3);
    }

    #[test]
    fn plan_counts_present_items() {
        let fx = fixture();
        let plan = fx.plan(&["loose1.jpg", "loose2.jpg"], &["a", "b"]);
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.folders.len(), 2);
        assert!(plan.files.iter().all(|f| !f.missing));
        let a = plan.folders.iter().find(|f| f.name == "a").unwrap();
        assert_eq!((a.state.clone(), a.subfolders, a.media_files), (FolderState::Present, 1, 3));
        assert!(!plan.is_database_only());
    }

    #[test]
    fn plan_never_contains_the_protected_root_and_ignores_duplicates_and_unknown_ids() {
        let fx = fixture();
        let protected = HashSet::from([fx.folder("")]);
        let plan = SelectionDeletePlanner::build(
            &fx.library,
            &[fx.media("loose1.jpg"), fx.media("loose1.jpg"), 999_999],
            &[fx.folder(""), fx.folder("b"), fx.folder("b"), 888_888],
            &protected,
        )
        .unwrap();
        assert_eq!((plan.files.len(), plan.folders.len()), (1, 1));
        assert_eq!((plan.protected_folders, plan.unknown_items), (1, 2));
    }

    #[test]
    fn nested_folders_and_files_inside_selected_folders_are_covered_once() {
        let fx = fixture();
        let plan = fx.plan(&["a/a1.jpg", "a/sub/s1.jpg", "loose3.jpg"], &["a", "a/sub"]);
        assert_eq!(plan.folders.len(), 1);
        assert_eq!(plan.folders[0].name, "a");
        assert_eq!(plan.nested_folders, 1);
        assert_eq!(plan.files_in_folders, 2);
        assert_eq!(plan.files.len(), 1);
    }

    #[test]
    fn plan_reports_parts_of_a_present_folder_that_are_missing_already() {
        let fx = fixture();
        let clean = fx.plan(&[], &["a"]);
        assert_eq!((clean.folders[0].missing_subfolders, clean.folders[0].missing_media), (0, 0));

        fx.library.folders().set_deleted(fx.folder("a/sub"), true).unwrap();
        fx.library.media().set_deleted(fx.media("a/sub/s1.jpg"), true).unwrap();
        let plan = fx.plan(&[], &["a"]);
        assert_eq!(plan.folders[0].state, FolderState::Present);
        assert_eq!((plan.folders[0].missing_subfolders, plan.folders[0].missing_media), (1, 1));
        assert_eq!(plan.folders[0].subfolders, 1, "the missing subfolder is still a subfolder of the tree");
    }

    /// What a scan does to a folder that vanished: flagged missing, and no longer in its parent's child list.
    fn vanish_like_a_scan(fx: &Fixture, rel: &str) {
        let id = fx.folder(rel);
        let record = fx.library.folders().get_folder(id).unwrap().unwrap();
        fs::remove_dir_all(fx.root.join(rel)).unwrap();
        fx.library.folders().set_deleted(id, true).unwrap();
        let parent = record.parent_id.unwrap();
        let rtxn = fx.library.raw_db().env.read_txn().unwrap();
        let mut children = fx.library.raw_db().folders_by_parent.get(&rtxn, &parent).unwrap().unwrap();
        drop(rtxn);
        children.retain(|&c| c != id);
        let mut wtxn = fx.library.raw_db().env.write_txn().unwrap();
        fx.library.raw_db().folders_by_parent.put(&mut wtxn, &parent, &children).unwrap();
        wtxn.commit().unwrap();
    }

    #[test]
    fn metrics_find_missing_subfolders_that_a_scan_unlinked() {
        let fx = fixture();
        let a = fx.folder("a");
        let sub = fx.folder("a/sub");
        let before = fx.library.folder_operations().get_metrics(a).unwrap();
        assert_eq!((before.subfolders_count, before.media_files_count, before.missing_subfolders, before.missing_media), (1, 3, 0, 0));

        vanish_like_a_scan(&fx, "a/sub");
        let after = fx.library.folder_operations().get_metrics(a).unwrap();
        assert_eq!(after.subfolders_count, 1, "the missing subfolder is still part of the subtree");
        assert!(after.affected_folder_ids.contains(&sub));
        assert_eq!((after.missing_subfolders, after.missing_media, after.media_files_count), (1, 1, 2));
    }

    #[test]
    fn deleting_a_folder_also_takes_its_unlinked_missing_subfolders_along() {
        let fx = fixture();
        let sub = fx.folder("a/sub");
        let s1 = fx.media("a/sub/s1.jpg");
        vanish_like_a_scan(&fx, "a/sub");
        let plan = fx.plan(&[], &["a"]);
        assert_eq!((plan.folders[0].missing_subfolders, plan.folders[0].missing_media), (1, 1));
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);
        assert_eq!(report.failures(), 0);
        assert!(report.folders_deleted.contains(&sub));
        assert!(fx.library.media().get(s1).unwrap().unwrap().is_deleted);
    }

    #[test]
    fn purging_a_missing_folder_leaves_no_records_of_its_missing_subfolders() {
        let fx = fixture();
        let (a, sub, s1) = (fx.folder("a"), fx.folder("a/sub"), fx.media("a/sub/s1.jpg"));
        vanish_like_a_scan(&fx, "a/sub");
        fs::remove_dir_all(fx.root.join("a")).unwrap();
        let plan = fx.plan(&[], &["a"]);
        assert_eq!(plan.folders[0].state, FolderState::Missing);
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);
        assert_eq!(report.failures(), 0);
        assert!(fx.library.folders().get_folder(a).unwrap().is_none());
        assert!(fx.library.folders().get_folder(sub).unwrap().is_none(), "no dangling subfolder record");
        assert!(fx.library.media().get(s1).unwrap().is_none());
    }

    #[test]
    fn plan_classifies_missing_and_reborn() {
        let fx = fixture();
        fs::remove_file(fx.root.join("loose2.jpg")).unwrap();
        fs::remove_dir_all(fx.root.join("b")).unwrap();
        fx.library.folders().set_deleted(fx.folder("d"), true).unwrap(); // flagged, but still on disk
        let plan = fx.plan(&["loose1.jpg", "loose2.jpg"], &["b", "c", "d"]);
        let file = |name: &str| plan.files.iter().find(|f| f.name == name).unwrap();
        assert!(!file("loose1.jpg").missing && file("loose2.jpg").missing);
        let state = |name: &str| plan.folders.iter().find(|f| f.name == name).unwrap().state.clone();
        assert_eq!((state("b"), state("c"), state("d")), (FolderState::Missing, FolderState::Present, FolderState::Reborn));
    }

    #[test]
    fn database_only_when_everything_is_missing() {
        let fx = fixture();
        fs::remove_file(fx.root.join("loose2.jpg")).unwrap();
        fs::remove_dir_all(fx.root.join("b")).unwrap();
        assert!(fx.plan(&["loose2.jpg"], &["b"]).is_database_only());
        assert!(!fx.plan(&["loose2.jpg", "loose1.jpg"], &["b"]).is_database_only());
        assert!(!fx.plan(&[], &[]).is_database_only());
    }

    #[test]
    fn permanent_delete_of_a_mixed_selection() {
        let fx = fixture();
        let loose1 = fx.media("loose1.jpg");
        let b1 = fx.media("b/b1.jpg");
        let sub = fx.folder("a/sub");
        let a = fx.folder("a");
        let plan = fx.plan(&["loose1.jpg", "loose2.jpg"], &["a", "b"]);
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);

        assert_eq!(report.failures(), 0, "{report:?}");
        assert_eq!((report.files_deleted.len(), report.top_level_folders_deleted), (2, 2));
        // Disk
        assert!(!fx.root.join("loose1.jpg").exists() && !fx.root.join("loose2.jpg").exists());
        assert!(!fx.root.join("a").exists() && !fx.root.join("b").exists());
        assert!(fx.root.join("loose3.jpg").exists() && fx.root.join("c").exists() && fx.root.join("d/d1.jpg").exists());
        // Database: files purged, folders flagged with their whole subtrees and media
        assert!(fx.library.media().get(loose1).unwrap().is_none());
        assert!(fx.library.folders().get_folder(a).unwrap().unwrap().is_deleted);
        assert!(fx.library.folders().get_folder(sub).unwrap().unwrap().is_deleted);
        assert!(fx.library.media().get(b1).unwrap().unwrap().is_deleted);
        assert!(report.folders_deleted.contains(&sub));
        assert_eq!(report.folder_media_flagged.len(), 4);
        // Untouched
        assert!(!fx.library.folders().get_folder(fx.folder("c")).unwrap().unwrap().is_deleted);
        assert_eq!(fx.library.media().get_active_count(fx.folder("")).unwrap(), 1);
    }

    #[test]
    fn missing_items_only_touch_the_database() {
        let fx = fixture();
        fs::remove_file(fx.root.join("loose2.jpg")).unwrap();
        fs::remove_dir_all(fx.root.join("b")).unwrap();
        let (b, b1, loose2) = (fx.folder("b"), fx.media("b/b1.jpg"), fx.media("loose2.jpg"));
        let plan = fx.plan(&["loose2.jpg"], &["b"]);
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Trash);

        assert_eq!(report.failures(), 0);
        assert_eq!((report.files_purged.len(), report.top_level_folders_purged), (1, 1));
        assert!(report.files_deleted.is_empty() && report.folders_deleted.is_empty());
        assert!(fx.library.media().get(loose2).unwrap().is_none());
        assert!(fx.library.folders().get_folder(b).unwrap().is_none());
        assert!(fx.library.media().get(b1).unwrap().is_none());
        assert!(fx.root.join("loose1.jpg").exists());
    }

    #[test]
    fn a_folder_that_came_back_is_restored_and_not_purged() {
        let fx = fixture();
        let d = fx.folder("d");
        fx.library.folders().set_deleted(d, true).unwrap();
        let plan = fx.plan(&[], &["d", "c"]);
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);

        assert_eq!(report.folders_reborn.len(), 1);
        assert_eq!(report.folders_reborn[0].name, "d");
        assert!(fx.root.join("d/d1.jpg").exists());
        let record = fx.library.folders().get_folder(d).unwrap().unwrap();
        assert!(!record.is_deleted, "restored");
        assert!(!fx.root.join("c").exists(), "the other folder is still deleted");
        assert_eq!(report.top_level_folders_deleted, 1);
    }

    #[test]
    fn a_folder_missing_in_the_plan_that_exists_at_execution_is_restored_not_purged() {
        let fx = fixture();
        fs::rename(fx.root.join("b"), fx.root.join("b_away")).unwrap();
        let plan = fx.plan(&[], &["b"]);
        assert_eq!(plan.folders[0].state, FolderState::Missing);
        fs::rename(fx.root.join("b_away"), fx.root.join("b")).unwrap();
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);
        assert_eq!(report.folders_reborn.len(), 1);
        assert!(fx.root.join("b/b1.jpg").exists());
        assert!(fx.library.folders().get_folder(fx.folder("b")).unwrap().is_some());
    }

    #[test]
    fn a_folder_that_vanishes_after_the_plan_is_purged_from_the_database() {
        let fx = fixture();
        let plan = fx.plan(&[], &["b"]);
        fs::remove_dir_all(fx.root.join("b")).unwrap();
        let b = plan.folders[0].id;
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);
        assert_eq!((report.top_level_folders_purged, report.failures()), (1, 0));
        assert!(fx.library.folders().get_folder(b).unwrap().is_none());
    }

    #[test]
    fn a_file_that_vanishes_after_the_plan_still_counts_as_removed() {
        let fx = fixture();
        let plan = fx.plan(&["loose1.jpg"], &[]);
        fs::remove_file(fx.root.join("loose1.jpg")).unwrap();
        let id = plan.files[0].id;
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);
        assert_eq!((report.files_deleted.len(), report.failures()), (1, 0));
        assert!(fx.library.media().get(id).unwrap().is_none());
    }

    #[test]
    fn a_file_that_cannot_be_removed_keeps_its_record_and_the_rest_goes_on() {
        let fx = fixture();
        let stuck = fx.media("loose1.jpg");
        let plan = fx.plan(&["loose1.jpg", "loose2.jpg"], &["c"]);
        // A directory in place of the file: remove_file fails although the path exists.
        fs::remove_file(fx.root.join("loose1.jpg")).unwrap();
        fs::create_dir(fx.root.join("loose1.jpg")).unwrap();
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);

        assert_eq!(report.file_failures.len(), 1);
        assert_eq!(report.file_failures[0].name, "loose1.jpg");
        assert!(fx.library.media().get(stuck).unwrap().is_some(), "the record of a file that is still there stays");
        assert_eq!(report.files_deleted.len(), 1);
        assert!(!fx.root.join("loose2.jpg").exists());
        assert_eq!(report.top_level_folders_deleted, 1);
        assert!(report.changed_anything());
    }

    #[test]
    fn a_folder_that_cannot_be_removed_is_reported_and_the_others_run() {
        let fx = fixture();
        let plan = fx.plan(&[], &["b", "c"]);
        // Make "b" undeletable: a read-only parent would stop the whole tree, so remove its permission instead.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(fx.root.join("b"), fs::Permissions::from_mode(0o555)).unwrap();
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);
        fs::set_permissions(fx.root.join("b"), fs::Permissions::from_mode(0o755)).ok();

        // remove_dir_all may delete the file inside only if allowed; either way "b" must not be flagged when it failed.
        let b = fx.folder("b");
        let b_record = fx.library.folders().get_folder(b).unwrap().unwrap();
        if report.folder_failures.iter().any(|f| f.name == "b") {
            assert!(!b_record.is_deleted, "a folder that could not be removed is not flagged");
            assert!(fx.root.join("b").exists());
        } else {
            assert!(running_as_root(), "the removal can only succeed where permissions are not enforced");
        }
        assert!(!fx.root.join("c").exists(), "the other folder was still deleted");
    }

    fn running_as_root() -> bool {
        std::fs::metadata("/proc/self").map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0).unwrap_or(false)
    }

    #[test]
    fn empty_selection_does_nothing() {
        let fx = fixture();
        let plan = fx.plan(&[], &[]);
        let report = SelectionDeleteExecutor::execute(&fx.library, &plan, DeleteMode::Permanent);
        assert!(!report.changed_anything());
        assert_eq!(report.failures(), 0);
    }

    #[test]
    fn take_reborn_splits_the_plan() {
        let fx = fixture();
        fx.library.folders().set_deleted(fx.folder("d"), true).unwrap();
        let mut plan = fx.plan(&[], &["d", "c"]);
        let reborn = plan.take_reborn();
        assert_eq!((reborn.len(), plan.folders.len()), (1, 1));
        assert_eq!(plan.folders[0].name, "c");
    }
}
