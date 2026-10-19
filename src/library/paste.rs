//! Pasting files from the clipboard into a folder (Ctrl+V and the "Paste" menu entries).
//!
//! The clipboard names paths, and the paths can come from anywhere: a file RustView lists, a file of a folder it never scanned, a file
//! another program put there that is gone again by now. `PastePlanner` turns that list into a plan (what can be pasted, what is skipped
//! and why); `PasteExecutor` carries out one item at a time with the operations the drag and drop transfers use, so conflicts, records,
//! tags, ratings and thumbnails behave exactly as there:
//!
//! * a file the library knows (a record in its folder): `move_single_media` / `copy_single_media` (record, tags, rating and thumbnail
//!   follow the file);
//! * any other media file: plain file system copy or move into the folder; the files become records when the folder is scanned afterwards
//!   (`PasteOutcome::needs_scan`);
//! * a listed file that is gone from the drive (moved away by another program): nothing is transferred and its record is flagged missing,
//!   the same flag a refresh sets. Records are never removed without the user's say.
//!
//! The planner and the executor touch the drive, which is allowed here because pasting is a file operation the user started.

use super::error::LibraryError;
use super::file_operations::{is_same_file_path, robust_move_file, FileOperations, FilenameValidator};
use super::models::{FileConflictItemDetails, FileConflictReport, FileTransferKind, FolderId, MediaId};
use super::Library;
use crate::media::{MediaFormat, MediaHasher, MetadataExtractor};
use crate::settings::LogObscurer;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Why a path of the clipboard is not pasted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The file is not there any more (moved, renamed or deleted by another program).
    SourceMissing,
    /// The file or its folder cannot be read (permissions, I/O error).
    Unreadable(String),
    /// A folder; only media files are pasted.
    IsFolder,
    /// A socket, device, pipe or similar.
    NotAFile,
    /// A file that is not an image or video RustView knows.
    Unsupported,
    /// The name is not usable as a file name here.
    InvalidName,
    /// A cut file that lies in the target folder already.
    AlreadyInFolder,
    /// The same path is on the clipboard twice.
    Duplicate,
}

impl SkipReason {
    pub fn describe(&self) -> String {
        match self {
            Self::SourceMissing => "no longer exists".to_string(),
            Self::Unreadable(why) => format!("cannot be read ({why})"),
            Self::IsFolder => "is a folder".to_string(),
            Self::NotAFile => "is not a regular file".to_string(),
            Self::Unsupported => "is not a supported image or video".to_string(),
            Self::InvalidName => "has a name that cannot be used".to_string(),
            Self::AlreadyInFolder => "is in this folder already".to_string(),
            Self::Duplicate => "is on the clipboard twice".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteSkip {
    pub path: PathBuf,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteItem {
    pub path: PathBuf,
    pub name: String,
    /// The record of the file when the library lists it.
    pub known: Option<MediaId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PastePlan {
    pub items: Vec<PasteItem>,
    pub skipped: Vec<PasteSkip>,
}

impl PastePlan {
    pub fn skipped_because(&self, reason: &SkipReason) -> usize {
        self.skipped.iter().filter(|s| &s.reason == reason).count()
    }
}

/// How a name collision in the target folder is settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteChoice {
    /// No collision: transfer under the file's own name.
    Direct,
    Rename(String),
    Overwrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteOutcome {
    pub final_name: String,
    /// The file is not in the library yet: the target folder must be scanned to list it.
    pub needs_scan: bool,
}

pub struct PastePlanner<'a> {
    library: &'a Library,
}

/// What the library knows about one folder, looked up once for all clipboard files in it.
struct FolderNames {
    names: HashMap<String, MediaId>,
}

impl<'a> PastePlanner<'a> {
    pub fn new(library: &'a Library) -> Self {
        Self { library }
    }

    pub fn plan(&self, files: &[PathBuf], target_folder_id: FolderId, kind: FileTransferKind) -> Result<PastePlan, LibraryError> {
        let target = self.library.folders().get_folder(target_folder_id)?.ok_or(LibraryError::FolderNotFound(target_folder_id))?;
        let target_path = PathBuf::from(&target.path);
        let mut plan = PastePlan::default();
        let mut seen = HashSet::new();
        let mut folders: HashMap<PathBuf, Option<FolderNames>> = HashMap::new();

        for path in files {
            if !seen.insert(path.clone()) {
                plan.skipped.push(PasteSkip { path: path.clone(), reason: SkipReason::Duplicate });
                continue;
            }
            match self.classify(path, &target_path, kind) {
                Err(reason) => plan.skipped.push(PasteSkip { path: path.clone(), reason }),
                Ok(name) => {
                    let known = self.known_record(path, &name, &mut folders);
                    plan.items.push(PasteItem { path: path.clone(), name, known });
                }
            }
        }
        tracing::info!(
            target_folder = target_folder_id,
            requested = files.len(),
            items = plan.items.len(),
            skipped = plan.skipped.len(),
            kind = ?kind,
            "[Paste: Plan] Clipboard files planned"
        );
        Ok(plan)
    }

    /// The usable name of a pasteable file, or why it is not.
    fn classify(&self, path: &Path, target: &Path, kind: FileTransferKind) -> Result<String, SkipReason> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => SkipReason::SourceMissing,
            _ => SkipReason::Unreadable(e.to_string()),
        })?;
        // A link counts as what it points to (a link to a file is a file, a dangling link is gone).
        let meta = if meta.file_type().is_symlink() {
            std::fs::metadata(path).map_err(|e| match e.kind() {
                ErrorKind::NotFound => SkipReason::SourceMissing,
                _ => SkipReason::Unreadable(e.to_string()),
            })?
        } else {
            meta
        };
        if meta.is_dir() {
            return Err(SkipReason::IsFolder);
        }
        if !meta.is_file() {
            return Err(SkipReason::NotAFile);
        }
        let name = path.file_name().and_then(|n| n.to_str()).ok_or(SkipReason::InvalidName)?;
        let name = FilenameValidator::validate(name).map_err(|_| SkipReason::InvalidName)?;
        if !MediaFormat::from_path(path).is_supported() {
            return Err(SkipReason::Unsupported);
        }
        let in_target = path.parent().is_some_and(|parent| parent == target || is_same_file_path(parent, target));
        if kind == FileTransferKind::Move && in_target {
            return Err(SkipReason::AlreadyInFolder);
        }
        Ok(name)
    }

    /// The record of `path` when its folder is in the library and lists the file.
    fn known_record(&self, path: &Path, name: &str, folders: &mut HashMap<PathBuf, Option<FolderNames>>) -> Option<MediaId> {
        let parent = path.parent()?.to_path_buf();
        let names = folders.entry(parent.clone()).or_insert_with(|| {
            let folder = self.library.folders().get_folder_by_path(&parent).ok().flatten()?;
            let records = self.library.media().get_all_by_folder(folder.id).ok()?;
            Some(FolderNames { names: records.into_iter().map(|r| (r.filename, r.id)).collect() })
        });
        names.as_ref()?.names.get(name).copied()
    }
}

pub struct PasteExecutor<'a> {
    library: &'a Library,
}

impl<'a> PasteExecutor<'a> {
    pub fn new(library: &'a Library) -> Self {
        Self { library }
    }

    fn operations(&self) -> FileOperations<'_> {
        self.library.file_operations()
    }

    /// Whether `item` collides with a file or record of the target folder, with what the conflict dialog shows.
    pub fn conflict(&self, item: &PasteItem, target_folder_id: FolderId, kind: FileTransferKind) -> Result<Option<FileConflictReport>, LibraryError> {
        if let Some(media_id) = item.known {
            return self.operations().check_transfer_conflict(media_id, target_folder_id, kind);
        }
        let target = self.library.folders().get_folder(target_folder_id)?.ok_or(LibraryError::FolderNotFound(target_folder_id))?;
        let destination = Path::new(&target.path).join(&item.name);
        let existing_id = self
            .library
            .media()
            .get_all_by_folder(target_folder_id)?
            .into_iter()
            .find(|r| r.filename.eq_ignore_ascii_case(&item.name))
            .map(|r| r.id);
        if existing_id.is_none() && std::fs::symlink_metadata(&destination).is_err() {
            return Ok(None);
        }
        let source = external_details(&item.path);
        let destination_details = self.operations().probe_file_details(&destination, target_folder_id, existing_id)?;
        Ok(Some(FileConflictReport { source, destination: destination_details }))
    }

    /// Transfers one item. A missing source is reported as `NotFound` (and its record flagged) before anything else happens.
    pub fn transfer(
        &self,
        item: &PasteItem,
        target_folder_id: FolderId,
        kind: FileTransferKind,
        choice: &PasteChoice,
    ) -> Result<PasteOutcome, LibraryError> {
        if std::fs::symlink_metadata(&item.path).is_err() {
            if let Some(media_id) = item.known {
                self.flag_missing(media_id);
            }
            return Err(LibraryError::Io(std::io::Error::new(ErrorKind::NotFound, "the file no longer exists")));
        }
        let (custom_name, overwrite) = match choice {
            PasteChoice::Direct => (None, false),
            PasteChoice::Rename(name) => (Some(name.as_str()), false),
            PasteChoice::Overwrite => (None, true),
        };
        if let Some(media_id) = item.known {
            let record = match kind {
                FileTransferKind::Move => self.operations().move_single_media(media_id, target_folder_id, custom_name, overwrite)?,
                FileTransferKind::Copy => self.operations().copy_single_media(media_id, target_folder_id, custom_name, overwrite)?,
            };
            return Ok(PasteOutcome { final_name: record.filename, needs_scan: false });
        }

        let target = self.library.folders().get_folder(target_folder_id)?.ok_or(LibraryError::FolderNotFound(target_folder_id))?;
        let name = FilenameValidator::validate(custom_name.unwrap_or(&item.name))?;
        let destination = Path::new(&target.path).join(&name);
        if is_same_file_path(&item.path, &destination) {
            return Err(LibraryError::FileOperationAlreadyExists(name));
        }
        let exists = std::fs::symlink_metadata(&destination).is_ok();
        if exists && !overwrite {
            return Err(LibraryError::FileOperationAlreadyExists(name));
        }
        match kind {
            FileTransferKind::Copy if overwrite && exists => copy_replacing(&item.path, &destination)?,
            FileTransferKind::Copy => copy_exclusive(&item.path, &destination)?,
            FileTransferKind::Move => robust_move_file(&item.path, &destination)?,
        }
        tracing::info!(
            file = %LogObscurer::filename(&name),
            kind = ?kind,
            "[Paste: Transfer] File outside the library pasted; the folder needs a scan to list it"
        );
        Ok(PasteOutcome { final_name: name, needs_scan: true })
    }

    /// Flags the record as missing on disk (the flag a refresh sets); the record, tags, rating and thumbnail stay.
    pub fn flag_missing(&self, media_id: MediaId) {
        match self.operations().flag_media_missing(media_id) {
            Ok(true) => tracing::info!(media_id, "[Paste: Missing] A listed file was gone from the drive; its record is flagged missing"),
            Ok(false) => {}
            Err(e) => tracing::warn!(media_id, error = %e, "[Paste: Missing] Could not flag the record"),
        }
    }
}

/// What the conflict dialog shows of a file the library does not list.
fn external_details(path: &Path) -> FileConflictItemDetails {
    let meta = std::fs::metadata(path).ok();
    let seconds = |t: Option<std::time::SystemTime>| {
        t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
    };
    let modification_time = seconds(meta.as_ref().and_then(|m| m.modified().ok()));
    let creation_time = meta.as_ref().and_then(|m| m.created().ok()).map(|t| seconds(Some(t))).unwrap_or(modification_time);
    let (width, height, duration_ms) = MetadataExtractor::extract(path).map(|m| (m.width, m.height, m.duration_ms)).unwrap_or((0, 0, 0));
    FileConflictItemDetails {
        media_id: None,
        folder_id: 0,
        folder_path: path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        filename: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        full_path: path.to_path_buf(),
        file_size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        modification_time,
        creation_time,
        duration_ms,
        file_hash: MediaHasher::hash_file(path).unwrap_or_default(),
        width,
        height,
        format: MediaFormat::from_path(path),
        rating: 0,
    }
}

/// Copies to a new file; an existing one (or a link) at `destination` is never touched, a failed copy leaves no partial file.
fn copy_exclusive(source: &Path, destination: &Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(source)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(destination)?;
    let result = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all());
    if let Err(e) = result {
        drop(output);
        let _ = std::fs::remove_file(destination);
        return Err(e);
    }
    if let Ok(meta) = input.metadata() {
        let _ = std::fs::set_permissions(destination, meta.permissions());
    }
    Ok(())
}

/// Replaces `destination` by a copy of `source`: the copy is written beside it first, so a failure keeps the old file and a link at
/// `destination` is replaced instead of written through.
fn copy_replacing(source: &Path, destination: &Path) -> std::io::Result<()> {
    let parent = destination.parent().unwrap_or(Path::new("."));
    let name = destination.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let temp = parent.join(format!(".{name}.rustview-paste-{}", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    copy_exclusive(source, &temp)?;
    if let Err(e) = std::fs::rename(&temp, destination) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::Arc;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        library: Arc<Library>,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path().join("media");
            std::fs::create_dir_all(&root).unwrap();
            let library = Arc::new(Library::open(&dir.path().join("library")).expect("library"));
            Self { _dir: dir, root, library }
        }

        fn folder(&self, name: &str) -> (FolderId, PathBuf) {
            let path = self.root.join(name);
            std::fs::create_dir_all(&path).unwrap();
            let record = self.library.folders().add_folder(&path).expect("add folder");
            (record.id, path)
        }

        fn png(&self, folder: &Path, name: &str, shade: u8) -> PathBuf {
            let path = folder.join(name);
            image::RgbImage::from_pixel(16, 12, image::Rgb([shade, 40, 40])).save(&path).expect("png");
            path
        }

        fn scan(&self, folder_id: FolderId) {
            self.library.refresher().refresh_folder(folder_id).expect("scan");
        }

        fn names_in_db(&self, folder_id: FolderId) -> Vec<String> {
            let mut names: Vec<String> =
                self.library.media().get_all_by_folder(folder_id).unwrap().into_iter().filter(|r| !r.is_deleted).map(|r| r.filename).collect();
            names.sort();
            names
        }

        fn plan(&self, files: &[PathBuf], target: FolderId, kind: FileTransferKind) -> PastePlan {
            PastePlanner::new(&self.library).plan(files, target, kind).expect("plan")
        }

        fn executor(&self) -> PasteExecutor<'_> {
            PasteExecutor::new(&self.library)
        }
    }

    fn is_root() -> bool {
        std::process::Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0").unwrap_or(false)
    }

    #[test]
    fn the_planner_names_a_reason_for_every_file_it_leaves_out() {
        let f = Fixture::new();
        let (src_id, src) = f.folder("src");
        let (dst_id, dst) = f.folder("dst");
        let good = f.png(&src, "good.png", 10);
        let text = src.join("notes.txt");
        std::fs::write(&text, b"x").unwrap();
        let sub = src.join("sub.png");
        std::fs::create_dir(&sub).unwrap();
        let fifo = src.join("pipe.png");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let dangling = src.join("dangling.png");
        symlink(src.join("nowhere.png"), &dangling).unwrap();
        let link = src.join("link.png");
        symlink(&good, &link).unwrap();
        let gone = src.join("gone.png");
        let nested = src.join("deep/er/x.png");
        f.scan(src_id);

        let files = vec![good.clone(), good.clone(), text, sub, fifo, dangling, link.clone(), gone, nested];
        let plan = f.plan(&files, dst_id, FileTransferKind::Copy);

        let reasons: HashMap<String, SkipReason> =
            plan.skipped.iter().map(|s| (s.path.file_name().unwrap().to_string_lossy().to_string(), s.reason.clone())).collect();
        assert_eq!(reasons["notes.txt"], SkipReason::Unsupported);
        assert_eq!(reasons["sub.png"], SkipReason::IsFolder);
        assert_eq!(reasons["pipe.png"], SkipReason::NotAFile);
        assert_eq!(reasons["dangling.png"], SkipReason::SourceMissing);
        assert_eq!(reasons["gone.png"], SkipReason::SourceMissing);
        assert_eq!(reasons["x.png"], SkipReason::SourceMissing);
        assert_eq!(plan.skipped_because(&SkipReason::Duplicate), 1);
        let names: Vec<&str> = plan.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["good.png", "link.png"], "a link to a file is a file");
        assert!(plan.items[0].known.is_some(), "scanned files are known");
        assert!(plan.items[1].known.is_some(), "the scan lists a link to a file as a file");
        assert!(dst.read_dir().unwrap().next().is_none(), "planning changes nothing");
    }

    #[test]
    fn cutting_into_the_own_folder_is_skipped_but_copying_is_not() {
        let f = Fixture::new();
        let (id, dir) = f.folder("one");
        let a = f.png(&dir, "a.png", 1);
        assert_eq!(f.plan(std::slice::from_ref(&a), id, FileTransferKind::Move).skipped[0].reason, SkipReason::AlreadyInFolder);
        assert_eq!(f.plan(std::slice::from_ref(&a), id, FileTransferKind::Copy).items.len(), 1);
        // The same folder spelled differently (`.` segment through a link) is still the same folder.
        let alias = f.root.join("alias");
        symlink(&dir, &alias).unwrap();
        let via_alias = alias.join("a.png");
        assert_eq!(f.plan(&[via_alias], id, FileTransferKind::Move).skipped[0].reason, SkipReason::AlreadyInFolder);
    }

    #[test]
    fn a_known_file_moves_with_its_record() {
        let f = Fixture::new();
        let (src_id, src) = f.folder("src");
        let (dst_id, dst) = f.folder("dst");
        let a = f.png(&src, "a.png", 10);
        f.scan(src_id);
        let plan = f.plan(&[a.clone()], dst_id, FileTransferKind::Move);
        let item = &plan.items[0];
        let media_id = item.known.expect("known");
        assert!(f.executor().conflict(item, dst_id, FileTransferKind::Move).unwrap().is_none());
        let outcome = f.executor().transfer(item, dst_id, FileTransferKind::Move, &PasteChoice::Direct).unwrap();
        assert!(!outcome.needs_scan);
        assert!(!a.exists() && dst.join("a.png").exists());
        assert_eq!(f.library.media().get(media_id).unwrap().unwrap().folder_id, dst_id);
        assert_eq!(f.names_in_db(dst_id), ["a.png"]);
        assert!(f.names_in_db(src_id).is_empty());
    }

    #[test]
    fn a_known_file_is_copied_as_a_second_record() {
        let f = Fixture::new();
        let (src_id, src) = f.folder("src");
        let (dst_id, dst) = f.folder("dst");
        let a = f.png(&src, "a.png", 10);
        f.scan(src_id);
        let plan = f.plan(&[a.clone()], dst_id, FileTransferKind::Copy);
        f.executor().transfer(&plan.items[0], dst_id, FileTransferKind::Copy, &PasteChoice::Direct).unwrap();
        assert!(a.exists() && dst.join("a.png").exists());
        assert_eq!(f.names_in_db(src_id), ["a.png"]);
        assert_eq!(f.names_in_db(dst_id), ["a.png"]);
    }

    #[test]
    fn a_file_outside_the_library_is_transferred_and_listed_by_the_next_scan() {
        let f = Fixture::new();
        let (dst_id, dst) = f.folder("dst");
        let outside = tempfile::tempdir().unwrap();
        let a = f.png(outside.path(), "a.png", 10);
        let b = f.png(outside.path(), "b.png", 20);
        let plan = f.plan(&[a.clone(), b.clone()], dst_id, FileTransferKind::Copy);
        assert!(plan.items.iter().all(|i| i.known.is_none()));
        let first = f.executor().transfer(&plan.items[0], dst_id, FileTransferKind::Copy, &PasteChoice::Direct).unwrap();
        assert!(first.needs_scan && a.exists() && dst.join("a.png").exists());
        let plan = f.plan(&[b.clone()], dst_id, FileTransferKind::Move);
        f.executor().transfer(&plan.items[0], dst_id, FileTransferKind::Move, &PasteChoice::Direct).unwrap();
        assert!(!b.exists() && dst.join("b.png").exists());
        assert!(f.names_in_db(dst_id).is_empty(), "nothing is listed before the scan");
        f.scan(dst_id);
        assert_eq!(f.names_in_db(dst_id), ["a.png", "b.png"]);
    }

    #[test]
    fn collisions_are_found_for_known_and_unknown_files_and_settled_three_ways() {
        let f = Fixture::new();
        let (dst_id, dst) = f.folder("dst");
        let existing = f.png(&dst, "a.png", 99);
        f.scan(dst_id);
        let outside = tempfile::tempdir().unwrap();
        let a = f.png(outside.path(), "a.png", 10);
        let plan = f.plan(&[a.clone()], dst_id, FileTransferKind::Copy);
        let item = &plan.items[0];
        let report = f.executor().conflict(item, dst_id, FileTransferKind::Copy).unwrap().expect("conflict");
        assert_eq!(report.source.full_path, a);
        assert_eq!(report.destination.full_path, existing);
        assert!(report.source.media_id.is_none() && report.destination.media_id.is_some());

        let refused = f.executor().transfer(item, dst_id, FileTransferKind::Copy, &PasteChoice::Direct);
        assert!(matches!(refused, Err(LibraryError::FileOperationAlreadyExists(_))));
        assert_eq!(std::fs::read(&existing).unwrap(), std::fs::read(f.png(outside.path(), "keep.png", 99)).unwrap());

        let renamed = f.executor().transfer(item, dst_id, FileTransferKind::Copy, &PasteChoice::Rename("a (1).png".into())).unwrap();
        assert_eq!(renamed.final_name, "a (1).png");
        assert!(dst.join("a (1).png").exists());

        f.executor().transfer(item, dst_id, FileTransferKind::Copy, &PasteChoice::Overwrite).unwrap();
        assert_eq!(std::fs::read(dst.join("a.png")).unwrap(), std::fs::read(&a).unwrap());
        assert!(!dst.read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("rustview-paste")), "no temp file is left");
    }

    #[test]
    fn overwriting_a_link_replaces_the_link_and_not_what_it_points_to() {
        let f = Fixture::new();
        let (dst_id, dst) = f.folder("dst");
        let precious = f.root.join("precious.png");
        image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3])).save(&precious).unwrap();
        let before = std::fs::read(&precious).unwrap();
        symlink(&precious, dst.join("a.png")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let a = f.png(outside.path(), "a.png", 10);
        let plan = f.plan(&[a.clone()], dst_id, FileTransferKind::Copy);
        assert!(f.executor().conflict(&plan.items[0], dst_id, FileTransferKind::Copy).unwrap().is_some(), "a link in the way is a collision");
        f.executor().transfer(&plan.items[0], dst_id, FileTransferKind::Copy, &PasteChoice::Overwrite).unwrap();
        assert_eq!(std::fs::read(&precious).unwrap(), before);
        assert_eq!(std::fs::read(dst.join("a.png")).unwrap(), std::fs::read(&a).unwrap());
    }

    #[test]
    fn a_listed_file_that_vanished_is_not_transferred_and_its_record_is_flagged() {
        let f = Fixture::new();
        let (src_id, src) = f.folder("src");
        let (dst_id, dst) = f.folder("dst");
        let a = f.png(&src, "a.png", 10);
        f.scan(src_id);
        let plan = f.plan(&[a.clone()], dst_id, FileTransferKind::Move);
        let media_id = plan.items[0].known.unwrap();
        // Another program moves it away between the plan and the paste.
        std::fs::remove_file(&a).unwrap();
        let error = f.executor().transfer(&plan.items[0], dst_id, FileTransferKind::Move, &PasteChoice::Direct).unwrap_err();
        assert!(matches!(&error, LibraryError::Io(e) if e.kind() == ErrorKind::NotFound), "{error}");
        assert!(dst.read_dir().unwrap().next().is_none());
        let record = f.library.media().get(media_id).unwrap().expect("the record stays");
        assert!(record.is_deleted && record.folder_id == src_id, "flagged, not purged and not moved");
    }

    #[test]
    fn a_read_only_target_fails_cleanly_and_leaves_the_source() {
        if is_root() {
            return;
        }
        let f = Fixture::new();
        let (dst_id, dst) = f.folder("dst");
        let outside = tempfile::tempdir().unwrap();
        let a = f.png(outside.path(), "a.png", 10);
        let plan = f.plan(&[a.clone()], dst_id, FileTransferKind::Move);
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = f.executor().transfer(&plan.items[0], dst_id, FileTransferKind::Move, &PasteChoice::Direct);
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert!(a.exists() && dst.read_dir().unwrap().next().is_none());
    }

    #[test]
    fn an_unreadable_source_leaves_no_partial_copy() {
        if is_root() {
            return;
        }
        let f = Fixture::new();
        let (dst_id, dst) = f.folder("dst");
        let outside = tempfile::tempdir().unwrap();
        let a = f.png(outside.path(), "a.png", 10);
        let plan = f.plan(&[a.clone()], dst_id, FileTransferKind::Copy);
        std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = f.executor().transfer(&plan.items[0], dst_id, FileTransferKind::Copy, &PasteChoice::Direct);
        std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(result.is_err());
        assert!(dst.read_dir().unwrap().next().is_none(), "no empty or partial file is left");
    }

    #[test]
    fn unusual_names_survive() {
        let f = Fixture::new();
        let (dst_id, dst) = f.folder("dst");
        let outside = tempfile::tempdir().unwrap();
        let names = ["äöü ß.png", "with space.png", "100%.png", "hash#1.png", "q?.png", "日本語.png", "a'b\"c.png"];
        let files: Vec<PathBuf> = names.iter().map(|n| f.png(outside.path(), n, 10)).collect();
        let plan = f.plan(&files, dst_id, FileTransferKind::Copy);
        assert_eq!(plan.items.len(), names.len());
        for item in &plan.items {
            f.executor().transfer(item, dst_id, FileTransferKind::Copy, &PasteChoice::Direct).unwrap();
        }
        for name in names {
            assert!(dst.join(name).exists(), "{name}");
        }
    }
}
