use super::stats::MigrationStats;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub struct MigrationLogger {
    file: Option<File>,
    path: Option<PathBuf>,
}

impl MigrationLogger {
    /// Initializes the migration logger. If enabled, creates the log file in `~/` or custom path.
    pub fn new(enabled: bool, custom_path: Option<PathBuf>) -> io::Result<Self> {
        if !enabled {
            return Ok(Self {
                file: None,
                path: None,
            });
        }

        let log_path = match custom_path {
            Some(p) => p,
            None => {
                let timestamp = current_timestamp_string();
                let filename = format!("rustview_migration_{}.log", timestamp);
                if let Ok(home) = std::env::var("HOME") {
                    PathBuf::from(home).join(filename)
                } else {
                    PathBuf::from(filename)
                }
            }
        };

        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)?;

        Ok(Self {
            file: Some(file),
            path: Some(log_path),
        })
    }

    /// Returns the active log file path if logging is enabled.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn log_header(
        &mut self,
        source_db: &Path,
        target_dir: &Path,
        skip_missing: bool,
        root_filter: Option<&Path>,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(f, "================================================================================");
            let _ = writeln!(f, "               RUSTVIEW - XNVIEWMP DATABASE MIGRATION AUDIT LOG                 ");
            let _ = writeln!(f, "================================================================================");
            let _ = writeln!(f, "Timestamp          : {}", current_timestamp_string());
            let _ = writeln!(f, "Source Database    : {}", source_db.display());
            let _ = writeln!(f, "Target RustView Dir: {}", target_dir.display());
            let _ = writeln!(
                f,
                "Base/Root Filter   : {}",
                match root_filter {
                    Some(r) => format!("Active (only paths under '{}')", r.display()),
                    None => "Disabled (all paths imported)".to_string(),
                }
            );
            let _ = writeln!(
                f,
                "Skip Missing Items : {}",
                if skip_missing {
                    "Enabled (non-existent folders & files skipped)"
                } else {
                    "Disabled (all records imported)"
                }
            );
            let _ = writeln!(f, "ID Re-indexing     : Contiguous 1-based sequential mapping");
            let _ = writeln!(f, "--------------------------------------------------------------------------------");
            let _ = writeln!(f, "DETAILED MIGRATION TRACE & AUDIT TRAIL:");
            let _ = writeln!(f);
        }
    }

    pub fn log_folder_outside_root(&mut self, old_id: i64, path: &Path, root_path: &Path) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[FOLDER OUTSIDE]  Folder #{:<5} | Path: {} | Reason: Outside configured base/root path ('{}')",
                old_id,
                path.display(),
                root_path.display()
            );
        }
    }

    pub fn log_folder_skipped(&mut self, old_id: i64, path: &Path, reason: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[FOLDER SKIPPED]  Folder #{:<5} | Path: {} | Reason: {}",
                old_id,
                path.display(),
                reason
            );
        }
    }

    pub fn log_folder_migrated(&mut self, old_id: i64, new_id: u64, path: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[FOLDER MIGRATED] Folder #{:<5} -> #{:<5} | Path: {}",
                old_id, new_id, path
            );
        }
    }

    pub fn log_tag_migrated(
        &mut self,
        old_id: i64,
        new_id: u32,
        name: &str,
        new_parent_id: Option<u32>,
    ) {
        if let Some(ref mut f) = self.file {
            let parent_info = match new_parent_id {
                Some(p) => format!("Parent: #{}", p),
                None => "Parent: Root".to_string(),
            };
            let _ = writeln!(
                f,
                "[TAG MIGRATED]    Tag #{:<5} -> #{:<5} | Name: '{}' ({})",
                old_id, new_id, name, parent_info
            );
        }
    }

    pub fn log_subfolder_entry_skipped(
        &mut self,
        old_image_id: i64,
        name: &str,
        folder_info: &str,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[SUBFOLDER ENTRY] Image #{:<6} | Name: '{}' in {} | Info: Subdirectory entry from XnView browser (handled by folder tree)",
                old_image_id, name, folder_info
            );
        }
    }

    pub fn log_unsupported_file_skipped(
        &mut self,
        old_image_id: i64,
        filename: &str,
        folder_info: &str,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[UNSUPPORTED FMT] Image #{:<6} | File: '{}' in {} | Reason: Non-media file format (not an image or video)",
                old_image_id, filename, folder_info
            );
        }
    }

    pub fn log_media_outside_root(
        &mut self,
        old_image_id: i64,
        filename: &str,
        folder_info: &str,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[MEDIA OUTSIDE]   Image #{:<6} | File: '{}' in {} | Reason: Parent folder is outside of base/root path",
                old_image_id, filename, folder_info
            );
        }
    }

    pub fn log_media_skipped(
        &mut self,
        old_image_id: i64,
        filename: &str,
        folder_info: &str,
        reason: &str,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[MEDIA SKIPPED]   Image #{:<6} | File: '{}' in {} | Reason: {}",
                old_image_id, filename, folder_info, reason
            );
        }
    }

    pub fn log_media_migrated(
        &mut self,
        old_image_id: i64,
        new_media_id: u64,
        new_folder_id: u64,
        filename: &str,
        rating: u8,
        hash_hex: &str,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[MEDIA MIGRATED]  Image #{:<6} -> Media #{:<6} | Folder: #{:<4} | File: '{}' | Rating: {} | Hash: {}",
                old_image_id, new_media_id, new_folder_id, filename, rating, hash_hex
            );
        }
    }

    pub fn log_tag_assignment_migrated(
        &mut self,
        new_media_id: u64,
        new_tag_id: u32,
        old_image_id: i64,
        old_tag_id: i64,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[TAG ASSIGNED]   Media #{:<6} (old #{}) -> Tag #{:<4} (old #{})",
                new_media_id, old_image_id, new_tag_id, old_tag_id
            );
        }
    }

    pub fn log_tag_assignment_skipped(
        &mut self,
        old_image_id: i64,
        old_tag_id: i64,
        reason: &str,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[TAG LINK SKIP]  Image #{:<6} -> Tag #{:<4} | Reason: {}",
                old_image_id, old_tag_id, reason
            );
        }
    }

    pub fn log_external_hash_migrated(
        &mut self,
        old_hash_id: i64,
        new_hash_id: u64,
        hash: &str,
        date_time: &str,
    ) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[HASH MIGRATED]   Hash #{:<5} -> #{:<5} | Hash: {} | Date: {}",
                old_hash_id, new_hash_id, hash, date_time
            );
        }
    }

    pub fn log_summary(&mut self, stats: &MigrationStats) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(f);
            let _ = writeln!(f, "--------------------------------------------------------------------------------");
            let _ = writeln!(f, "FINAL MIGRATION AUDIT SUMMARY:");
            let _ = writeln!(
                f,
                "  • Folders Extracted         : {:>8} | Outside Root: {:>6} | Missing: {:>6} | Migrated: {:>8}",
                stats.extracted_folders,
                stats.skipped_folders_outside_root,
                stats.skipped_folders_missing,
                stats.total_folders
            );
            let _ = writeln!(
                f,
                "  • XnView Images Entries     : {:>8} | Skipped: {:>7} | Migrated: {:>8}",
                stats.extracted_media,
                stats.skipped_folders_outside_root
                    + stats.skipped_media_outside_root
                    + stats.skipped_subfolders_in_images
                    + stats.skipped_unsupported_media
                    + stats.skipped_media_missing_folder
                    + stats.skipped_media_missing_file,
                stats.total_media
            );
            if stats.skipped_media_outside_root > 0 {
                let _ = writeln!(
                    f,
                    "    ├─ Outside Root Skipped   : {:>8}",
                    stats.skipped_media_outside_root
                );
            }
            let _ = writeln!(
                f,
                "    ├─ Subfolders in Images   : {:>8} (directory entries from XnView browser)",
                stats.skipped_subfolders_in_images
            );
            let _ = writeln!(
                f,
                "    ├─ Unsupported Formats    : {:>8} (non-media files: .txt, .py, etc.)",
                stats.skipped_unsupported_media
            );
            let _ = writeln!(
                f,
                "    ├─ Missing Folder Skipped : {:>8}",
                stats.skipped_media_missing_folder
            );
            let _ = writeln!(
                f,
                "    └─ Missing File Skipped   : {:>8}",
                stats.skipped_media_missing_file
            );
            let _ = writeln!(
                f,
                "  • Tags / Categories         : {:>8} | Migrated: {:>8}",
                stats.extracted_tags, stats.total_tags
            );
            let _ = writeln!(
                f,
                "  • Tag Associations          : {:>8} | Skipped: {:>7} | Migrated: {:>8}",
                stats.extracted_tag_assignments,
                stats.skipped_tag_assignments_orphaned,
                stats.total_tag_assignments
            );
            let _ = writeln!(
                f,
                "  • Dedicated External Hashes : {:>8} | Migrated: {:>8}",
                stats.extracted_external_hashes, stats.total_external_hashes
            );
            let _ = writeln!(f, "--------------------------------------------------------------------------------");
            let _ = writeln!(f, "PERFORMANCE TIMINGS:");
            let _ = writeln!(f, "  • SQLite Extraction Phase   : {:>7} ms", stats.read_time_ms);
            let _ = writeln!(f, "  • Mapping & Disk Probing    : {:>7} ms", stats.map_time_ms);
            let _ = writeln!(f, "  • Wipe & LMDB Commit        : {:>7} ms", stats.write_time_ms);
            let _ = writeln!(f, "  • Total Migration Time      : {:>7} ms", stats.total_time_ms);
            let _ = writeln!(f, "================================================================================");
            let _ = f.flush();
        }
    }
}

/// Generates formatted local calendar timestamp string: YYYY-MM-DD_HH-MM-SS.
pub fn current_timestamp_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let sec = secs % 60;
    let mins = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;
    let days = (secs / 86400) as i64;

    // Euclidean affine transform for Gregorian calendar
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        y, m, d, hours, mins, sec
    )
}
