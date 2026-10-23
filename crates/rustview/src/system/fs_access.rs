//! Permission checks that ask the operating system what the current user may actually do (ACLs and root included), which the
//! permission bits of `std::fs::Metadata` cannot tell. Used right before a file operation, never for display.

use rustix::fs::{Access, access};
use std::path::Path;

/// Whether files can be created in, and removed from, the directory `path` (write and search permission, directory exists).
pub fn can_write_directory(path: &Path) -> bool {
    path.is_dir() && access(path, Access::WRITE_OK | Access::EXEC_OK).is_ok()
}

#[cfg(test)]
mod tests {
    use super::can_write_directory;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_writable_directory_is_accepted_and_a_missing_one_is_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(can_write_directory(dir.path()));
        assert!(!can_write_directory(&dir.path().join("missing")));
        assert!(
            !can_write_directory(&dir.path().join("..\u{0}")),
            "a path with a NUL byte is never accepted"
        );
    }

    #[test]
    fn a_read_only_directory_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).expect("create dir");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).expect("chmod");
        // root ignores permission bits; the check then rightly says "writable".
        let is_root = rustix::process::geteuid().is_root();
        assert_eq!(can_write_directory(&locked), is_root);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).expect("chmod back");
    }
}
