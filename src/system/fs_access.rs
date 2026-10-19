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
