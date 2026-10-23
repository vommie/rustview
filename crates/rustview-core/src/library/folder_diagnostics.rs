//! Read-only diagnostics for the folder tree schema (folders, `folders_by_path`, `folders_by_parent`)
//! and for media path resolution (`media.folder_id` -> folders). Never mutates the database.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folder_snapshot::{is_in_scope, scope_prefix};
use super::models::FolderId;
use crate::settings::LogObscurer;
use crate::util::cast::SaturatingAs;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Instant;

const MAX_SAMPLES: usize = 5;

/// Result of checking that every media record still resolves to a registered folder (and thus a path).
#[derive(Debug, Default, Clone)]
pub struct MediaResolutionReport {
    pub total_media: usize,
    pub unresolvable_media: usize,
    pub undecodable_media: usize,
    /// (`media_id`, `folder_id`) samples of media whose folder record is missing.
    pub samples: Vec<(u64, FolderId)>,
}

/// Consistency report over the folder schema.
#[derive(Debug, Default, Clone)]
pub struct FolderAuditReport {
    pub total_folders: usize,
    pub undecodable_folders: usize,
    pub parentless_folders: usize,
    pub folders_with_parent: usize,
    pub deleted_folders: usize,
    pub hidden_folders: usize,
    pub max_depth: u32,
    pub duplicate_path_records: usize,
    pub dangling_parent_links: usize,
    pub parent_index_entries: usize,
    pub parent_index_children: usize,
    pub parent_index_mismatches: usize,
    pub path_index_entries: usize,
    pub stale_path_index_entries: usize,
    pub unindexed_folders: usize,
    pub root_scope_folders: Option<usize>,
    pub root_registered: Option<bool>,
    pub media: MediaResolutionReport,
    pub elapsed_ms: u128,
}

pub struct FolderDiagnostics;

impl FolderDiagnostics {
    /// Runs the full audit on a background thread and logs the result. Never blocks startup.
    pub fn spawn_background_audit(db: LibraryDatabase, trigger: &'static str) {
        let spawned =
            std::thread::Builder::new()
                .name("folder-audit".into())
                .spawn(move || match Self::run_audit(&db) {
                    Ok(report) => Self::log_report(trigger, &report),
                    Err(e) => tracing::error!(
                        trigger,
                        error = %e,
                        "[FolderAudit: Failed] Folder schema audit failed"
                    ),
                });

        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[FolderAudit: Spawn] Could not spawn folder audit thread");
        }
    }

    /// Audits the folder tables and media path resolution in a single read transaction.
    pub fn run_audit(db: &LibraryDatabase) -> Result<FolderAuditReport, LibraryError> {
        let t0 = Instant::now();
        let rtxn = db.env.read_txn()?;
        let mut report = FolderAuditReport::default();

        let root = db.tree_root();
        let root_str = root.as_ref().map(|p| p.to_string_lossy().to_string());
        let prefix = root_str.as_deref().map(scope_prefix);
        if root_str.is_some() {
            report.root_scope_folders = Some(0);
            report.root_registered = Some(false);
        }

        let mut parent_map: HashMap<FolderId, Option<FolderId>> = HashMap::new();
        let mut seen_path_hashes: HashSet<u64> = HashSet::new();

        for entry in db.folders.iter(&rtxn)? {
            let (id, rec) = if let Ok(v) = entry {
                v
            } else {
                report.undecodable_folders += 1;
                continue;
            };

            report.total_folders += 1;
            if rec.parent_id.is_none() {
                report.parentless_folders += 1;
            } else {
                report.folders_with_parent += 1;
            }
            if rec.is_deleted {
                report.deleted_folders += 1;
            }
            if rec.is_hidden {
                report.hidden_folders += 1;
            }
            report.max_depth = report.max_depth.max(rec.depth);

            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            rec.path.hash(&mut hasher);
            if !seen_path_hashes.insert(hasher.finish()) {
                report.duplicate_path_records += 1;
            }

            if let (Some(r), Some(p)) = (root_str.as_deref(), prefix.as_deref())
                && is_in_scope(&rec.path, r, p)
            {
                report.root_scope_folders = report.root_scope_folders.map(|c| c + 1);
                if rec.path == r {
                    report.root_registered = Some(true);
                }
            }

            parent_map.insert(id, rec.parent_id);
        }

        report.dangling_parent_links = parent_map
            .values()
            .filter(|pid| matches!(pid, Some(p) if !parent_map.contains_key(p)))
            .count();

        for entry in db.folders_by_parent.iter(&rtxn)? {
            let Ok((parent, children)) = entry else {
                continue;
            };
            report.parent_index_entries += 1;
            for child in children {
                report.parent_index_children += 1;
                if !matches!(parent_map.get(&child), Some(Some(p)) if *p == parent) {
                    report.parent_index_mismatches += 1;
                }
            }
        }

        let mut indexed_ids: HashSet<FolderId> = HashSet::with_capacity(parent_map.len());
        for entry in db.folders_by_path.iter(&rtxn)? {
            let Ok((_, id)) = entry else {
                continue;
            };
            report.path_index_entries += 1;
            if parent_map.contains_key(&id) {
                indexed_ids.insert(id);
            } else {
                report.stale_path_index_entries += 1;
            }
        }
        report.unindexed_folders = parent_map.len().saturating_sub(indexed_ids.len());

        report.media = Self::media_resolution_with_ids(db, &rtxn, |fid| parent_map.contains_key(fid))?;
        report.elapsed_ms = t0.elapsed().as_millis();

        Ok(report)
    }

    /// Counts media records whose folder record is missing (such media could no longer be resolved to a path).
    pub fn count_unresolvable_media(db: &LibraryDatabase) -> Result<MediaResolutionReport, LibraryError> {
        let rtxn = db.env.read_txn()?;
        Self::media_resolution_with_ids(db, &rtxn, |fid| matches!(db.folders.get(&rtxn, fid), Ok(Some(_))))
    }

    fn media_resolution_with_ids<F>(
        db: &LibraryDatabase,
        rtxn: &heed::RoTxn,
        folder_exists: F,
    ) -> Result<MediaResolutionReport, LibraryError>
    where
        F: Fn(&FolderId) -> bool,
    {
        let mut report = MediaResolutionReport::default();

        for entry in db.media.iter(rtxn)? {
            match entry {
                Ok((media_id, rec)) => {
                    report.total_media += 1;
                    if !folder_exists(&rec.folder_id) {
                        report.unresolvable_media += 1;
                        if report.samples.len() < MAX_SAMPLES {
                            report.samples.push((media_id, rec.folder_id));
                        }
                    }
                }
                Err(_) => report.undecodable_media += 1,
            }
        }

        Ok(report)
    }

    fn log_report(trigger: &str, r: &FolderAuditReport) {
        tracing::info!(
            trigger,
            total_folders = r.total_folders,
            parentless_folders = r.parentless_folders,
            folders_with_parent = r.folders_with_parent,
            deleted_folders = r.deleted_folders,
            hidden_folders = r.hidden_folders,
            max_depth = r.max_depth,
            duplicate_path_records = r.duplicate_path_records,
            dangling_parent_links = r.dangling_parent_links,
            parent_index_entries = r.parent_index_entries,
            parent_index_children = r.parent_index_children,
            parent_index_mismatches = r.parent_index_mismatches,
            path_index_entries = r.path_index_entries,
            stale_path_index_entries = r.stale_path_index_entries,
            unindexed_folders = r.unindexed_folders,
            root_scope_folders = ?r.root_scope_folders,
            root_registered = ?r.root_registered,
            total_media = r.media.total_media,
            unresolvable_media = r.media.unresolvable_media,
            elapsed_ms = r.elapsed_ms.saturating_as::<u64>(),
            "[FolderAudit: Summary] Folder schema audit completed"
        );

        if r.total_folders > 1 && r.parentless_folders * 2 > r.total_folders {
            tracing::warn!(
                parentless_folders = r.parentless_folders,
                total_folders = r.total_folders,
                "[FolderAudit: FlatHierarchy] Most folder records have no parent link. The tree will only be correct after 'Refresh Tree (Full)'."
            );
        }
        if r.dangling_parent_links > 0 {
            tracing::warn!(
                count = r.dangling_parent_links,
                "[FolderAudit: DanglingParents] Folder records reference parents that do not exist"
            );
        }
        if r.parent_index_mismatches > 0 {
            tracing::warn!(
                count = r.parent_index_mismatches,
                "[FolderAudit: ParentIndexMismatch] folders_by_parent disagrees with folder parent_id links"
            );
        }
        if r.duplicate_path_records > 0 {
            tracing::warn!(
                count = r.duplicate_path_records,
                "[FolderAudit: DuplicatePaths] Multiple folder records share the same path"
            );
        }
        if r.stale_path_index_entries > 0 || r.unindexed_folders > 0 {
            tracing::warn!(
                stale = r.stale_path_index_entries,
                unindexed = r.unindexed_folders,
                "[FolderAudit: PathIndex] folders_by_path is inconsistent with the folders table"
            );
        }
        if r.root_registered == Some(false) {
            tracing::info!(
                "[FolderAudit: RootUnregistered] The configured root is not registered yet. The folder tree stays empty until 'Refresh Tree (Full)' is triggered."
            );
        }
        if r.media.unresolvable_media > 0 {
            tracing::error!(
                count = r.media.unresolvable_media,
                samples = ?r.media.samples,
                "[FolderAudit: UnresolvableMedia] Media records reference folder ids that do not exist"
            );
        }
        if r.undecodable_folders > 0 || r.media.undecodable_media > 0 {
            tracing::warn!(
                folders = r.undecodable_folders,
                media = r.media.undecodable_media,
                "[FolderAudit: Undecodable] Some records could not be decoded"
            );
        }
    }

    /// Convenience used in log lines.
    #[allow(dead_code)]
    pub(crate) fn display_path(path: &Path) -> String {
        LogObscurer::path(path).clone()
    }
}
