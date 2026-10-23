use super::stats::MigrationStats;
use super::types::MigratedTagTreeNode;
use crate::settings::LogObscurer;
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
            return Ok(Self { file: None, path: None });
        }

        let log_path = if let Some(p) = custom_path {
            p
        } else {
            let timestamp = current_timestamp_string();
            let filename = format!("rustview_migration_{timestamp}.log");
            if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(home).join(filename)
            } else {
                PathBuf::from(filename)
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

    pub fn log_header(&mut self, source_db: &Path, target_dir: &Path, skip_missing: bool, root_filter: Option<&Path>) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "================================================================================"
            );
            let _ = writeln!(
                f,
                "               RUSTVIEW - XNVIEWMP DATABASE MIGRATION AUDIT LOG                 "
            );
            let _ = writeln!(
                f,
                "================================================================================"
            );
            let _ = writeln!(f, "Timestamp          : {}", current_timestamp_string());
            let _ = writeln!(f, "Source Database    : {}", LogObscurer::path(source_db));
            let _ = writeln!(f, "Target RustView Dir: {}", LogObscurer::path(target_dir));
            let _ = writeln!(
                f,
                "Base/Root Filter   : {}",
                match root_filter {
                    Some(r) => format!("Active (only paths under '{}')", LogObscurer::path(r)),
                    None => "Disabled (all paths imported)".to_string(),
                }
            );
            let _ = writeln!(
                f,
                "Log Privacy Mode   : {}",
                if LogObscurer::is_active() {
                    "Enabled (Obfuscated IDs and surrogate hashes)"
                } else {
                    "Disabled (Plain text filenames and paths)"
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
            let _ = writeln!(
                f,
                "--------------------------------------------------------------------------------"
            );
            let _ = writeln!(f, "DETAILED MIGRATION TRACE & AUDIT TRAIL:");
            let _ = writeln!(f);
            let _ = f.flush();
        }
    }

    pub fn log_folder_outside_root(&mut self, old_id: i64, path: &Path, root_path: &Path) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[FOLDER OUTSIDE]  Folder #{:<5} | Path: {} | Reason: Outside configured base/root path ('{}')",
                old_id,
                LogObscurer::folder(None, path),
                LogObscurer::path(root_path)
            );
        }
    }

    pub fn log_folder_skipped(&mut self, old_id: i64, path: &Path, reason: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[FOLDER SKIPPED]  Folder #{:<5} | Path: {} | Reason: {}",
                old_id,
                LogObscurer::folder(None, path),
                reason
            );
        }
    }

    pub fn log_folder_migrated(&mut self, old_id: i64, new_id: u64, path: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[FOLDER MIGRATED] Folder #{:<5} -> Folder #{:<5} | Path: {}",
                old_id,
                new_id,
                LogObscurer::folder(Some(new_id), path)
            );
        }
    }

    pub fn log_tag_migrated(&mut self, old_id: i64, new_id: u32, name: &str, new_parent_id: Option<u32>) {
        if let Some(ref mut f) = self.file {
            let parent_info = match new_parent_id {
                Some(p) => format!("Parent: Tag #{p}"),
                None => "Parent: Root".to_string(),
            };
            let _ = writeln!(
                f,
                "[TAG MIGRATED]    Tag #{:<5} -> Tag #{:<5} | Name: {} ({})",
                old_id,
                new_id,
                LogObscurer::tag(Some(new_id), name),
                parent_info
            );
        }
    }

    /// Renders a full visual ASCII Tag Tree diagram showing exact hierarchical relationships and ID mapping.
    pub fn log_tag_tree(&mut self, root_nodes: &[MigratedTagTreeNode]) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(f);
            let _ = writeln!(
                f,
                "--------------------------------------------------------------------------------"
            );
            let _ = writeln!(f, "MAPPED TAG TREE HIERARCHY (XnViewMP -> RustView):");
            if root_nodes.is_empty() {
                let _ = writeln!(f, "  <No tags found in XnViewMP database>");
            } else {
                let mut lines = Vec::new();
                let total_roots = root_nodes.len();
                for (idx, root_node) in root_nodes.iter().enumerate() {
                    let is_last = idx + 1 == total_roots;
                    Self::render_tree_node(root_node, "", is_last, &mut lines);
                }
                for line in lines {
                    let _ = writeln!(f, "{line}");
                }
            }
            let _ = writeln!(
                f,
                "--------------------------------------------------------------------------------"
            );
            let _ = writeln!(f);
            let _ = f.flush();
        }
    }

    fn render_tree_node(node: &MigratedTagTreeNode, prefix: &str, is_last: bool, lines: &mut Vec<String>) {
        let display_name = if LogObscurer::is_active() {
            format!("Tag#{}", node.new_id)
        } else {
            node.name.clone()
        };

        let branch = if is_last { "└── " } else { "├── " };
        let line = format!(
            "{}{}[#{} -> #{}] {}",
            prefix, branch, node.old_id, node.new_id, display_name
        );
        lines.push(line);

        let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
        let total_children = node.children.len();
        for (idx, child) in node.children.iter().enumerate() {
            let child_is_last = idx + 1 == total_children;
            Self::render_tree_node(child, &child_prefix, child_is_last, lines);
        }
    }

    pub fn log_subfolder_entry_skipped(&mut self, old_image_id: i64, name: &str, folder_info: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[SUBFOLDER ENTRY] Image #{:<6} | Name: {} in {} | Info: Subdirectory entry from XnView browser (handled by folder tree)",
                old_image_id,
                LogObscurer::filename(name),
                folder_info
            );
        }
    }

    pub fn log_unsupported_file_skipped(&mut self, old_image_id: i64, filename: &str, folder_info: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[UNSUPPORTED FMT] Image #{:<6} | File: {} in {} | Reason: Non-media file format (not an image or video)",
                old_image_id,
                LogObscurer::filename(filename),
                folder_info
            );
        }
    }

    pub fn log_media_outside_root(&mut self, old_image_id: i64, filename: &str, folder_info: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[MEDIA OUTSIDE]   Image #{:<6} | File: {} in {} | Reason: Parent folder is outside of base/root path",
                old_image_id,
                LogObscurer::filename(filename),
                folder_info
            );
        }
    }

    pub fn log_media_skipped(&mut self, old_image_id: i64, filename: &str, folder_info: &str, reason: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[MEDIA SKIPPED]   Image #{:<6} | File: {} in {} | Reason: {}",
                old_image_id,
                LogObscurer::filename(filename),
                folder_info,
                reason
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
                "[MEDIA MIGRATED]  Image #{:<6} -> Media #{:<6} | Folder: #{:<4} | File: {} | Rating: {} | Hash: {}",
                old_image_id,
                new_media_id,
                new_folder_id,
                LogObscurer::media(Some(new_media_id), filename),
                rating,
                hash_hex
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
                "[TAG ASSIGNED]   Media #{new_media_id:<6} (old #{old_image_id}) -> Tag #{new_tag_id:<4} (old #{old_tag_id})"
            );
        }
    }

    pub fn log_tag_assignment_skipped(&mut self, old_image_id: i64, old_tag_id: i64, reason: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[TAG LINK SKIP]  Image #{old_image_id:<6} -> Tag #{old_tag_id:<4} | Reason: {reason}"
            );
        }
    }

    pub fn log_external_hash_migrated(&mut self, old_hash_id: i64, new_hash_id: u64, hash: &str, date_time: &str) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(
                f,
                "[HASH MIGRATED]   Hash #{old_hash_id:<5} -> Hash #{new_hash_id:<5} | Hash: {hash} | Date: {date_time}"
            );
        }
    }

    pub fn log_summary(&mut self, stats: &MigrationStats) {
        if let Some(ref mut f) = self.file {
            let _ = writeln!(f);
            let _ = writeln!(
                f,
                "--------------------------------------------------------------------------------"
            );
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
                stats.extracted_tag_assignments, stats.skipped_tag_assignments_orphaned, stats.total_tag_assignments
            );
            let _ = writeln!(
                f,
                "  • Dedicated External Hashes : {:>8} | Migrated: {:>8}",
                stats.extracted_external_hashes, stats.total_external_hashes
            );
            let _ = writeln!(
                f,
                "--------------------------------------------------------------------------------"
            );
            let _ = writeln!(f, "PERFORMANCE TIMINGS:");
            let _ = writeln!(f, "  • SQLite Extraction Phase   : {:>7} ms", stats.read_time_ms);
            let _ = writeln!(f, "  • Mapping & Disk Probing    : {:>7} ms", stats.map_time_ms);
            let _ = writeln!(f, "  • Wipe & LMDB Commit        : {:>7} ms", stats.write_time_ms);
            let _ = writeln!(f, "  • Total Migration Time      : {:>7} ms", stats.total_time_ms);
            let _ = writeln!(
                f,
                "================================================================================"
            );
            let _ = f.flush();
        }
    }
}

/// Generates formatted local calendar timestamp string: YYYY-MM-DD_HH-MM-SS.
pub fn current_timestamp_string() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}
