//! Linux mount table access.
//!
//! Reads the kernel mount table of this process (`/proc/self/mounts`, parsed by the `proc-mounts` crate, which decodes the kernel's octal
//! escapes such as `\040`) and answers which mount points are nested below a given directory. RustView only targets Linux.

use crate::settings::LogObscurer;
use proc_mounts::MountIter;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const PROC_MOUNTS_PATH: &str = "/proc/self/mounts";

const MAX_LOGGED_MOUNTS: usize = 10;

pub struct SystemMounts;

impl SystemMounts {
    /// Reads all mount points of the current mount namespace (a malformed line is skipped).
    pub fn read_mount_points() -> std::io::Result<Vec<PathBuf>> {
        Ok(MountIter::new_from_file(PROC_MOUNTS_PATH)?
            .filter_map(Result::ok)
            .map(|mount| mount.dest)
            .collect())
    }

    /// Returns all mount points located strictly below `root` (at any depth). The root itself is never included.
    pub fn nested_mount_points(root: &Path) -> std::io::Result<HashSet<PathBuf>> {
        let all = Self::read_mount_points()?;
        let total_mounts = all.len();
        let nested = Self::filter_nested(root, all);

        tracing::info!(
            root = %LogObscurer::path(root),
            total_mounts,
            nested_mounts = nested.len(),
            "[MountFilter: Detected] Read system mount table and resolved mount points below the scan root"
        );
        for mount in nested.iter().take(MAX_LOGGED_MOUNTS) {
            tracing::debug!(
                mount = %LogObscurer::path(mount),
                "[MountFilter: Detected] Nested mount point below scan root"
            );
        }

        Ok(nested)
    }

    /// Keeps only mount points strictly below `root`.
    pub(crate) fn filter_nested(root: &Path, mounts: Vec<PathBuf>) -> HashSet<PathBuf> {
        mounts
            .into_iter()
            .filter(|m| m.as_path() != root && m.starts_with(root))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(table: &str) -> Vec<PathBuf> {
        MountIter::new_from_reader(Cursor::new(table.as_bytes().to_vec()))
            .filter_map(Result::ok)
            .map(|mount| mount.dest)
            .collect()
    }

    #[test]
    fn decodes_the_escaped_space_of_a_mount_point() {
        assert_eq!(
            parse("//nas/share /mnt/my\\040share cifs rw,relatime 0 0\n"),
            vec![PathBuf::from("/mnt/my share")]
        );
    }

    #[test]
    fn skips_malformed_lines() {
        assert!(parse("too short\n\n").is_empty());
    }

    #[test]
    fn keeps_only_mounts_strictly_below_root() {
        let root = Path::new("/home/u");
        let nested = SystemMounts::filter_nested(
            root,
            vec![
                PathBuf::from("/"),
                PathBuf::from("/home"),
                PathBuf::from("/home/u"),
                PathBuf::from("/home/u/mounts/nas"),
                PathBuf::from("/home/user2"),
            ],
        );
        assert_eq!(nested.len(), 1);
        assert!(nested.contains(Path::new("/home/u/mounts/nas")));
    }
}
