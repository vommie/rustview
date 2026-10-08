use super::db::LibraryDatabase;
use super::models::{FolderId, MediaId, MediaRecord};
use crate::media::{MediaFormat, MediaHasher, MediaType, MetadataExtractor};
use crate::settings::LogObscurer;
use crate::thumbnails::{
    PackStore, ThumbnailConfig, ThumbnailGenerator, ThumbnailLocation, VideoPreviewRecord,
};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Detailed summary report returned after completing external media stubs.
#[derive(Debug, Clone, Default)]
pub struct ExternalMediaCompletionReport {
    pub detected_candidates: usize,
    pub completed_count: usize,
    pub still_pending_count: usize,
    pub failed_count: usize,
    pub elapsed_ms: u128,
    pub sources: Vec<String>,
}

/// Processed container holding completed media items ready to be committed and synchronized to UI in-place.
pub struct ProcessedCompletionItem {
    pub media_id: MediaId,
    pub updated_record: MediaRecord,
    pub primary_loc: Option<ThumbnailLocation>,
    pub preview_rec: Option<VideoPreviewRecord>,
    pub primary_pixels: Vec<u8>,
    pub primary_width: u32,
    pub primary_height: u32,
}

/// Dedicated, decoupled subsystem managing the auto-completion of externally registered stub media files
/// (e.g. from CLI API clients like PyCutRv) when their physical files become available and fully written on disk.
pub struct ExternalMediaCompleter<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> ExternalMediaCompleter<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Inspects the given folder IDs for media records created by external tools (`source.is_some()`)
    /// that still lack complete metadata or thumbnails.
    pub fn find_pending_stubs_by_folder_ids(
        &self,
        folder_ids: &[FolderId],
    ) -> (
        Vec<(MediaId, PathBuf, MediaRecord)>,
        usize,
        Vec<String>,
    ) {
        let rtxn = match self.db.env.read_txn() {
            Ok(txn) => txn,
            Err(e) => {
                tracing::error!(error = %e, "[ExternalMediaCompleter] Failed to acquire read txn");
                return (Vec::new(), 0, Vec::new());
            }
        };

        let mut collected_records = Vec::new();
        for &fid in folder_ids {
            if let Ok(Some(ids)) = self.db.media_by_folder.get(&rtxn, &fid) {
                for mid in ids {
                    if let Ok(Some(rec)) = self.db.media.get(&rtxn, &mid) {
                        collected_records.push(rec);
                    }
                }
            }
        }
        drop(rtxn);

        self.find_pending_stubs_in_records(&collected_records)
    }

    /// Inspects an arbitrary slice of MediaRecords (e.g. from Query search results across any scope)
    /// for pending external stubs that are verified to be complete, not open by writers, and ready on disk.
    pub fn find_pending_stubs_in_records(
        &self,
        records: &[MediaRecord],
    ) -> (
        Vec<(MediaId, PathBuf, MediaRecord)>,
        usize,
        Vec<String>,
    ) {
        let t0 = Instant::now();
        let rtxn = match self.db.env.read_txn() {
            Ok(txn) => txn,
            Err(e) => {
                tracing::error!(error = %e, "[ExternalMediaCompleter] Failed to acquire read txn");
                return (Vec::new(), 0, Vec::new());
            }
        };

        let mut ready_candidates = Vec::new();
        let mut still_pending_count = 0usize;
        let mut sources_set = HashSet::new();
        let mut folder_path_cache: std::collections::HashMap<FolderId, PathBuf> =
            std::collections::HashMap::new();

        for rec in records {
            // Check 1: Must be explicitly tagged with an external source application!
            let Some(ref source_name) = rec.source else {
                continue;
            };

            // Check 2: Does this record need completion?
            let has_thumbnail = self
                .db
                .thumbnails
                .get(&rtxn, &rec.id)
                .ok()
                .flatten()
                .is_some();

            let needs_completion = rec.file_size == 0
                || rec.width == 0
                || rec.height == 0
                || rec.is_corrupted
                || !has_thumbnail;

            if !needs_completion {
                continue;
            }

            sources_set.insert(source_name.clone());

            let folder_dir = folder_path_cache.entry(rec.folder_id).or_insert_with(|| {
                self.db
                    .folders
                    .get(&rtxn, &rec.folder_id)
                    .ok()
                    .flatten()
                    .map(|f| PathBuf::from(&f.path))
                    .unwrap_or_default()
            });

            let file_path = folder_dir.join(&rec.filename);
            let format = MediaFormat::from_path(&file_path);
            let is_video = format.media_type() == Some(MediaType::Video);

            // Check 3: Is the file physically complete and ready on the filesystem?
            let (is_ready, pending_reason) = Self::evaluate_file_readiness(&file_path, is_video);

            if is_ready {
                ready_candidates.push((rec.id, file_path, rec.clone()));
            } else {
                still_pending_count += 1;
                tracing::debug!(
                    media_id = rec.id,
                    source = %source_name,
                    file_path = %LogObscurer::path(&file_path),
                    reason = %pending_reason,
                    "[ExternalMediaCompleter: Pending] External stub file is not yet ready for extraction (rendering in queue / active writer)"
                );
            }
        }

        let mut sources_list: Vec<String> = sources_set.into_iter().collect();
        sources_list.sort();

        tracing::info!(
            records_inspected = records.len(),
            ready_candidates = ready_candidates.len(),
            still_pending = still_pending_count,
            sources = ?sources_list,
            elapsed_us = t0.elapsed().as_micros(),
            "[ExternalMediaCompleter: Scan] Completed scan for pending external media stubs"
        );

        (ready_candidates, still_pending_count, sources_list)
    }

    /// Evaluates whether a physical file on disk is ready for thumbnail extraction,
    /// detecting active writers in /proc, size fluctuations, and video container completeness.
    pub fn evaluate_file_readiness(path: &Path, is_video: bool) -> (bool, &'static str) {
        // 1. Existence and File Attributes Check
        let meta1 = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return (false, "File does not exist on physical disk yet"),
        };

        if !meta1.is_file() {
            return (false, "Path is not a regular file");
        }

        let size1 = meta1.len();
        if size1 == 0 {
            return (false, "File size is 0 bytes (placeholder created by external tool)");
        }

        // 2. Linux /proc Active Writer Process Check
        if Self::is_file_open_for_writing(path) {
            return (false, "File is currently held open by an active writer process in /proc");
        }

        // 3. Time Since Last Modification & Size Stability Sampling
        let modified_time = meta1.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let time_since_mod = SystemTime::now()
            .duration_since(modified_time)
            .unwrap_or(Duration::from_secs(9999));

        // If file was touched or modified within the last 4 seconds, verify write stability
        if time_since_mod < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(meta2) = std::fs::metadata(path) {
                if meta2.len() != size1 {
                    return (false, "File size is actively growing (in-flight render)");
                }
                if meta2.modified().ok() != meta1.modified().ok() {
                    return (false, "File timestamp is actively updating (in-flight render)");
                }
            }
        }

        // 4. Video Container Header & Duration Verification
        if is_video {
            match MetadataExtractor::extract(path) {
                Ok(raw_meta) => {
                    if raw_meta.duration_ms == 0 && time_since_mod < Duration::from_secs(60) {
                        return (false, "Video container duration is 0ms (incomplete render header / cues missing)");
                    }
                    if (raw_meta.width == 0 || raw_meta.height == 0)
                        && time_since_mod < Duration::from_secs(60)
                    {
                        return (false, "Video stream dimensions are 0x0 (render stream incomplete)");
                    }
                }
                Err(_) => {
                    if time_since_mod < Duration::from_secs(60) {
                        return (false, "Video container header could not be parsed yet (file still rendering)");
                    }
                }
            }
        }

        (true, "File is complete and ready")
    }

    /// On Linux systems, inspects `/proc` to verify whether any external process
    /// holds an open file descriptor with write access (`O_WRONLY`, `O_RDWR`, or `O_APPEND`) to the target file.
    pub fn is_file_open_for_writing(file_path: &Path) -> bool {
        #[cfg(target_os = "linux")]
        {
            let canonical_target = match file_path.canonicalize() {
                Ok(p) => p,
                Err(_) => return false,
            };

            let proc_dir = Path::new("/proc");
            let entries = match std::fs::read_dir(proc_dir) {
                Ok(e) => e,
                Err(_) => return false,
            };

            let current_pid = std::process::id();

            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let name_str = file_name.to_string_lossy();

                // Only inspect numeric process directories
                let Ok(pid) = name_str.parse::<u32>() else {
                    continue;
                };

                if pid == current_pid {
                    continue;
                }

                let fd_dir = entry.path().join("fd");
                let fd_entries = match std::fs::read_dir(&fd_dir) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                for fd_entry in fd_entries.flatten() {
                    if let Ok(link_target) = std::fs::read_link(fd_entry.path())
                        && link_target == canonical_target
                    {
                        let fd_name = fd_entry.file_name();
                        let fdinfo_path = entry.path().join("fdinfo").join(&fd_name);
                        if let Ok(info_content) = std::fs::read_to_string(&fdinfo_path) {
                            for line in info_content.lines() {
                                if let Some(flags_str) = line.strip_prefix("flags:") {
                                    let trimmed = flags_str.trim();
                                    if let Ok(flags) = u32::from_str_radix(trimmed, 8) {
                                        let acc_mode = flags & 0o3;
                                        if acc_mode == 0o1 || acc_mode == 0o2 || (flags & 0o2000) != 0 {
                                            tracing::debug!(
                                                pid,
                                                file = %LogObscurer::path(file_path),
                                                "[ExternalMediaCompleter: ActiveWriter] File is held open for writing by PID in /proc"
                                            );
                                            return true;
                                        }
                                    }
                                }
                            }
                        } else {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Parallel processing of ready external stub items with interactive live progress reporting:
    /// extracts container metadata, computes MD5, generates thumbnails & video previews,
    /// and atomically commits changes to LMDB.
    pub fn complete_candidates<F>(
        &self,
        candidates: Vec<(MediaId, PathBuf, MediaRecord)>,
        config: &ThumbnailConfig,
        store: &PackStore,
        progress_callback: Option<F>,
    ) -> (
        ExternalMediaCompletionReport,
        Vec<ProcessedCompletionItem>,
    )
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        let t0 = Instant::now();
        let total_candidates = candidates.len();
        if total_candidates == 0 {
            return (ExternalMediaCompletionReport::default(), Vec::new());
        }

        tracing::info!(
            candidates_count = total_candidates,
            "[ExternalMediaCompleter: Execute] Starting multi-core metadata extraction & thumbnail generation with progress reporting..."
        );

        let generator = Arc::new(ThumbnailGenerator::new());
        let cfg = config.clone();

        let progress_counter = Arc::new(AtomicUsize::new(0));
        let cb_arc = progress_callback.map(Arc::new);

        let processed_results: Vec<Result<ProcessedCompletionItem, (MediaId, String)>> = candidates
            .into_par_iter()
            .map(|(media_id, file_path, old_record)| {
                let fs_meta = match std::fs::metadata(&file_path) {
                    Ok(m) => m,
                    Err(e) => return Err((media_id, e.to_string())),
                };

                let file_size = fs_meta.len();
                let mtime = fs_meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(old_record.modification_time);

                let ctime = fs_meta
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(old_record.creation_time);

                // Compute real MD5 checksum of file
                let new_hash = match MediaHasher::hash_file(&file_path) {
                    Ok(h) => h,
                    Err(e) => {
                        return Err((media_id, format!("Hash computation failed: {}", e)));
                    }
                };

                let format = MediaFormat::from_path(&file_path);
                let is_video = format.media_type() == Some(MediaType::Video);

                // Extract metadata (dimensions, duration)
                let (raw_meta, meta_ok) = match MetadataExtractor::extract(&file_path) {
                    Ok(m) => (m, true),
                    Err(e) => {
                        tracing::warn!(
                            media_id,
                            error = %e,
                            "[ExternalMediaCompleter] Metadata extraction warning (file might still be writing)"
                        );
                        return Err((media_id, format!("Metadata extraction failed: {}", e)));
                    }
                };

                // Generate primary thumbnail and video previews
                let (primary_frame_opt, preview_rec) = if is_video {
                    match generator.generate_video_thumbnails(&file_path, &cfg) {
                        Ok(video_res) => {
                            let primary = video_res.primary_frame;
                            let mut prev_rec = None;

                            if !video_res.preview_frames.is_empty() {
                                let (p_format, p_quality) = if cfg.custom_preview_settings {
                                    (cfg.preview_format, cfg.preview_quality)
                                } else {
                                    (cfg.format, cfg.quality)
                                };

                                let mut locations =
                                    Vec::with_capacity(video_res.preview_frames.len());
                                for p_frame in &video_res.preview_frames {
                                    if let Ok(p_bytes) = generator.compress(
                                        &p_frame.rgb_pixels,
                                        p_frame.width,
                                        p_frame.height,
                                        p_format,
                                        p_quality,
                                    ) && let Ok(p_loc) = store.append(&p_bytes)
                                    {
                                        locations.push(p_loc);
                                    }
                                }
                                if !locations.is_empty() {
                                    prev_rec = Some(VideoPreviewRecord {
                                        frame_count: locations.len() as u32,
                                        frame_locations: locations,
                                    });
                                }
                            }

                            (Some(primary), prev_rec)
                        }
                        Err(e) => {
                            return Err((media_id, format!("Video generation failed: {}", e)));
                        }
                    }
                } else {
                    match generator.generate_image_thumbnail(
                        &file_path,
                        cfg.max_bound,
                        cfg.scaling_algorithm,
                    ) {
                        Ok(frame) => (Some(frame), None),
                        Err(e) => {
                            return Err((media_id, format!("Image generation failed: {}", e)));
                        }
                    }
                };

                let primary_loc = if let Some(ref primary_frame) = primary_frame_opt {
                    if let Ok(compressed) = generator.compress(
                        &primary_frame.rgb_pixels,
                        primary_frame.width,
                        primary_frame.height,
                        cfg.format,
                        cfg.quality,
                    ) {
                        store.append(&compressed).ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                let (pixels, pw, ph) = if let Some(f) = primary_frame_opt {
                    (f.rgb_pixels, f.width, f.height)
                } else {
                    (Vec::new(), 0, 0)
                };

                let is_corrupted = file_size == 0 || !meta_ok || (raw_meta.width == 0 && raw_meta.height == 0);

                let updated_record = MediaRecord {
                    id: media_id,
                    folder_id: old_record.folder_id,
                    filename: old_record.filename,
                    file_hash: new_hash,
                    file_size,
                    modification_time: mtime,
                    creation_time: ctime,
                    duration_ms: raw_meta.duration_ms,
                    media_type: if is_video { MediaType::Video } else { MediaType::Image },
                    format,
                    width: raw_meta.width,
                    height: raw_meta.height,
                    rating: old_record.rating,
                    last_seen: 1,
                    is_deleted: false,
                    is_corrupted,
                    source: old_record.source,
                };

                // Report progress to callback
                let cur = progress_counter.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref cb) = cb_arc {
                    cb(cur, total_candidates);
                }

                Ok(ProcessedCompletionItem {
                    media_id,
                    updated_record,
                    primary_loc,
                    preview_rec,
                    primary_pixels: pixels,
                    primary_width: pw,
                    primary_height: ph,
                })
            })
            .collect();

        let mut successful_items = Vec::with_capacity(processed_results.len());
        let mut failed_count = 0usize;

        for res in processed_results {
            match res {
                Ok(item) => successful_items.push(item),
                Err((mid, err)) => {
                    failed_count += 1;
                    tracing::warn!(
                        media_id = mid,
                        error = %err,
                        "[ExternalMediaCompleter: RetryNext] Could not complete stub item on this access (will retry on next access)"
                    );
                }
            }
        }

        // Commit all successful records atomically to LMDB
        if !successful_items.is_empty()
            && let Ok(mut wtxn) = self.db.env.write_txn()
        {
            let current_gen = self.db.next_generation(&mut wtxn).unwrap_or(1);

            for item in &successful_items {
                let mut rec = item.updated_record.clone();
                rec.last_seen = current_gen;
                let _ = self.db.media.put(&mut wtxn, &item.media_id, &rec);

                if let Some(loc) = item.primary_loc {
                    let _ = self.db.thumbnails.put(&mut wtxn, &item.media_id, &loc);
                }
                if let Some(ref p_rec) = item.preview_rec {
                    let _ = self.db.video_previews.put(&mut wtxn, &item.media_id, p_rec);
                }

                // Update hash secondary index
                let _ = self.db.update_media_hash_index(
                    &mut wtxn,
                    item.media_id,
                    None,
                    Some(&rec.file_hash),
                );
            }
            let _ = wtxn.commit();
        }

        let elapsed_ms = t0.elapsed().as_millis();
        let completed_count = successful_items.len();

        tracing::info!(
            total_candidates,
            completed_count,
            failed_count,
            elapsed_ms,
            "[ExternalMediaCompleter: Done] Auto-completion finished and committed to LMDB"
        );

        let report = ExternalMediaCompletionReport {
            detected_candidates: total_candidates,
            completed_count,
            still_pending_count: 0,
            failed_count,
            elapsed_ms,
            sources: Vec::new(),
        };

        (report, successful_items)
    }
}
