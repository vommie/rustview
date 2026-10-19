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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlink_loops_end_and_links_to_files_are_found_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.jpg"), b"x").unwrap();
        fs::write(root.join("sub/b.jpg"), b"x").unwrap();
        symlink(&root, root.join("sub/loop")).unwrap();
        symlink(root.join("a.jpg"), root.join("linked.jpg")).unwrap();

        let shallow: Vec<String> = DirectoryScanner::scan(&root, false).into_iter().map(|f| f.filename).collect();
        let mut shallow_sorted = shallow.clone();
        shallow_sorted.sort();
        assert_eq!(shallow_sorted, ["a.jpg", "linked.jpg"], "a folder is scanned alone, links to folders are not entered");

        // The recursive walk meets the loop, reports it as an error entry and goes on; it must end.
        let deep = DirectoryScanner::scan(&root, true);
        assert!(deep.iter().any(|f| f.filename == "b.jpg"));
        assert!(deep.len() < 100, "the loop must not multiply the files ({} found)", deep.len());
    }
}
