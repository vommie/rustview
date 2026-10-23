use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folders::FolderManager;
use super::models::{FolderId, MediaId, MediaRecord};
use super::schema::SEQ_MEDIA_ID;
use crate::media::{
    DirectoryScanner, FileHash, FormatMask, FormatPolicy, IgnoredFiles, MediaDetector, MediaHasher, MediaType,
    MetadataExtractor, RawMetadata, ScanResult, ScannedFile,
};
use crate::settings::LogObscurer;
use crate::util::cast::SaturatingAs;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RefreshReport {
    pub total_scanned: usize,
    pub new_media: usize,
    pub updated_media: usize,
    pub unchanged_media: usize,
    pub deleted_media: usize,
    pub restored_media: usize,
    pub corrupted_media: usize,
    /// Records of formats that are switched off: left exactly as they are (not scanned, not marked missing).
    pub hidden_format_media: usize,
    /// Files in the folder that were not taken, and why.
    pub ignored: IgnoredFiles,
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
        ctime: i64,
    },
    New {
        scanned: ScannedFile,
        file_size: u64,
        mtime: i64,
        ctime: i64,
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
        ctime: i64,
        was_deleted: bool,
    },
    ModifiedContentChanged {
        existing_id: MediaId,
        old_hash: FileHash,
        new_hash: FileHash,
        file_size: u64,
        mtime: i64,
        ctime: i64,
        raw_meta: RawMetadata,
        filename: String,
        preserved_rating: u8,
        preserved_source: Option<String>,
        is_corrupted: bool,
        was_deleted: bool,
    },
    NewItem {
        filename: String,
        file_hash: FileHash,
        file_size: u64,
        mtime: i64,
        ctime: i64,
        raw_meta: RawMetadata,
        is_corrupted: bool,
    },
    Cancelled,
}

pub struct FolderRefresher<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderRefresher<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Performs an incremental refresh on the target folder with default auto-extension detection enabled.
    pub fn refresh_folder(&self, folder_id: FolderId) -> Result<RefreshReport, LibraryError> {
        self.refresh_folder_with_options(folder_id, true, None, None::<fn(usize, usize)>)
    }

    /// Parallelized incremental folder refresh using Rayon with cancel and progress tracking support.
    pub fn refresh_folder_with_progress<F>(
        &self,
        folder_id: FolderId,
        cancel_flag: Option<&AtomicBool>,
        progress_callback: Option<F>,
    ) -> Result<RefreshReport, LibraryError>
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        self.refresh_folder_with_options(folder_id, true, cancel_flag, progress_callback)
    }

    /// Full-featured incremental folder refresh with explicit control over auto-extension detection for extensionless files.
    pub fn refresh_folder_with_options<F>(
        &self,
        folder_id: FolderId,
        auto_detect_extensionless: bool,
        cancel_flag: Option<&AtomicBool>,
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

        // Auto-detect MIME type & append extension for extensionless files if option is enabled
        if auto_detect_extensionless && folder_path.is_dir() {
            Self::auto_detect_and_fix_extensionless(folder_path, FormatPolicy::mask(), cancel_flag);
        }

        if let Some(flag) = cancel_flag
            && flag.load(Ordering::Acquire)
        {
            tracing::debug!(
                folder_id = folder_id,
                "Folder refresh aborted early due to cancellation"
            );
            return Ok(report);
        }

        tracing::info!(folder = %LogObscurer::folder(Some(folder_id), folder_path), "Scanning folder on filesystem...");

        let mask = FormatPolicy::mask();
        let ScanResult {
            files: scanned_files,
            ignored,
        } = DirectoryScanner::scan_with(folder_path, false, mask);
        report.total_scanned = scanned_files.len();
        report.ignored = ignored;

        tracing::info!(
            folder_id = folder_id,
            scanned = scanned_files.len(),
            "Found supported media files in folder"
        );

        if let Some(flag) = cancel_flag
            && flag.load(Ordering::Acquire)
        {
            tracing::debug!(
                folder_id = folder_id,
                "Folder refresh aborted early due to cancellation"
            );
            return Ok(report);
        }

        // 1. Snapshot existing media records from LMDB and deduplicate by filename
        let rtxn = self.db.env.read_txn()?;
        let existing_ids = self.db.media_by_folder.get(&rtxn, &folder_id)?.unwrap_or_default();

        let mut existing_map: HashMap<String, (MediaId, MediaRecord)> = HashMap::with_capacity(existing_ids.len());
        let mut seen_ids = HashSet::new();
        // Records of switched-off formats are not part of this refresh at all: they are neither compared with the
        // directory (which no longer lists their files) nor marked missing, and they stay in the folder's list.
        let mut hidden_format_ids = Vec::new();

        for id in existing_ids {
            if seen_ids.insert(id)
                && let Some(rec) = self.db.media.get(&rtxn, &id)?
            {
                if mask.shows(rec.format) {
                    existing_map.insert(rec.filename.clone(), (id, rec));
                } else {
                    hidden_format_ids.push(id);
                }
            }
        }
        drop(rtxn);
        report.hidden_format_media = hidden_format_ids.len();

        // 2. Classify scanned items into Unchanged (fast path), Modified, and New
        let mut candidates = Vec::with_capacity(scanned_files.len());

        for scanned in scanned_files {
            if let Some(flag) = cancel_flag
                && flag.load(Ordering::Relaxed)
            {
                return Ok(report);
            }

            let Ok(fs_meta) = std::fs::metadata(&scanned.path) else {
                continue;
            };

            let file_size = fs_meta.len();
            let mtime = fs_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs().saturating_as::<i64>());

            let ctime = fs_meta
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(mtime, |d| d.as_secs().saturating_as::<i64>());

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
                        ctime,
                    });
                }
            } else {
                candidates.push(ProcessingCandidate::New {
                    scanned,
                    file_size,
                    mtime,
                    ctime,
                });
            }
        }

        let total_candidates = candidates.len();
        let processed_counter = Arc::new(AtomicUsize::new(0));

        if let Some(ref cb) = progress_callback {
            cb(0, total_candidates);
        }

        // 3. Parallelize Rayon computation across CPU cores
        let cb_arc = progress_callback.map(Arc::new);
        let counter_ref = processed_counter.clone();
        let cancel_ref = cancel_flag;

        let processed_results: Vec<ProcessedResult> = candidates
            .into_par_iter()
            .map(|candidate| {
                if let Some(flag) = cancel_ref
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
                        ctime,
                    } => {
                        let was_deleted = existing_record.is_deleted;
                        let is_zero_byte = file_size == 0;

                        let (new_hash, hash_ok) = if is_zero_byte {
                            (MediaHasher::hash_bytes(b""), true)
                        } else {
                            match MediaHasher::hash_file(&scanned.path) {
                                Ok(h) => (h, true),
                                Err(_) => (
                                    MediaHasher::hash_bytes(scanned.path.to_string_lossy().as_bytes()),
                                    false,
                                ),
                            }
                        };

                        if let Some(flag) = cancel_ref
                            && flag.load(Ordering::Relaxed)
                        {
                            return ProcessedResult::Cancelled;
                        }

                        let (raw_meta, meta_ok) = if is_zero_byte {
                            (
                                RawMetadata {
                                    file_size: 0,
                                    modification_time: mtime,
                                    creation_time: ctime,
                                    duration_ms: 0,
                                    width: 0,
                                    height: 0,
                                    media_type: scanned.format.media_type().unwrap_or(MediaType::Image),
                                    format: scanned.format,
                                },
                                false,
                            )
                        } else {
                            match MetadataExtractor::extract(&scanned.path) {
                                Ok(m) => (m, true),
                                Err(_) => (
                                    RawMetadata {
                                        file_size,
                                        modification_time: mtime,
                                        creation_time: ctime,
                                        duration_ms: 0,
                                        width: 0,
                                        height: 0,
                                        media_type: scanned.format.media_type().unwrap_or(MediaType::Image),
                                        format: scanned.format,
                                    },
                                    false,
                                ),
                            }
                        };

                        let is_corrupted =
                            is_zero_byte || !hash_ok || !meta_ok || (raw_meta.width == 0 && raw_meta.height == 0);

                        if !is_corrupted && new_hash == existing_record.file_hash && !existing_record.is_corrupted {
                            ProcessedResult::ModifiedHashUnchanged {
                                existing_id,
                                existing_record,
                                file_size,
                                mtime,
                                ctime,
                                was_deleted,
                            }
                        } else {
                            ProcessedResult::ModifiedContentChanged {
                                existing_id,
                                old_hash: existing_record.file_hash,
                                new_hash,
                                file_size,
                                mtime,
                                ctime,
                                raw_meta,
                                filename: scanned.filename,
                                preserved_rating: existing_record.rating,
                                preserved_source: existing_record.source.clone(),
                                is_corrupted,
                                was_deleted,
                            }
                        }
                    }
                    ProcessingCandidate::New {
                        scanned,
                        file_size,
                        mtime,
                        ctime,
                    } => {
                        let is_zero_byte = file_size == 0;

                        let (new_hash, hash_ok) = if is_zero_byte {
                            (MediaHasher::hash_bytes(b""), true)
                        } else {
                            match MediaHasher::hash_file(&scanned.path) {
                                Ok(h) => (h, true),
                                Err(_) => (
                                    MediaHasher::hash_bytes(scanned.path.to_string_lossy().as_bytes()),
                                    false,
                                ),
                            }
                        };

                        if let Some(flag) = cancel_ref
                            && flag.load(Ordering::Relaxed)
                        {
                            return ProcessedResult::Cancelled;
                        }

                        let (raw_meta, meta_ok) = if is_zero_byte {
                            (
                                RawMetadata {
                                    file_size: 0,
                                    modification_time: mtime,
                                    creation_time: ctime,
                                    duration_ms: 0,
                                    width: 0,
                                    height: 0,
                                    media_type: scanned.format.media_type().unwrap_or(MediaType::Image),
                                    format: scanned.format,
                                },
                                false,
                            )
                        } else {
                            match MetadataExtractor::extract(&scanned.path) {
                                Ok(m) => (m, true),
                                Err(_) => (
                                    RawMetadata {
                                        file_size,
                                        modification_time: mtime,
                                        creation_time: ctime,
                                        duration_ms: 0,
                                        width: 0,
                                        height: 0,
                                        media_type: scanned.format.media_type().unwrap_or(MediaType::Image),
                                        format: scanned.format,
                                    },
                                    false,
                                ),
                            }
                        };

                        let is_corrupted =
                            is_zero_byte || !hash_ok || !meta_ok || (raw_meta.width == 0 && raw_meta.height == 0);

                        ProcessedResult::NewItem {
                            filename: scanned.filename,
                            file_hash: new_hash,
                            file_size,
                            mtime,
                            ctime,
                            raw_meta,
                            is_corrupted,
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

        if let Some(flag) = cancel_flag
            && flag.load(Ordering::Acquire)
        {
            tracing::debug!(
                folder_id = folder_id,
                "Folder refresh aborted before commit due to cancellation"
            );
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
                    if existing_record.is_corrupted {
                        report.corrupted_media += 1;
                    }
                    report.unchanged_media += 1;
                }
                ProcessedResult::ModifiedHashUnchanged {
                    existing_id,
                    mut existing_record,
                    file_size,
                    mtime,
                    ctime,
                    was_deleted,
                } => {
                    existing_record.modification_time = mtime;
                    existing_record.creation_time = ctime;
                    existing_record.file_size = file_size;
                    existing_record.last_seen = current_generation;
                    existing_record.is_deleted = false;
                    self.db.media.put(&mut wtxn, &existing_id, &existing_record)?;
                    active_media_ids.push(existing_id);
                    if existing_record.is_corrupted {
                        report.corrupted_media += 1;
                    }
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
                    ctime,
                    raw_meta,
                    filename,
                    preserved_rating,
                    preserved_source,
                    is_corrupted,
                    was_deleted,
                } => {
                    self.db
                        .update_media_hash_index(&mut wtxn, existing_id, Some(&old_hash), Some(&new_hash))?;

                    let updated_record = MediaRecord {
                        id: existing_id,
                        folder_id,
                        filename,
                        file_hash: new_hash,
                        file_size,
                        modification_time: mtime,
                        creation_time: ctime,
                        duration_ms: raw_meta.duration_ms,
                        media_type: raw_meta.media_type,
                        format: raw_meta.format,
                        width: raw_meta.width,
                        height: raw_meta.height,
                        rating: preserved_rating,
                        last_seen: current_generation,
                        is_deleted: false,
                        is_corrupted,
                        source: preserved_source,
                    };

                    self.db.media.put(&mut wtxn, &existing_id, &updated_record)?;
                    active_media_ids.push(existing_id);
                    if is_corrupted {
                        report.corrupted_media += 1;
                    }
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
                    ctime,
                    raw_meta,
                    is_corrupted,
                } => {
                    let new_media_id = self.db.next_id(&mut wtxn, SEQ_MEDIA_ID)?;
                    let new_record = MediaRecord {
                        id: new_media_id,
                        folder_id,
                        filename,
                        file_hash,
                        file_size,
                        modification_time: mtime,
                        creation_time: ctime,
                        duration_ms: raw_meta.duration_ms,
                        media_type: raw_meta.media_type,
                        format: raw_meta.format,
                        width: raw_meta.width,
                        height: raw_meta.height,
                        rating: 0,
                        last_seen: current_generation,
                        is_deleted: false,
                        is_corrupted,
                        source: None,
                    };

                    self.db.media.put(&mut wtxn, &new_media_id, &new_record)?;
                    self.db
                        .update_media_hash_index(&mut wtxn, new_media_id, None, Some(&file_hash))?;

                    active_media_ids.push(new_media_id);
                    if is_corrupted {
                        report.corrupted_media += 1;
                    }
                    report.new_media += 1;
                }
                ProcessedResult::Cancelled => {}
            }
        }

        // 5. Zero-Data-Loss Soft Deletion with absolute deduplication & External Pending Stub Preservation
        let mut all_folder_media_ids = active_media_ids.clone();
        all_folder_media_ids.extend(hidden_format_ids);

        for (deleted_id, mut deleted_record) in existing_map.into_values() {
            // Check whether this record is an external pending stub (e.g. from PyCutRv) that has not finished rendering yet
            let is_external_stub = deleted_record.source.is_some()
                && (deleted_record.file_size == 0
                    || deleted_record.width == 0
                    || deleted_record.height == 0
                    || self.db.thumbnails.get(&wtxn, &deleted_id)?.is_none());

            if is_external_stub {
                // DO NOT mark pending external stubs as deleted! Preserve them as active pending stubs.
                deleted_record.is_deleted = false;
                deleted_record.last_seen = current_generation;
                self.db.media.put(&mut wtxn, &deleted_id, &deleted_record)?;
                active_media_ids.push(deleted_id);
                tracing::debug!(
                    media_id = deleted_id,
                    filename = %LogObscurer::filename(&deleted_record.filename),
                    source = ?deleted_record.source,
                    "[FolderRefresh: PreserveStub] Preserved pending external stub during folder scan"
                );
            } else if !deleted_record.is_deleted {
                deleted_record.is_deleted = true;
                self.db.media.put(&mut wtxn, &deleted_id, &deleted_record)?;
                report.deleted_media += 1;
            }
            all_folder_media_ids.push(deleted_id);
        }

        let mut final_unique_ids = Vec::with_capacity(all_folder_media_ids.len());
        let mut id_dedup = HashSet::new();
        for id in all_folder_media_ids {
            if id_dedup.insert(id) {
                final_unique_ids.push(id);
            }
        }

        self.db.media_by_folder.put(&mut wtxn, &folder_id, &final_unique_ids)?;

        // The tree shows the number of files the folder lists: bring the stored count in step with the folder that was just read
        // (switched-off formats are not part of it, see FolderFormatCounts).
        let shown_now = active_media_ids.len().saturating_as::<i64>();
        let count_delta = match self.db.folders.get(&wtxn, &folder_id)? {
            Some(folder) => shown_now - i64::from(folder.media_count),
            None => 0,
        };
        FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, folder_id, count_delta)?;
        wtxn.commit()?;

        tracing::info!(
            folder_id = folder_id,
            total_active = active_media_ids.len(),
            new = report.new_media,
            updated = report.updated_media,
            unchanged = report.unchanged_media,
            marked_deleted = report.deleted_media,
            restored = report.restored_media,
            corrupted = report.corrupted_media,
            "Folder refresh committed successfully to LMDB"
        );

        Ok(report)
    }

    /// Appends the detected extension to extensionless media files. Files of a switched-off format are left alone: a
    /// disabled format is not handled in any way.
    fn auto_detect_and_fix_extensionless(folder_path: &Path, mask: FormatMask, cancel_flag: Option<&AtomicBool>) {
        let Ok(entries) = std::fs::read_dir(folder_path) else {
            return;
        };

        for entry in entries.flatten() {
            if let Some(flag) = cancel_flag
                && flag.load(Ordering::Relaxed)
            {
                break;
            }

            let Ok(file_type) = entry.file_type() else { continue };

            if !file_type.is_file() {
                continue;
            }

            let path = entry.path();
            let filename = entry.file_name().to_string_lossy().to_string();

            if !filename.starts_with('.')
                && path.extension().is_none()
                && let Some((format, ext)) = MediaDetector::detect_format(&path)
                && mask.is_enabled(format)
            {
                // "photo" becomes "photo.jpg"; only when that name is taken is a counter added ("photo (1).jpg")
                let target_filename = format!("{filename}.{ext}");
                let unique_filename = if folder_path.join(&target_filename).exists() {
                    crate::library::file_operations::FileOperations::generate_unique_filename(
                        folder_path,
                        &target_filename,
                    )
                } else {
                    target_filename
                };
                let new_path = folder_path.join(&unique_filename);

                let t_ren = Instant::now();
                match std::fs::rename(&path, &new_path) {
                    Ok(()) => {
                        tracing::info!(
                            original = %LogObscurer::path(&path),
                            renamed = %LogObscurer::path(&new_path),
                            detected_format = ?format,
                            elapsed_us = t_ren.elapsed().as_micros(),
                            "[AutoExtension: Fix] Detected extensionless media file and appended extension"
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            file = %LogObscurer::path(&path),
                            error = %err,
                            "[AutoExtension: Error] Failed to rename extensionless file"
                        );
                    }
                }
            }
        }
    }
}
