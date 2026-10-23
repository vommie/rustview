//! Pure filesystem walker used by the manual folder tree scans.
//!
//! This module only touches the filesystem and never the database. It is invoked exclusively from
//! explicitly user-triggered refresh actions (Refresh Tree Full / Relative), never automatically.
//! Directories that are mount points listed in `ignored_mounts` are not entered and not reported as folders.

use crate::media::{DirectoryScanner, FormatMask, FormatPolicy, MediaFormat};
use crate::settings::LogObscurer;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Shared progress callback receiving the number of directories visited so far.
pub type ScanProgressCallback = Arc<dyn Fn(usize) + Send + Sync>;

/// One directory discovered on disk, in pre-order (a parent always precedes its children).
pub struct ScannedDirItem {
    pub path: PathBuf,
    pub name: String,
    pub parent_path: Option<PathBuf>,
    /// Depth relative to the scan root (the scan root itself has depth 0).
    pub depth: u32,
    pub is_hidden: bool,
    pub media_files_count: u32,
}

/// Result of a filesystem walk.
pub struct WalkOutcome {
    /// All discovered directories in pre-order. The first element is always the scan root.
    pub items: Vec<ScannedDirItem>,
    /// Mount points that were skipped because they are on the ignore list.
    pub skipped_mounts: Vec<PathBuf>,
}

/// Mutable state shared across the recursive walk.
struct WalkContext<'a> {
    counter: AtomicUsize,
    progress: Option<&'a ScanProgressCallback>,
    ignored_mounts: &'a HashSet<PathBuf>,
    items: Vec<ScannedDirItem>,
    skipped_mounts: Vec<PathBuf>,
    /// One policy snapshot for the whole walk: only files of these formats are counted.
    formats: FormatMask,
}

/// Recursively walks `root` and returns all directories in pre-order, skipping ignored mount points.
#[allow(
    clippy::implicit_hasher,
    reason = "the mount set is always built with the default hasher"
)]
pub fn walk_directory_tree(
    root: &Path,
    progress: Option<&ScanProgressCallback>,
    ignored_mounts: &HashSet<PathBuf>,
) -> WalkOutcome {
    let mut ctx = WalkContext {
        counter: AtomicUsize::new(1),
        progress,
        ignored_mounts,
        items: Vec::with_capacity(1024),
        skipped_mounts: Vec::new(),
        formats: FormatPolicy::mask(),
    };

    let root_hidden = root.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));

    walk_dir(&mut ctx, root, None, 0, root_hidden);

    if let Some(cb) = ctx.progress {
        cb(ctx.counter.load(Ordering::Relaxed));
    }

    WalkOutcome {
        items: ctx.items,
        skipped_mounts: ctx.skipped_mounts,
    }
}

fn walk_dir(ctx: &mut WalkContext<'_>, dir: &Path, parent: Option<&Path>, depth: u32, is_parent_hidden: bool) {
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let starts_with_dot = dir_name.starts_with('.');
    let hidden_set = read_dot_hidden_entries(dir);

    let mut sub_dirs = Vec::new();
    let mut media_count = 0u32;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let file_type = entry.file_type();
            let is_dir = file_type.as_ref().is_ok_and(std::fs::FileType::is_dir);
            let is_file = file_type.as_ref().is_ok_and(std::fs::FileType::is_file);
            let entry_name = entry.file_name().to_string_lossy().to_string();

            if is_dir {
                let child_path = entry.path();

                if ctx.ignored_mounts.contains(&child_path) {
                    tracing::debug!(
                        mount = %LogObscurer::path(&child_path),
                        "[MountFilter: Skip] Skipping mount point inside scan root"
                    );
                    ctx.skipped_mounts.push(child_path);
                    continue;
                }

                let child_hidden = starts_with_dot
                    || is_parent_hidden
                    || entry_name.starts_with('.')
                    || hidden_set.contains(&entry_name);
                sub_dirs.push((child_path, child_hidden));
            } else if is_file {
                let format = MediaFormat::from_path(Path::new(&entry_name));
                if ctx.formats.is_enabled(format) && DirectoryScanner::header_fits(format, &entry.path()) {
                    media_count += 1;
                }
            }
        }
    } else {
        tracing::warn!(
            dir = %LogObscurer::path(dir),
            "[FolderScan: Warning] Unable to read directory entries from filesystem"
        );
    }

    ctx.items.push(ScannedDirItem {
        path: dir.to_path_buf(),
        name: dir_name,
        parent_path: parent.map(std::path::Path::to_path_buf),
        depth,
        is_hidden: is_parent_hidden || starts_with_dot,
        media_files_count: media_count,
    });

    let cur = ctx.counter.fetch_add(1, Ordering::Relaxed) + 1;
    if let Some(callback) = ctx.progress
        && (cur.is_multiple_of(32) || cur == 1)
    {
        callback(cur);
    }

    for (sub_p, sub_hidden) in sub_dirs {
        walk_dir(ctx, &sub_p, Some(dir), depth + 1, sub_hidden);
    }
}

/// Parses the optional local `.hidden` file (one entry name per line, `#` comments allowed).
pub(crate) fn read_dot_hidden_entries(dir: &Path) -> HashSet<String> {
    let mut hidden_set = HashSet::new();
    let dot_hidden_path = dir.join(".hidden");

    if dot_hidden_path.is_file()
        && let Ok(content) = std::fs::read_to_string(&dot_hidden_path)
    {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                hidden_set.insert(trimmed.to_string());
            }
        }
    }

    hidden_set
}
