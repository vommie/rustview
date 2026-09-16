use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, FolderRecord};
use super::schema::SEQ_FOLDER_ID;
use crate::settings::LogObscurer;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FolderTreeRefreshReport {
    pub total_scanned_dirs: usize,
    pub new_folders_added: usize,
    pub folders_marked_deleted: usize,
    pub folders_restored: usize,
    pub unchanged_folders: usize,
}

impl FolderTreeRefreshReport {
    /// Returns true if any filesystem structure changes were detected (additions, deletions, or restorations).
    #[inline]
    pub fn has_changes(&self) -> bool {
        self.new_folders_added > 0 || self.folders_marked_deleted > 0 || self.folders_restored > 0
    }
}

pub struct FolderTreeRefresher<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderTreeRefresher<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Parallelized recursive directory scan and LMDB synchronization for the entire library tree.
    pub fn refresh_tree<F>(
        &self,
        root_path: &Path,
        progress_callback: Option<F>,
    ) -> Result<FolderTreeRefreshReport, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.refresh_path(root_path, progress_callback)
    }

    /// Targeted, highly efficient recursive directory scan and LMDB synchronization for a specific subfolder subtree.
    pub fn refresh_subtree<F>(
        &self,
        target_path: &Path,
        progress_callback: Option<F>,
    ) -> Result<FolderTreeRefreshReport, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.refresh_path(target_path, progress_callback)
    }

    /// Core engine performing parallel directory discovery and atomic LMDB synchronization for any root or subtree path.
    pub fn refresh_path<F>(
        &self,
        base_path: &Path,
        progress_callback: Option<F>,
    ) -> Result<FolderTreeRefreshReport, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let t_total = Instant::now();
        let canonical_base = match base_path.canonicalize() {
            Ok(p) => p,
            Err(_) => base_path.to_path_buf(),
        };

        let base_str = canonical_base.to_string_lossy().to_string();
        tracing::info!(
            target_base = %LogObscurer::path(&canonical_base),
            "[Benchmark: FolderTreeScan] Starting parallel multi-threaded directory discovery..."
        );

        // 1. Ultra-fast multi-core recursive directory discovery (Zero inner canonicalize syscalls)
        let t_fs_discovery = Instant::now();
        let discovered =
            Self::discover_directories_parallel(&canonical_base, progress_callback.as_ref())?;
        let total_scanned = discovered.len();
        let fs_elapsed_ms = t_fs_discovery.elapsed().as_millis();

        tracing::info!(
            target_base = %LogObscurer::path(&canonical_base),
            discovered_dirs = total_scanned,
            fs_discovery_ms = fs_elapsed_ms,
            "[Benchmark: FolderTreeScan] Parallel directory discovery completed. Beginning LMDB sync transaction..."
        );

        // 2. Synchronize with LMDB in a single atomic write transaction
        let t_lmdb_sync = Instant::now();
        let mut wtxn = self.db.env.write_txn()?;
        let mut report = FolderTreeRefreshReport {
            total_scanned_dirs: total_scanned,
            ..Default::default()
        };

        // Ensure base folder itself is registered
        let base_folder_id = match self.db.folders_by_path.get(&wtxn, &base_str)? {
            Some(id) => {
                if let Some(mut base_record) = self.db.folders.get(&wtxn, &id)?
                    && base_record.is_deleted
                {
                    base_record.is_deleted = false;
                    self.db.folders.put(&mut wtxn, &id, &base_record)?;
                    report.folders_restored += 1;
                }
                id
            }
            None => {
                let new_id = self.db.next_id(&mut wtxn, SEQ_FOLDER_ID)?;
                let record = FolderRecord {
                    id: new_id,
                    path: base_str.clone(),
                    added_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                    is_deleted: false,
                };
                self.db.folders.put(&mut wtxn, &new_id, &record)?;
                self.db.folders_by_path.put(&mut wtxn, &base_str, &new_id)?;
                self.db
                    .media_by_folder
                    .put(&mut wtxn, &new_id, &Vec::new())?;
                report.new_folders_added += 1;
                new_id
            }
        };

        // Load all existing folders under base from LMDB
        let mut existing_in_base: HashMap<String, (FolderId, FolderRecord)> = HashMap::new();
        for entry in self.db.folders.iter(&wtxn)? {
            let (id, record) = entry?;
            let path_obj = Path::new(&record.path);
            if path_obj.starts_with(&canonical_base) && record.id != base_folder_id {
                existing_in_base.insert(record.path.clone(), (id, record));
            }
        }

        // Process all discovered subdirectories
        for dir_path in discovered {
            if dir_path == canonical_base {
                continue;
            }

            let path_str = dir_path.to_string_lossy().to_string();

            if let Some((id, mut existing_rec)) = existing_in_base.remove(&path_str) {
                if existing_rec.is_deleted {
                    existing_rec.is_deleted = false;
                    self.db.folders.put(&mut wtxn, &id, &existing_rec)?;
                    report.folders_restored += 1;
                } else {
                    report.unchanged_folders += 1;
                }
            } else {
                // Newly discovered folder
                let new_id = self.db.next_id(&mut wtxn, SEQ_FOLDER_ID)?;
                let record = FolderRecord {
                    id: new_id,
                    path: path_str.clone(),
                    added_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                    is_deleted: false,
                };
                self.db.folders.put(&mut wtxn, &new_id, &record)?;
                self.db.folders_by_path.put(&mut wtxn, &path_str, &new_id)?;
                self.db
                    .media_by_folder
                    .put(&mut wtxn, &new_id, &Vec::new())?;
                report.new_folders_added += 1;
            }
        }

        // Any remaining records in existing_in_base are missing on physical disk -> mark is_deleted = true
        for (id, mut missing_rec) in existing_in_base.into_values() {
            if !missing_rec.is_deleted {
                missing_rec.is_deleted = true;
                self.db.folders.put(&mut wtxn, &id, &missing_rec)?;
                report.folders_marked_deleted += 1;
            }
        }

        wtxn.commit()?;
        let lmdb_elapsed_ms = t_lmdb_sync.elapsed().as_millis();

        tracing::info!(
            target_base = %LogObscurer::path(&canonical_base),
            total_scanned = report.total_scanned_dirs,
            new_added = report.new_folders_added,
            marked_deleted = report.folders_marked_deleted,
            restored = report.folders_restored,
            unchanged = report.unchanged_folders,
            has_changes = report.has_changes(),
            fs_discovery_ms = fs_elapsed_ms,
            lmdb_sync_ms = lmdb_elapsed_ms,
            total_time_ms = t_total.elapsed().as_millis(),
            "[Benchmark: FolderTreeCommit] Folder tree scan transaction committed to LMDB storage."
        );

        Ok(report)
    }

    /// Multi-core parallel directory discovery with dynamic Rayon work-stealing and zero inner canonicalize syscalls.
    fn discover_directories_parallel<F>(
        root: &Path,
        progress_callback: Option<&F>,
    ) -> Result<Vec<PathBuf>, LibraryError>
    where
        F: Fn(usize) + Send + Sync,
    {
        let counter = Arc::new(AtomicUsize::new(1));
        let results = Self::scan_dir_parallel(root.to_path_buf(), 0, &counter, progress_callback);

        let total = counter.load(Ordering::Relaxed);
        if let Some(cb) = progress_callback {
            cb(total);
        }

        Ok(results)
    }

    fn scan_dir_parallel<F>(
        dir: PathBuf,
        depth: usize,
        counter: &Arc<AtomicUsize>,
        progress_callback: Option<&F>,
    ) -> Vec<PathBuf>
    where
        F: Fn(usize) + Send + Sync,
    {
        let mut local_dirs = Vec::with_capacity(32);
        let mut sub_dirs = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                // On Linux, file_type() uses d_type directly without any stat syscalls
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    sub_dirs.push(entry.path());
                }
            }
        }

        let added_count = sub_dirs.len();
        local_dirs.push(dir);

        let cur = counter.fetch_add(added_count, Ordering::Relaxed) + added_count;
        if let Some(cb) = progress_callback
            && (cur.is_multiple_of(32) || cur == added_count)
        {
            cb(cur);
        }

        // Dynamically parallelize across Rayon threads for top 3 hierarchy levels
        if depth < 3 && sub_dirs.len() > 1 {
            let child_results: Vec<Vec<PathBuf>> = sub_dirs
                .into_par_iter()
                .map(|sub| Self::scan_dir_parallel(sub, depth + 1, counter, progress_callback))
                .collect();

            for mut c in child_results {
                local_dirs.append(&mut c);
            }
        } else {
            for sub in sub_dirs {
                let mut c = Self::scan_dir_sequential(sub, counter, progress_callback);
                local_dirs.append(&mut c);
            }
        }

        local_dirs
    }

    fn scan_dir_sequential<F>(
        dir: PathBuf,
        counter: &Arc<AtomicUsize>,
        progress_callback: Option<&F>,
    ) -> Vec<PathBuf>
    where
        F: Fn(usize) + Send + Sync,
    {
        let mut results = Vec::with_capacity(64);
        let mut queue = std::collections::VecDeque::with_capacity(32);
        queue.push_back(dir);

        while let Some(current) = queue.pop_front() {
            if let Ok(entries) = std::fs::read_dir(&current) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                        let p = entry.path();
                        queue.push_back(p);
                        let cur = counter.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(cb) = progress_callback
                            && (cur.is_multiple_of(32) || cur == 1)
                        {
                            cb(cur);
                        }
                    }
                }
            }
            results.push(current);
        }

        results
    }
}
