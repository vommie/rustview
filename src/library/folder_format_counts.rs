//! Keeps the media counts of the folder tree in step with the formats the user has switched on.
//!
//! A folder's count is the number of media files it shows. Switching a format off hides its records, so every folder
//! loses as many files as it holds records of that format; switching it on gives them back. The counts are changed by
//! exactly that difference (never recomputed from the library): a folder whose files were never indexed keeps the count of
//! its tree scan, and switching a format off and on again returns every count to where it was.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folders::FolderManager;
use super::models::FolderId;
use crate::media::{FormatMask, MediaFormat};
use std::collections::HashMap;

/// What a change of the policy did to the folder counts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FormatCountReport {
    pub folders_changed: usize,
    /// Records that became hidden.
    pub hidden: u64,
    /// Records that are shown again.
    pub restored: u64,
}

pub struct FolderFormatCounts<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderFormatCounts<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Moves the folder counts from what `old` shows to what `new` shows. One pass over the media table plus one write
    /// transaction; call it from a worker thread.
    pub fn apply_change(&self, old: FormatMask, new: FormatMask) -> Result<FormatCountReport, LibraryError> {
        let changed: HashMap<MediaFormat, i64> = MediaFormat::all()
            .filter(|f| old.shows(*f) != new.shows(*f))
            .map(|f| (f, if new.shows(f) { 1 } else { -1 }))
            .collect();
        let mut report = FormatCountReport::default();
        if changed.is_empty() {
            return Ok(report);
        }

        let mut per_folder: HashMap<FolderId, i64> = HashMap::new();
        {
            let rtxn = self.db.env.read_txn()?;
            for entry in self.db.media.iter(&rtxn)? {
                let (_, record) = entry?;
                if record.is_deleted {
                    continue;
                }
                if let Some(step) = changed.get(&record.format) {
                    *per_folder.entry(record.folder_id).or_default() += step;
                    if *step > 0 {
                        report.restored += 1;
                    } else {
                        report.hidden += 1;
                    }
                }
            }
        }

        let mut wtxn = self.db.env.write_txn()?;
        for (folder_id, delta) in per_folder.into_iter().filter(|(_, d)| *d != 0) {
            FolderManager::adjust_folder_media_count_txn(self.db, &mut wtxn, folder_id, delta)?;
            report.folders_changed += 1;
        }
        wtxn.commit()?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{FolderScanOptions, Library};
    use crate::media::FormatPolicy;
    use parking_lot::Mutex;
    use std::path::Path;

    /// The policy is process-wide: tests that change it take turns and put it back.
    static POLICY_LOCK: Mutex<()> = Mutex::new(());

    fn write_image(path: &Path) {
        image::RgbImage::from_pixel(24, 16, image::Rgb([30, 120, 200])).save(path).expect("write image");
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        library: Library,
        root_id: FolderId,
        sub_id: FolderId,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        write_image(&root.join("a.png"));
        write_image(&root.join("b.jpg"));
        write_image(&root.join("sub").join("c.png"));
        std::fs::write(root.join("notes.txt"), "x").unwrap();
        let library = Library::open(&dir.path().join("library")).expect("open library");
        library.folders().scan_directory_tree(&root, FolderScanOptions::default(), None::<fn(usize)>).expect("scan");
        let id_of = |p: &Path| library.folders().get_folder_by_path(p).expect("lookup").expect("registered").id;
        let (root_id, sub_id) = (id_of(&root), id_of(&root.join("sub")));
        library.refresher().refresh_folder(root_id).expect("refresh root");
        library.refresher().refresh_folder(sub_id).expect("refresh sub");
        Fixture { _dir: dir, library, root_id, sub_id }
    }

    fn counts(f: &Fixture) -> [(u32, u32); 2] {
        let get = |id| {
            let rec = f.library.folders().get_folder(id).unwrap().unwrap();
            (rec.media_count, rec.recursive_media_count)
        };
        [get(f.root_id), get(f.sub_id)]
    }

    fn png_off() -> FormatMask {
        FormatMask::ALL_ENABLED.with(MediaFormat::Png, false)
    }

    #[test]
    fn a_refresh_leaves_the_records_of_switched_off_formats_alone() {
        let _guard = POLICY_LOCK.lock();
        let f = fixture();
        assert_eq!(counts(&f), [(2, 3), (1, 1)], "the tree and the refreshed folders agree");

        FormatPolicy::set(png_off());
        let report = f.library.refresher().refresh_folder(f.root_id);
        FormatPolicy::set(FormatMask::ALL_ENABLED);
        let report = report.expect("refresh");

        assert_eq!(report.hidden_format_media, 1, "a.png is left as it is");
        assert_eq!(report.ignored.disabled.get("png"), Some(&1));
        assert_eq!(report.deleted_media, 0, "a switched-off format is never marked missing");
        let all = f.library.media().get_all_by_folder(f.root_id).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|r| !r.is_deleted));
        assert_eq!(report.ignored.unsupported.get("txt"), Some(&1));
    }

    #[test]
    fn the_folder_counts_move_with_the_switches_and_return_exactly() {
        let _guard = POLICY_LOCK.lock();
        let f = fixture();

        let off = f.library.folder_format_counts().apply_change(FormatMask::ALL_ENABLED, png_off()).unwrap();
        assert_eq!(off, FormatCountReport { folders_changed: 2, hidden: 2, restored: 0 });
        assert_eq!(counts(&f), [(1, 1), (0, 0)]);

        let on = f.library.folder_format_counts().apply_change(png_off(), FormatMask::ALL_ENABLED).unwrap();
        assert_eq!(on, FormatCountReport { folders_changed: 2, hidden: 0, restored: 2 });
        assert_eq!(counts(&f), [(2, 3), (1, 1)]);

        let nothing = f.library.folder_format_counts().apply_change(png_off(), png_off()).unwrap();
        assert_eq!(nothing, FormatCountReport::default());
    }

    #[test]
    fn queries_and_lists_of_the_repository_skip_hidden_formats() {
        let _guard = POLICY_LOCK.lock();
        let f = fixture();
        FormatPolicy::set(png_off());
        let listed = f.library.media().get_by_folder(f.root_id).unwrap();
        let counted = f.library.media().get_active_count(f.root_id).unwrap();
        FormatPolicy::set(FormatMask::ALL_ENABLED);
        assert_eq!(listed.len(), 1);
        assert_eq!(counted, 1);
        assert_eq!(f.library.media().get_by_folder(f.root_id).unwrap().len(), 2);
        let by_format = f.library.media().count_by_format().unwrap();
        assert_eq!(by_format.get(&MediaFormat::Png), Some(&2));
        assert_eq!(by_format.get(&MediaFormat::Jpeg), Some(&1));
    }

    #[test]
    fn renaming_into_or_out_of_a_switched_off_format_moves_the_count() {
        let _guard = POLICY_LOCK.lock();
        let f = fixture();
        f.library.folder_format_counts().apply_change(FormatMask::ALL_ENABLED, png_off()).unwrap();
        assert_eq!(counts(&f), [(1, 1), (0, 0)]);

        FormatPolicy::set(png_off());
        let id = f.library.media().get_all_by_folder(f.root_id).unwrap().into_iter().find(|r| r.filename == "b.jpg").unwrap().id;
        let renamed = f.library.file_operations().rename_media(id, "b.png");
        let after_hide = counts(&f);
        let back = f.library.file_operations().rename_media(id, "b.jpg");
        let after_show = counts(&f);
        FormatPolicy::set(FormatMask::ALL_ENABLED);

        assert_eq!(renamed.unwrap().format, MediaFormat::Png);
        assert_eq!(after_hide, [(0, 0), (0, 0)], "b.png belongs to a switched-off format now");
        assert_eq!(back.unwrap().format, MediaFormat::Jpeg);
        assert_eq!(after_show, [(1, 1), (0, 0)]);
    }
}
