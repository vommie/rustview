use super::detector::MediaDetector;
use super::format_catalog::KNOWN_UNSUPPORTED_EXTENSIONS;
use super::format_policy::{FormatMask, FormatPolicy};
use super::formats::MediaFormat;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub filename: String,
    pub format: MediaFormat,
}

/// The files a scan walked past, grouped by why they were left out. Nothing here is indexed, hidden or removed: the
/// counts only tell the user why a file they can see in a file manager is not in RustView.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IgnoredFiles {
    /// Files of a format the user switched off in Settings > File Formats.
    pub disabled: BTreeMap<&'static str, u32>,
    /// Files whose extension no format uses (lower-case; empty for none).
    pub unsupported: HashMap<String, u32>,
    /// Files with an ambiguous extension (`.ts`) whose content is not that format (a TypeScript source).
    pub header_mismatch: u32,
}

impl IgnoredFiles {
    pub fn is_empty(&self) -> bool {
        self.disabled.is_empty() && self.unsupported.is_empty() && self.header_mismatch == 0
    }

    pub fn disabled_total(&self) -> u32 {
        self.disabled.values().sum()
    }

    /// The `limit` most frequent unsupported extensions, most frequent first (ties by name); files without an extension
    /// are not listed.
    pub fn top_unsupported(&self, limit: usize) -> Vec<(&str, u32)> {
        let mut list: Vec<(&str, u32)> = self
            .unsupported
            .iter()
            .filter(|(ext, _)| !ext.is_empty())
            .map(|(ext, n)| (ext.as_str(), *n))
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        list.truncate(limit);
        list
    }

    /// One sentence for the status line after a refresh, or `None` when nothing worth saying was left out: files of
    /// switched-off formats, media extensions RustView cannot handle (phone photos, camera RAW), and files whose
    /// extension is shared with something else (TypeScript `.ts`). Unknown extensions such as `.txt` are not mentioned.
    pub fn describe(&self) -> Option<String> {
        let mut parts = Vec::new();
        let unsupported: Vec<(&str, u32)> = {
            let mut known: Vec<(&str, u32)> = self
                .unsupported
                .iter()
                .filter(|(ext, _)| KNOWN_UNSUPPORTED_EXTENSIONS.contains(&ext.as_str()))
                .map(|(ext, n)| (ext.as_str(), *n))
                .collect();
            known.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            known
        };
        if !unsupported.is_empty() {
            let total: u32 = unsupported.iter().map(|(_, n)| n).sum();
            let listed: Vec<String> = unsupported.iter().take(3).map(|(ext, n)| format!("{n} .{ext}")).collect();
            let more = if unsupported.len() > 3 { ", ..." } else { "" };
            parts.push(format!("{total} not supported ({}{more})", listed.join(", ")));
        }
        let disabled = self.disabled_total();
        if disabled > 0 {
            let mut names: Vec<(&&str, &u32)> = self.disabled.iter().collect();
            names.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let listed: Vec<String> = names.iter().take(3).map(|(id, n)| format!("{n} {}", id.to_uppercase())).collect();
            let more = if names.len() > 3 { ", ..." } else { "" };
            parts.push(format!("{disabled} of switched-off formats ({}{more})", listed.join(", ")));
        }
        if self.header_mismatch > 0 {
            parts.push(format!("{} whose content does not match their extension", self.header_mismatch));
        }
        (!parts.is_empty()).then(|| format!("Skipped: {}.", parts.join("; ")))
    }

    pub fn merge(&mut self, other: IgnoredFiles) {
        for (id, n) in other.disabled {
            *self.disabled.entry(id).or_default() += n;
        }
        for (ext, n) in other.unsupported {
            *self.unsupported.entry(ext).or_default() += n;
        }
        self.header_mismatch += other.header_mismatch;
    }

    fn note_unsupported(&mut self, path: &Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // Counted by the lower-case extension; a name that is already lower case (nearly all) costs no allocation.
        let key = if ext.bytes().any(|b| b.is_ascii_uppercase()) {
            std::borrow::Cow::Owned(ext.to_ascii_lowercase())
        } else {
            std::borrow::Cow::Borrowed(ext)
        };
        match self.unsupported.get_mut(key.as_ref()) {
            Some(n) => *n += 1,
            None => {
                self.unsupported.insert(key.into_owned(), 1);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub files: Vec<ScannedFile>,
    pub ignored: IgnoredFiles,
}

pub struct DirectoryScanner;

impl DirectoryScanner {
    /// Scans a directory shallowly (or recursively if specified) collecting all media files of the enabled formats.
    pub fn scan(dir: &Path, recursive: bool) -> Vec<ScannedFile> {
        Self::scan_with(dir, recursive, FormatPolicy::mask()).files
    }

    /// Like `scan`, with an explicit policy snapshot, and it reports what was left out.
    pub fn scan_with(dir: &Path, recursive: bool, mask: FormatMask) -> ScanResult {
        let max_depth = if recursive { usize::MAX } else { 1 };
        let mut result = ScanResult::default();

        for entry in WalkDir::new(dir)
            .min_depth(1)
            .max_depth(max_depth)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let format = MediaFormat::from_path(path);
            if !format.is_supported() {
                result.ignored.note_unsupported(path);
                continue;
            }
            if !mask.is_enabled(format) {
                *result.ignored.disabled.entry(format.spec().id).or_default() += 1;
                continue;
            }
            if !Self::header_fits(format, path) {
                result.ignored.header_mismatch += 1;
                continue;
            }
            if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                result.files.push(ScannedFile {
                    path: path.to_path_buf(),
                    filename: filename.to_string(),
                    format,
                });
            }
        }

        result
    }

    /// Formats that share their extension with unrelated files are only taken when the header matches.
    pub fn header_fits(format: MediaFormat, path: &Path) -> bool {
        !format.spec().needs_header_check
            || MediaDetector::read_header(path).is_some_and(|header| MediaDetector::confirms(format, &header))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_worth_saying_gives_no_message() {
        let mut ignored = IgnoredFiles::default();
        ignored.unsupported.insert("txt".into(), 40);
        ignored.unsupported.insert(String::new(), 3);
        assert_eq!(ignored.describe(), None);
    }

    #[test]
    fn the_message_explains_each_kind_of_skipped_file() {
        let mut ignored = IgnoredFiles::default();
        ignored.unsupported.insert("nef".into(), 4);
        ignored.unsupported.insert("cr2".into(), 7);
        ignored.unsupported.insert("txt".into(), 99);
        ignored.disabled.insert("png", 2);
        ignored.header_mismatch = 1;
        assert_eq!(
            ignored.describe().unwrap(),
            "Skipped: 11 not supported (7 .cr2, 4 .nef); 2 of switched-off formats (2 PNG); 1 whose content does not match their extension."
        );
    }

    #[test]
    fn merging_adds_up() {
        let mut a = IgnoredFiles::default();
        a.disabled.insert("gif", 1);
        a.unsupported.insert("nef".into(), 1);
        let mut b = IgnoredFiles::default();
        b.disabled.insert("gif", 2);
        b.unsupported.insert("nef".into(), 3);
        b.header_mismatch = 2;
        a.merge(b);
        assert_eq!(a.disabled["gif"], 3);
        assert_eq!(a.unsupported["nef"], 4);
        assert_eq!(a.header_mismatch, 2);
    }

    #[test]
    fn the_scanner_skips_by_policy_and_header_and_reports_why() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| fs::write(dir.path().join(name), bytes).unwrap();
        write("a.jpg", b"x");
        write("b.png", b"x");
        write("c.nef", b"x");
        write("notes.txt", b"x");
        write("source.ts", b"export const a = 1;\n");
        write("UPPER.JPG", b"x");
        write("one.NEF", b"x");
        write("two.nef", b"x");
        let mask = FormatMask::ALL_ENABLED.with(MediaFormat::Png, false);
        let result = DirectoryScanner::scan_with(dir.path(), false, mask);
        let mut names: Vec<_> = result.files.iter().map(|f| f.filename.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["UPPER.JPG", "a.jpg"]);
        assert_eq!(result.ignored.disabled["png"], 1);
        assert_eq!(result.ignored.unsupported["nef"], 3, "the case of the extension does not matter");
        assert_eq!(result.ignored.unsupported["txt"], 1);
        assert_eq!(result.ignored.header_mismatch, 1);
    }
}
