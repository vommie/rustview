//! Permission checks that ask the operating system what the current user may actually do (ACLs and root included), which the
//! permission bits of `std::fs::Metadata` cannot tell. Used right before a file operation, never for display.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const X_OK: c_int = 1;
const W_OK: c_int = 2;

unsafe extern "C" {
    fn access(path: *const c_char, mode: c_int) -> c_int;
}

fn allowed(path: &Path, mode: c_int) -> bool {
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the call; `access` only reads it.
    unsafe { access(c_path.as_ptr(), mode) == 0 }
}

/// Whether files can be created in, and removed from, the directory `path` (write and search permission, directory exists).
pub fn can_write_directory(path: &Path) -> bool {
    path.is_dir() && allowed(path, W_OK | X_OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn running_as_root() -> bool {
        fs::metadata("/proc/self").map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0).unwrap_or(false)
    }

    #[test]
    fn a_normal_directory_is_writable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_write_directory(dir.path()));
    }

    #[test]
    fn a_read_only_directory_is_not_writable_for_a_normal_user() {
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let result = can_write_directory(&locked);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!result || running_as_root(), "root may write anywhere, everybody else may not");
    }

    #[test]
    fn missing_paths_and_files_are_not_writable_directories() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!can_write_directory(&dir.path().join("nope")));
        let file = dir.path().join("f");
        fs::write(&file, b"x").unwrap();
        assert!(!can_write_directory(&file));
    }

    #[test]
    fn a_path_with_a_nul_byte_is_refused() {
        assert!(!can_write_directory(Path::new("a\0b")));
    }
}
