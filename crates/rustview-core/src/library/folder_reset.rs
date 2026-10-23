//! TEMPORARY DEBUG FEATURE: one-shot reset of the folder tree (Single Point of Truth) at startup.
//!
//! Triggered with the CLI argument `--reset-folder-tree`. Remove this file together with the
//! `pub mod folder_reset;` / `pub use folder_reset::FolderTreeReset;` lines in `library/mod.rs` and the
//! `TEMPORARY DEBUG FEATURE` block in `main.rs` once the folder tree bugs are fixed.
//!
//! What the reset does (single atomic write transaction):
//! - Deletes every folder record that no media record references (including stale path-index entries).
//! - Keeps folder records referenced by media (`folder_id` -> `path`), so ALL media keep resolving to their path,
//!   but strips their tree linkage (parent, depth, counters) so they look "never scanned".
//! - Clears the whole `folders_by_parent` index.
//! - Leaves media, tags, thumbnails and id sequences untouched.
//!
//! The next manual 'Refresh Tree (Full)' rebuilds the hierarchy and re-uses the retained folder ids by path.

use super::Library;
use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folder_diagnostics::FolderDiagnostics;
use super::models::{FolderId, FolderRecord};
use crate::settings::SettingsManager;
use std::collections::HashSet;
use std::time::Instant;

/// Summary of one reset run.
#[derive(Debug, Default, Clone)]
pub struct FolderTreeResetReport {
    pub folders_total: usize,
    pub folders_retained: usize,
    pub folders_removed: usize,
    pub media_referenced_folders: usize,
    pub stale_path_entries_removed: usize,
    pub path_entries_repaired: usize,
    pub unresolvable_media_before: usize,
    pub unresolvable_media_after: usize,
    pub total_media: usize,
    pub elapsed_ms: u128,
}

pub struct FolderTreeReset;

impl FolderTreeReset {
    /// Performs the reset atomically and verifies that media path resolution did not regress.
    pub fn run(db: &LibraryDatabase) -> Result<FolderTreeResetReport, LibraryError> {
        let t0 = Instant::now();
        let mut report = FolderTreeResetReport::default();

        let before = FolderDiagnostics::count_unresolvable_media(db)?;
        report.unresolvable_media_before = before.unresolvable_media;
        report.total_media = before.total_media;

        let mut wtxn = db.env.write_txn()?;

        let referenced = Self::collect_referenced_folder_ids(db, &wtxn)?;
        report.media_referenced_folders = referenced.len();

        let mut retained: Vec<FolderRecord> = Vec::new();
        let mut removed_ids: Vec<FolderId> = Vec::new();
        for entry in db.folders.iter(&wtxn)? {
            let (id, rec) = entry?;
            report.folders_total += 1;
            if referenced.contains(&id) {
                retained.push(rec);
            } else {
                removed_ids.push(id);
            }
        }
        report.folders_retained = retained.len();
        report.folders_removed = removed_ids.len();

        for id in &removed_ids {
            db.folders.delete(&mut wtxn, id)?;
            db.media_by_folder.delete(&mut wtxn, id)?;
        }

        let retained_ids: HashSet<FolderId> = retained.iter().map(|r| r.id).collect();

        let mut stale_keys: Vec<String> = Vec::new();
        for entry in db.folders_by_path.iter(&wtxn)? {
            let (key, id) = entry?;
            if !retained_ids.contains(&id) {
                stale_keys.push(key.to_string());
            }
        }
        for key in &stale_keys {
            db.folders_by_path.delete(&mut wtxn, key.as_str())?;
        }
        report.stale_path_entries_removed = stale_keys.len();

        for mut rec in retained {
            rec.parent_id = None;
            rec.depth = 0;
            rec.is_deleted = false;
            rec.subfolder_count = 0;
            rec.media_count = 0;
            rec.recursive_media_count = 0;
            rec.last_scanned_at = 0;

            db.folders.put(&mut wtxn, &rec.id, &rec)?;

            if db.folders_by_path.get(&wtxn, rec.path.as_str())? != Some(rec.id) {
                db.folders_by_path.put(&mut wtxn, rec.path.as_str(), &rec.id)?;
                report.path_entries_repaired += 1;
            }
        }

        db.folders_by_parent.clear(&mut wtxn)?;

        wtxn.commit()?;

        let after = FolderDiagnostics::count_unresolvable_media(db)?;
        report.unresolvable_media_after = after.unresolvable_media;
        report.elapsed_ms = t0.elapsed().as_millis();

        Ok(report)
    }

    /// Collects all folder ids that media records point to (via the `media_by_folder` index and via `media.folder_id`).
    fn collect_referenced_folder_ids(
        db: &LibraryDatabase,
        rtxn: &heed::RoTxn,
    ) -> Result<HashSet<FolderId>, LibraryError> {
        let mut referenced: HashSet<FolderId> = HashSet::new();

        for entry in db.media_by_folder.iter(rtxn)? {
            let (folder_id, media_ids) = entry?;
            if !media_ids.is_empty() {
                referenced.insert(folder_id);
            }
        }

        // Undecodable media records are skipped here on purpose; `flatten()` only yields the `Ok` items.
        for (_, rec) in db.media.iter(rtxn)?.flatten() {
            referenced.insert(rec.folder_id);
        }

        Ok(referenced)
    }
}

/// Executes the reset if `--reset-folder-tree` was passed. Never aborts startup on failure.
/// Must run right after the library was opened and before the UI is created.
pub fn run_if_requested(requested: bool, library: &Library, settings: &SettingsManager) {
    if !requested {
        return;
    }

    tracing::warn!(
        "[FolderReset: Start] '--reset-folder-tree' requested: resetting the folder tree (SPoT). Media records are preserved."
    );

    match FolderTreeReset::run(library.raw_db()) {
        Ok(report) => {
            tracing::warn!(
                folders_total = report.folders_total,
                folders_retained = report.folders_retained,
                folders_removed = report.folders_removed,
                media_referenced_folders = report.media_referenced_folders,
                stale_path_entries_removed = report.stale_path_entries_removed,
                path_entries_repaired = report.path_entries_repaired,
                total_media = report.total_media,
                unresolvable_media_before = report.unresolvable_media_before,
                unresolvable_media_after = report.unresolvable_media_after,
                elapsed_ms = report.elapsed_ms as u64,
                "[FolderReset: Done] Folder tree reset completed. Trigger 'Refresh Tree (Full)' manually to rebuild the tree."
            );

            if report.unresolvable_media_after > report.unresolvable_media_before {
                tracing::error!(
                    before = report.unresolvable_media_before,
                    after = report.unresolvable_media_after,
                    "[FolderReset: MediaRegression] Reset increased the number of media without a resolvable folder path!"
                );
            } else {
                tracing::info!(
                    total_media = report.total_media,
                    "[FolderReset: MediaIntact] All media that resolved before the reset still resolve to their path"
                );
            }

            let mut app_settings = settings.get();
            if !app_settings.folders.expanded_folder_ids.is_empty() {
                app_settings.folders.expanded_folder_ids.clear();
                settings.save_quiet(&app_settings);
                tracing::info!("[FolderReset: Settings] Cleared persisted expanded folder ids");
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                "[FolderReset: Failed] Folder tree reset failed. The database was left unchanged."
            );
        }
    }
}
