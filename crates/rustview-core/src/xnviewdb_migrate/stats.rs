use crate::settings::LogObscurer;

#[derive(Debug, Default, Clone)]
pub struct MigrationStats {
    pub source_path: String,
    pub source_size_bytes: u64,
    pub source_version: String,
    pub root_filter_path: Option<String>,
    pub skip_missing_enabled: bool,
    pub hashes_table_found: bool,

    // Source Counts
    pub extracted_folders: usize,
    pub extracted_media: usize,
    pub extracted_tags: usize,
    pub extracted_tag_assignments: usize,
    pub extracted_external_hashes: usize,

    // Categorized Skipped Counts
    pub skipped_folders_outside_root: usize,
    pub skipped_folders_missing: usize,
    pub skipped_media_outside_root: usize,
    pub skipped_subfolders_in_images: usize,
    pub skipped_unsupported_media: usize,
    pub skipped_media_missing_folder: usize,
    pub skipped_media_missing_file: usize,
    pub skipped_tag_assignments_orphaned: usize,

    // Successfully Migrated Counts
    pub total_folders: usize,
    pub total_media: usize,
    pub total_tags: usize,
    pub total_tag_assignments: usize,
    pub total_external_hashes: usize,

    // Hash Resolution
    pub resolved_hashes_from_table: usize,
    pub resolved_hashes_computed_live: usize,
    pub fallback_hashes_generated: usize,

    // Timings
    pub read_time_ms: u128,
    pub map_time_ms: u128,
    pub write_time_ms: u128,
    pub total_time_ms: u128,

    pub warnings: Vec<String>,
}

impl MigrationStats {
    pub fn new(source_path: &str, skip_missing_enabled: bool, root_filter: Option<&str>) -> Self {
        let size = std::fs::metadata(source_path).map_or(0, |m| m.len());
        Self {
            source_path: source_path.to_string(),
            source_size_bytes: size,
            root_filter_path: root_filter.map(std::string::ToString::to_string),
            skip_missing_enabled,
            hashes_table_found: false,
            ..Default::default()
        }
    }

    /// Renders a structured CLI summary report.
    pub fn print_summary(&self) {
        println!();
        println!("================================================================================");
        println!("              RUSTVIEW - XNVIEWMP DATABASE MIGRATION SUMMARY                    ");
        println!("================================================================================");
        println!("Source Database    : {}", LogObscurer::path(&self.source_path));
        println!(
            "Source Size        : {:.2} MB",
            self.source_size_bytes as f64 / (1024.0 * 1024.0)
        );
        println!(
            "XnView Schema Ver. : {}",
            if self.source_version.is_empty() {
                "Unknown".to_string()
            } else {
                self.source_version.clone()
            }
        );
        println!(
            "Base/Root Filter   : {}",
            match self.root_filter_path {
                Some(ref r) => format!("Active (only paths at or under '{}')", LogObscurer::path(r)),
                None => "Disabled (all paths imported)".to_string(),
            }
        );
        println!(
            "Log Privacy Mode   : {}",
            if LogObscurer::is_active() {
                "Enabled (Obfuscated IDs/Hashes)"
            } else {
                "Disabled (Plain text audit trail)"
            }
        );
        println!(
            "Missing Item Check : {}",
            if self.skip_missing_enabled {
                "Enabled (Skipped non-existent folders & files)"
            } else {
                "Disabled (Imported all records)"
            }
        );
        println!("ID Re-indexing     : Contiguous 1-based sequential mapping");
        println!("--------------------------------------------------------------------------------");
        println!("EXTRACTION & FILTERING RESULTS:");
        println!(
            "  • Folders                   : {:>7} extracted | {:>6} skipped | {:>7} migrated (IDs 1..={})",
            self.extracted_folders,
            self.skipped_folders_outside_root + self.skipped_folders_missing,
            self.total_folders,
            self.total_folders
        );
        if self.skipped_folders_outside_root > 0 {
            println!(
                "    └─ Outside Root Skipped   : {:>7}",
                self.skipped_folders_outside_root
            );
        }
        if self.skipped_folders_missing > 0 {
            println!("    └─ Missing Folder Skipped : {:>7}", self.skipped_folders_missing);
        }
        println!(
            "  • XnView Images Entries     : {:>7} extracted | {:>6} skipped | {:>7} migrated (IDs 1..={})",
            self.extracted_media,
            self.skipped_media_outside_root
                + self.skipped_subfolders_in_images
                + self.skipped_unsupported_media
                + self.skipped_media_missing_folder
                + self.skipped_media_missing_file,
            self.total_media,
            self.total_media
        );
        if self.skipped_media_outside_root > 0 {
            println!("    ├─ Outside Root Skipped   : {:>7}", self.skipped_media_outside_root);
        }
        println!(
            "    ├─ Subfolders in Images   : {:>7} (directory entries from XnView browser)",
            self.skipped_subfolders_in_images
        );
        println!(
            "    ├─ Unsupported Formats    : {:>7} (non-media files: .txt, .py, etc.)",
            self.skipped_unsupported_media
        );
        if self.skip_missing_enabled {
            if self.skipped_media_missing_folder > 0 {
                println!(
                    "    ├─ Missing Folder Skipped : {:>7}",
                    self.skipped_media_missing_folder
                );
            }
            if self.skipped_media_missing_file > 0 {
                println!("    └─ Missing File Skipped   : {:>7}", self.skipped_media_missing_file);
            }
        }
        println!(
            "  • Tags / Categories         : {:>7} extracted | {:>6} skipped | {:>7} migrated (IDs 1..={})",
            self.extracted_tags,
            self.extracted_tags - self.total_tags,
            self.total_tags,
            self.total_tags
        );
        println!(
            "  • Tag Associations          : {:>7} extracted | {:>6} skipped | {:>7} migrated",
            self.extracted_tag_assignments, self.skipped_tag_assignments_orphaned, self.total_tag_assignments
        );
        println!(
            "  • Dedicated External Hashes : {:>7} extracted | {:>6} skipped | {:>7} migrated{}",
            self.extracted_external_hashes,
            self.extracted_external_hashes - self.total_external_hashes,
            self.total_external_hashes,
            if self.hashes_table_found {
                ""
            } else {
                " (table not present)"
            }
        );
        println!("--------------------------------------------------------------------------------");
        println!("CHECKSUM & HASH RESOLUTION:");
        println!("  • Resolved from Hashes Table: {:>8}", self.resolved_hashes_from_table);
        println!(
            "  • Computed Live from Files  : {:>8}",
            self.resolved_hashes_computed_live
        );
        println!("  • Seeded Fallback Checksums : {:>8}", self.fallback_hashes_generated);
        println!("--------------------------------------------------------------------------------");
        println!("PERFORMANCE & EXECUTION METRICS:");
        println!("  • Extraction Phase (SQLite) : {:>7} ms", self.read_time_ms);
        println!("  • Mapping, Probing & Filter : {:>7} ms", self.map_time_ms);
        println!("  • Wipe & LMDB Commit        : {:>7} ms", self.write_time_ms);
        println!("  • Total Migration Time      : {:>7} ms", self.total_time_ms);
        println!("--------------------------------------------------------------------------------");

        if !self.warnings.is_empty() {
            println!("WARNINGS / DISCREPANCIES ({} total):", self.warnings.len());
            for (idx, w) in self.warnings.iter().take(5).enumerate() {
                println!("  [{}] {}", idx + 1, w);
            }
            if self.warnings.len() > 5 {
                println!("  ... and {} more warnings.", self.warnings.len() - 5);
            }
            println!("--------------------------------------------------------------------------------");
        }

        println!("STATUS: Migration completed successfully. Clean & coherent LMDB storage ready for RustView.");
        println!("================================================================================");
        println!();
    }
}
