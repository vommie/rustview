//! Linux mount table access.
//!
//! Reads the kernel mount table from `/proc/self/mountinfo` (fallback: `/proc/mounts`) and answers which mount
//! points are nested below a given directory. RustView only targets Linux, so no other platforms are handled.

use crate::settings::LogObscurer;
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

const PROC_MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const PROC_MOUNTS_PATH: &str = "/proc/mounts";

/// Field index of the mount point in a `/proc/self/mountinfo` line.
const MOUNTINFO_MOUNT_POINT_FIELD: usize = 4;
/// Field index of the mount point in a `/proc/mounts` line.
const PROC_MOUNTS_MOUNT_POINT_FIELD: usize = 1;

const MAX_LOGGED_MOUNTS: usize = 10;

pub struct SystemMounts;

impl SystemMounts {
    /// Reads all mount points of the current mount namespace.
    pub fn read_mount_points() -> std::io::Result<Vec<PathBuf>> {
        match std::fs::read(PROC_MOUNTINFO_PATH) {
            Ok(bytes) => Ok(Self::parse_mountinfo(&bytes)),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "[MountFilter: Fallback] Could not read /proc/self/mountinfo, falling back to /proc/mounts"
                );
                let bytes = std::fs::read(PROC_MOUNTS_PATH)?;
                Ok(Self::parse_proc_mounts(&bytes))
            }
        }
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

    pub(crate) fn parse_mountinfo(content: &[u8]) -> Vec<PathBuf> {
        Self::parse_mount_field(content, MOUNTINFO_MOUNT_POINT_FIELD)
    }

    pub(crate) fn parse_proc_mounts(content: &[u8]) -> Vec<PathBuf> {
        Self::parse_mount_field(content, PROC_MOUNTS_MOUNT_POINT_FIELD)
    }

    /// Extracts and unescapes the whitespace-separated field at `field_index` from every line.
    fn parse_mount_field(content: &[u8], field_index: usize) -> Vec<PathBuf> {
        content
            .split(|b| *b == b'\n')
            .filter_map(|line| {
                line.split(|b| *b == b' ')
                    .filter(|field| !field.is_empty())
                    .nth(field_index)
                    .map(Self::unescape_mount_path)
            })
            .collect()
    }

    /// Decodes kernel octal escapes such as `\040` (space), `\011` (tab), `\012` (newline) and `\134` (backslash).
    fn unescape_mount_path(field: &[u8]) -> PathBuf {
        let mut out = Vec::with_capacity(field.len());
        let mut i = 0usize;

        while i < field.len() {
            if field[i] == b'\\'
                && i + 3 < field.len()
                && field[i + 1..=i + 3].iter().all(|d| (b'0'..=b'7').contains(d))
            {
                let value = (u32::from(field[i + 1] - b'0') << 6)
                    | (u32::from(field[i + 2] - b'0') << 3)
                    | u32::from(field[i + 3] - b'0');
                if let Ok(byte) = u8::try_from(value) {
                    out.push(byte);
                    i += 4;
                    continue;
                }
            }
            out.push(field[i]);
            i += 1;
        }

        PathBuf::from(OsString::from_vec(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mountinfo_mount_point_with_escaped_space() {
        let line = b"36 35 98:0 /mnt1 /mnt/my\\040share rw,noatime master:1 - ext3 /dev/root rw\n";
        let mounts = SystemMounts::parse_mountinfo(line);
        assert_eq!(mounts, vec![PathBuf::from("/mnt/my share")]);
    }

    #[test]
    fn parses_proc_mounts_fallback_format() {
        let line = b"//nas/share /home/u/mounts/nas cifs rw,relatime 0 0\n";
        let mounts = SystemMounts::parse_proc_mounts(line);
        assert_eq!(mounts, vec![PathBuf::from("/home/u/mounts/nas")]);
    }

    #[test]
    fn skips_malformed_lines() {
        let mounts = SystemMounts::parse_mountinfo(b"too short\n\n");
        assert!(mounts.is_empty());
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
