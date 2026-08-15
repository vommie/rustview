use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, FolderRecord};
use super::schema::SEQ_FOLDER_ID;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
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

    /// High-performance recursive directory scan and LMDB synchronization.
    /// Optimized for Linux filesystem traversal without inspecting or loading regular files.
    pub fn refresh_tree<F>(
        &self,
        root_path: &Path,
        progress_callback: Option<F>,
    ) -> Result<FolderTreeRefreshReport, LibraryError>
    where
        F: Fn(usize),
    {
        let t0 = Instant::now();
        let canonical_root = match root_path.canonicalize() {
            Ok(p) => p,
            Err(_) => root_path.to_path_buf(),
        };

        let root_str = canonical_root.to_string_lossy().to_string();
        tracing::info!(root = %root_str, "Starting recursive high-speed directory discovery on Linux filesystem...");

        // 1. Recursive pure-directory discovery
        let discovered =
            Self::discover_directories_linux(&canonical_root, progress_callback.as_ref())?;
        let total_scanned = discovered.len();

        tracing::info!(
            root = %root_str,
            discovered_dirs = total_scanned,
            scan_elapsed_ms = t0.elapsed().as_millis(),
            "Physical directory discovery completed. Synchronizing with LMDB storage..."
        );

        // 2. Synchronize with LMDB in a single atomic write transaction
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

        tracing::info!(
            total_scanned = report.total_scanned_dirs,
            new_added = report.new_folders_added,
            marked_deleted = report.folders_marked_deleted,
            restored = report.folders_restored,
            unchanged = report.unchanged_folders,
            total_time_ms = t0.elapsed().as_millis(),
            "Committed folder tree refresh transaction to LMDB storage."
        );

        Ok(report)
    }

    /// High-performance recursive directory discovery avoiding stat(2) calls via d_type extraction.
    fn discover_directories_linux<F>(
        root: &Path,
        progress_callback: Option<&F>,
    ) -> Result<Vec<PathBuf>, LibraryError>
    where
        F: Fn(usize),
    {
        let mut discovered = Vec::with_capacity(1024);
        let mut queue = VecDeque::with_capacity(256);
        let mut visited_paths = HashSet::new();

        queue.push_back(root.to_path_buf());
        visited_paths.insert(root.to_path_buf());
        discovered.push(root.to_path_buf());

        let mut count = 1;
        let mut last_progress_report = Instant::now();

        while let Some(current_dir) = queue.pop_front() {
            let read_dir_res = std::fs::read_dir(&current_dir);
            let entries = match read_dir_res {
                Ok(it) => it,
                Err(e) => {
                    tracing::warn!(dir = ?current_dir, error = %e, "Skipping unreadable directory");
                    continue;
                }
            };

            for entry_res in entries {
                let entry = match entry_res {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                // On Linux, file_type() uses d_type from getdents64 with ZERO stat syscalls
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                if is_dir {
                    let path = entry.path();
                    let canonical = match path.canonicalize() {
                        Ok(p) => p,
                        Err(_) => path,
                    };

                    if visited_paths.insert(canonical.clone()) {
                        queue.push_back(canonical.clone());
                        discovered.push(canonical);
                        count += 1;

                        if let Some(cb) = progress_callback
                            && last_progress_report.elapsed().as_millis() >= 80
                        {
                            cb(count);
                            last_progress_report = Instant::now();
                        }
                    }
                }
            }
        }

        if let Some(cb) = progress_callback {
            cb(count);
        }

        Ok(discovered)
    }
}
