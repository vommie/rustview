use super::formats::MediaFormat;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub filename: String,
    pub format: MediaFormat,
}

pub struct DirectoryScanner;

impl DirectoryScanner {
    /// Scans a directory shallowly (or recursively if specified) collecting all supported media files.
    pub fn scan(dir: &Path, recursive: bool) -> Vec<ScannedFile> {
        let max_depth = if recursive { usize::MAX } else { 1 };
        let mut results = Vec::new();

        for entry in WalkDir::new(dir)
            .min_depth(1)
            .max_depth(max_depth)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let path = entry.path().to_path_buf();
                let format = MediaFormat::from_path(&path);

                if format.is_supported()
                    && let Some(filename) = path
                        .file_name()
                        .and_then(|f| f.to_str())
                        .map(|s| s.to_string())
                {
                    results.push(ScannedFile {
                        path,
                        filename,
                        format,
                    });
                }
            }
        }

        results
    }
}
