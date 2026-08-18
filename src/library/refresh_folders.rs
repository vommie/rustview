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

pub struct FolderTreeRefresher<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderTreeRefresher<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Parallelized recursive directory scan and LMDB synchronization.
    /// Uses Linux d_type getdents64 extraction without regular file stat calls across all CPU threads.
    pub fn refresh_tree<F>(
        &self,
        root_path: &Path,
        progress_callback: Option<F>,
    ) -> Result<FolderTreeRefreshReport, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let t_total = Instant::now();
        let canonical_root = match root_path.canonicalize() {
            Ok(p) => p,
            Err(_) => root_path.to_path_buf(),
        };

        let root_str = canonical_root.to_string_lossy().to_string();
        tracing::info!(
            root = %LogObscurer::path(&canonical_root),
            "[Benchmark: FolderTreeScan] Starting parallel multi-threaded directory discovery..."
        );

        // 1. Ultra-fast multi-core recursive directory discovery (Zero inner canonicalize syscalls)
        let t_fs_discovery = Instant::now();
        let discovered =
            Self::discover_directories_parallel(&canonical_root, progress_callback.as_ref())?;
        let total_scanned = discovered.len();
        let fs_elapsed_ms = t_fs_discovery.elapsed().as_millis();

        tracing::info!(
            root = %LogObscurer::path(&canonical_root),
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

        // Ensure root folder itself is registered
        let root_folder_id = match self.db.folders_by_path.get(&wtxn, &root_str)? {
            Some(id) => {
                if let Some(mut root_record) = self.db.folders.get(&wtxn, &id)?
                    && root_record.is_deleted
                {
                    root_record.is_deleted = false;
                    self.db.folders.put(&mut wtxn, &id, &root_record)?;
                    report.folders_restored += 1;
                }
                id
            }
            None => {
                let new_id = self.db.next_id(&mut wtxn, SEQ_FOLDER_ID)?;
                let record = FolderRecord {
                    id: new_id,
                    path: root_str.clone(),
                    added_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                    is_deleted: false,
                };
                self.db.folders.put(&mut wtxn, &new_id, &record)?;
                self.db.folders_by_path.put(&mut wtxn, &root_str, &new_id)?;
                self.db
                    .media_by_folder
                    .put(&mut wtxn, &new_id, &Vec::new())?;
                report.new_folders_added += 1;
                new_id
            }
        };

        // Load all existing folders under root from LMDB
        let mut existing_in_root: HashMap<String, (FolderId, FolderRecord)> = HashMap::new();
        for entry in self.db.folders.iter(&wtxn)? {
            let (id, record) = entry?;
            let path_obj = Path::new(&record.path);
            if path_obj.starts_with(&canonical_root) && record.id != root_folder_id {
                existing_in_root.insert(record.path.clone(), (id, record));
            }
        }

        // Process all discovered subdirectories
        for dir_path in discovered {
            if dir_path == canonical_root {
                continue;
            }

            let path_str = dir_path.to_string_lossy().to_string();

            if let Some((id, mut existing_rec)) = existing_in_root.remove(&path_str) {
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

        // Any remaining records in existing_in_root are missing on physical disk -> mark is_deleted = true
        for (id, mut missing_rec) in existing_in_root.into_values() {
            if !missing_rec.is_deleted {
                missing_rec.is_deleted = true;
                self.db.folders.put(&mut wtxn, &id, &missing_rec)?;
                report.folders_marked_deleted += 1;
            }
        }

        wtxn.commit()?;
        let lmdb_elapsed_ms = t_lmdb_sync.elapsed().as_millis();

        tracing::info!(
            total_scanned = report.total_scanned_dirs,
            new_added = report.new_folders_added,
            marked_deleted = report.folders_marked_deleted,
            restored = report.folders_restored,
            unchanged = report.unchanged_folders,
            fs_discovery_ms = fs_elapsed_ms,
            lmdb_sync_ms = lmdb_elapsed_ms,
            total_time_ms = t_total.elapsed().as_millis(),
            "[Benchmark: FolderTreeCommit] Folder tree refresh transaction committed to LMDB storage."
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

        let cur = counter.fetch_add(added_count, Ordering::Relaxed);
        if let Some(cb) = progress_callback
            && cur.is_multiple_of(256)
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
                        let cur = counter.fetch_add(1, Ordering::Relaxed);
                        if let Some(cb) = progress_callback
                            && cur.is_multiple_of(256)
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
