use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{FolderId, MediaId, MediaRecord};
use super::schema::SEQ_MEDIA_ID;
use crate::media::{DirectoryScanner, FileHash, MediaHasher, MetadataExtractor, RawMetadata, ScannedFile};
use crate::settings::LogObscurer;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RefreshReport {
    pub total_scanned: usize,
    pub new_media: usize,
    pub updated_media: usize,
    pub unchanged_media: usize,
    pub deleted_media: usize,
    pub restored_media: usize,
}

enum ProcessingCandidate {
    Unchanged {
        existing_id: MediaId,
        existing_record: MediaRecord,
    },
    Modified {
        scanned: ScannedFile,
        existing_id: MediaId,
        existing_record: MediaRecord,
        file_size: u64,
        mtime: i64,
    },
    New {
        scanned: ScannedFile,
        file_size: u64,
        mtime: i64,
    },
}

enum ProcessedResult {
    Unchanged {
        existing_id: MediaId,
        existing_record: MediaRecord,
    },
    ModifiedHashUnchanged {
        existing_id: MediaId,
        existing_record: MediaRecord,
        file_size: u64,
        mtime: i64,
        was_deleted: bool,
    },
    ModifiedContentChanged {
        existing_id: MediaId,
        old_hash: FileHash,
        new_hash: FileHash,
        file_size: u64,
        mtime: i64,
        raw_meta: RawMetadata,
        filename: String,
        preserved_rating: u8,
        was_deleted: bool,
    },
    NewItem {
        filename: String,
        file_hash: FileHash,
        file_size: u64,
        mtime: i64,
        raw_meta: RawMetadata,
    },
    Failed,
    Cancelled,
}

pub struct FolderRefresher<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderRefresher<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Performs an incremental refresh on the target folder.
    pub fn refresh_folder(&self, folder_id: FolderId) -> Result<RefreshReport, LibraryError> {
        self.refresh_folder_with_progress(folder_id, None, None::<fn(usize, usize)>)
    }

    /// Parallelized incremental folder refresh using Rayon to compute hashes and metadata with cancel support.
    pub fn refresh_folder_with_progress<F>(
        &self,
        folder_id: FolderId,
        cancel_flag: Option<Arc<AtomicBool>>,
        progress_callback: Option<F>,
    ) -> Result<RefreshReport, LibraryError>
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        let mut report = RefreshReport::default();

        let folder_record = {
            let rtxn = self.db.env.read_txn()?;
            self.db
                .folders
                .get(&rtxn, &folder_id)?
                .ok_or(LibraryError::FolderNotFound(folder_id))?
        };

        let folder_path = Path::new(&folder_record.path);
        tracing::info!(folder = %LogObscurer::folder(Some(folder_id), folder_path), "Scanning folder on filesystem...");

        let scanned_files = DirectoryScanner::scan(folder_path, false);
        report.total_scanned = scanned_files.len();

        tracing::info!(
            folder_id = folder_id,
            scanned = scanned_files.len(),
            "Found supported media files in folder"
        );

        if let Some(ref flag) = cancel_flag
            && flag.load(Ordering::Acquire)
        {
            tracing::debug!(folder_id = folder_id, "Folder refresh aborted early due to cancellation");
            return Ok(report);
        }

        // 1. Snapshot existing media records from LMDB
        let rtxn = self.db.env.read_txn()?;
        let existing_ids = self
            .db
            .media_by_folder
            .get(&rtxn, &folder_id)?
            .unwrap_or_default();

        let mut existing_map: HashMap<String, (MediaId, MediaRecord)> =
            HashMap::with_capacity(existing_ids.len());
        for id in existing_ids {
            if let Some(rec) = self.db.media.get(&rtxn, &id)? {
                existing_map.insert(rec.filename.clone(), (id, rec));
            }
        }
        drop(rtxn);

        // 2. Classify scanned items into Unchanged (fast path), Modified, and New
        let mut candidates = Vec::with_capacity(scanned_files.len());

        for scanned in scanned_files {
            if let Some(ref flag) = cancel_flag
                && flag.load(Ordering::Relaxed)
            {
                return Ok(report);
            }

            let fs_meta = match std::fs::metadata(&scanned.path) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(file = %LogObscurer::path(&scanned.path), error = %e, "Cannot read file metadata");
                    continue;
                }
            };

            let file_size = fs_meta.len();
            let mtime = fs_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            if let Some((existing_id, existing_record)) = existing_map.remove(&scanned.filename) {
                if !existing_record.is_deleted
                    && existing_record.file_size == file_size
                    && existing_record.modification_time == mtime
                {
                    candidates.push(ProcessingCandidate::Unchanged {
                        existing_id,
                        existing_record,
                    });
                } else {
                    candidates.push(ProcessingCandidate::Modified {
                        scanned,
                        existing_id,
                        existing_record,
                        file_size,
                        mtime,
                    });
                }
            } else {
                candidates.push(ProcessingCandidate::New {
                    scanned,
                    file_size,
                    mtime,
                });
            }
        }

        let total_candidates = candidates.len();
        let processed_counter = Arc::new(AtomicUsize::new(0));

        if let Some(ref cb) = progress_callback {
            cb(0, total_candidates);
        }

        // 3. Parallelize Rayon computation (MD5 hashing & Metadata extraction) across CPU cores
        let cb_arc = progress_callback.map(Arc::new);
        let counter_ref = processed_counter.clone();
        let cancel_ref = cancel_flag.clone();

        let processed_results: Vec<ProcessedResult> = candidates
            .into_par_iter()
            .map(|candidate| {
                if let Some(ref flag) = cancel_ref
                    && flag.load(Ordering::Relaxed)
                {
                    return ProcessedResult::Cancelled;
                }

                let res = match candidate {
                    ProcessingCandidate::Unchanged {
                        existing_id,
                        existing_record,
                    } => ProcessedResult::Unchanged {
                        existing_id,
                        existing_record,
                    },
                    ProcessingCandidate::Modified {
                        scanned,
                        existing_id,
                        existing_record,
                        file_size,
                        mtime,
                    } => {
                        let was_deleted = existing_record.is_deleted;
                        let new_hash = match MediaHasher::hash_file(&scanned.path) {
                            Ok(h) => h,
                            Err(e) => {
                                tracing::warn!(file = %LogObscurer::path(&scanned.path), error = %e, "Failed to hash modified file");
                                return ProcessedResult::Failed;
                            }
                        };

                        if let Some(ref flag) = cancel_ref
                            && flag.load(Ordering::Relaxed)
                        {
                            return ProcessedResult::Cancelled;
                        }

                        if new_hash == existing_record.file_hash {
                            ProcessedResult::ModifiedHashUnchanged {
                                existing_id,
                                existing_record,
                                file_size,
                                mtime,
                                was_deleted,
                            }
                        } else {
                            let raw_meta = match MetadataExtractor::extract(&scanned.path) {
                                Ok(m) => m,
                                Err(e) => {
                                    tracing::warn!(file = %LogObscurer::path(&scanned.path), error = %e, "Failed to extract metadata for modified file");
                                    return ProcessedResult::Failed;
                                }
                            };

                            ProcessedResult::ModifiedContentChanged {
                                existing_id,
                                old_hash: existing_record.file_hash,
                                new_hash,
                                file_size,
                                mtime,
                                raw_meta,
                                filename: scanned.filename,
                                preserved_rating: existing_record.rating,
                                was_deleted,
                            }
                        }
                    }
                    ProcessingCandidate::New {
                        scanned,
                        file_size,
                        mtime,
                    } => {
                        let new_hash = match MediaHasher::hash_file(&scanned.path) {
                            Ok(h) => h,
                            Err(e) => {
                                tracing::warn!(file = %LogObscurer::path(&scanned.path), error = %e, "Failed to hash new file");
                                return ProcessedResult::Failed;
                            }
                        };

                        if let Some(ref flag) = cancel_ref
                            && flag.load(Ordering::Relaxed)
                        {
                            return ProcessedResult::Cancelled;
                        }

                        let raw_meta = match MetadataExtractor::extract(&scanned.path) {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::warn!(file = %LogObscurer::path(&scanned.path), error = %e, "Failed to extract metadata for new file");
                                return ProcessedResult::Failed;
                            }
                        };

                        ProcessedResult::NewItem {
                            filename: scanned.filename,
                            file_hash: new_hash,
                            file_size,
                            mtime,
                            raw_meta,
                        }
                    }
                };

                let cur = counter_ref.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref cb) = cb_arc {
                    cb(cur, total_candidates);
                }

                res
            })
            .collect();

        if let Some(ref flag) = cancel_flag
            && flag.load(Ordering::Acquire)
        {
            tracing::debug!(folder_id = folder_id, "Folder refresh aborted before commit due to cancellation");
            return Ok(report);
        }

        // 4. Batch Commit to LMDB Storage Engine in a Single Write Transaction
        let mut wtxn = self.db.env.write_txn()?;
        let current_generation = self.db.next_generation(&mut wtxn)?;
        let mut active_media_ids = Vec::with_capacity(processed_results.len());

        for result in processed_results {
            match result {
                ProcessedResult::Unchanged {
                    existing_id,
                    mut existing_record,
                } => {
                    existing_record.last_seen = current_generation;
                    existing_record.is_deleted = false;
                    self.db.media.put(&mut wtxn, &existing_id, &existing_record)?;
                    active_media_ids.push(existing_id);
                    report.unchanged_media += 1;
                }
                ProcessedResult::ModifiedHashUnchanged {
                    existing_id,
                    mut existing_record,
                    file_size,
                    mtime,
                    was_deleted,
                } => {
                    existing_record.modification_time = mtime;
                    existing_record.file_size = file_size;
                    existing_record.last_seen = current_generation;
                    existing_record.is_deleted = false;
                    self.db.media.put(&mut wtxn, &existing_id, &existing_record)?;
                    active_media_ids.push(existing_id);
                    if was_deleted {
                        report.restored_media += 1;
                    } else {
                        report.unchanged_media += 1;
                    }
                }
                ProcessedResult::ModifiedContentChanged {
                    existing_id,
                    old_hash,
                    new_hash,
                    file_size,
                    mtime,
                    raw_meta,
                    filename,
                    preserved_rating,
                    was_deleted,
                } => {
                    self.db.update_media_hash_index(&mut wtxn, existing_id, Some(&old_hash), Some(&new_hash))?;

                    let updated_record = MediaRecord {
                        id: existing_id,
                        folder_id,
                        filename,
                        file_hash: new_hash,
                        file_size,
                        modification_time: mtime,
                        media_type: raw_meta.media_type,
                        format: raw_meta.format,
                        width: raw_meta.width,
                        height: raw_meta.height,
                        rating: preserved_rating,
                        last_seen: current_generation,
                        is_deleted: false,
                    };

                    self.db.media.put(&mut wtxn, &existing_id, &updated_record)?;
                    active_media_ids.push(existing_id);
                    if was_deleted {
                        report.restored_media += 1;
                    } else {
                        report.updated_media += 1;
                    }
                }
                ProcessedResult::NewItem {
                    filename,
                    file_hash,
                    file_size,
                    mtime,
                    raw_meta,
                } => {
                    let new_media_id = self.db.next_id(&mut wtxn, SEQ_MEDIA_ID)?;
                    let new_record = MediaRecord {
                        id: new_media_id,
                        folder_id,
                        filename,
                        file_hash,
                        file_size,
                        modification_time: mtime,
                        media_type: raw_meta.media_type,
                        format: raw_meta.format,
                        width: raw_meta.width,
                        height: raw_meta.height,
                        rating: 0,
                        last_seen: current_generation,
                        is_deleted: false,
                    };

                    self.db.media.put(&mut wtxn, &new_media_id, &new_record)?;
                    self.db.update_media_hash_index(&mut wtxn, new_media_id, None, Some(&file_hash))?;

                    active_media_ids.push(new_media_id);
                    report.new_media += 1;
                }
                ProcessedResult::Failed | ProcessedResult::Cancelled => {}
            }
        }

        // 5. Zero-Data-Loss Soft Deletion: Mark missing files as is_deleted = true, strictly preserving tags & ratings
        let mut all_folder_media_ids = active_media_ids.clone();

        for (deleted_id, mut deleted_record) in existing_map.into_values() {
            if !deleted_record.is_deleted {
                deleted_record.is_deleted = true;
                self.db.media.put(&mut wtxn, &deleted_id, &deleted_record)?;
                report.deleted_media += 1;
            }
            all_folder_media_ids.push(deleted_id);
        }

        self.db.media_by_folder.put(&mut wtxn, &folder_id, &all_folder_media_ids)?;
        wtxn.commit()?;

        tracing::info!(
            folder_id = folder_id,
            total_active = active_media_ids.len(),
            new = report.new_media,
            updated = report.updated_media,
            unchanged = report.unchanged_media,
            marked_deleted = report.deleted_media,
            restored = report.restored_media,
            "Folder refresh committed successfully to LMDB"
        );

        Ok(report)
    }
}
