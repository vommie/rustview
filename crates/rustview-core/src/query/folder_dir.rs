//! The directory of a folder's records, shared by all of them.
//!
//! A record used to carry its own copies of the directory (as a path, as a Slint string and in lower case), which is three heap
//! allocations per record for one value that is the same for the whole folder. Records now hold an `Arc<FolderDir>`.

use slint::SharedString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct FolderDir {
    path: PathBuf,
    path_str: SharedString,
    path_lower: String,
}

impl FolderDir {
    pub fn new(path: impl Into<PathBuf>) -> Arc<Self> {
        let path = path.into();
        let (path_str, path_lower) = {
            let text = path.to_string_lossy();
            (SharedString::from(&*text), text.to_lowercase())
        };
        Arc::new(Self { path, path_str, path_lower })
    }

    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The directory as shown in the UI.
    #[inline]
    pub fn path_str(&self) -> &SharedString {
        &self.path_str
    }

    /// The directory in lower case, the key of the "Path" sort order.
    #[inline]
    pub fn path_lower(&self) -> &str {
        &self.path_lower
    }
}
