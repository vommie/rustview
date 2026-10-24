//! Is the file on disk still the one that was edited? (Read only when the user saves.)
//!
//! The identity of a file is taken when its picture is decoded for a tile (the decode reads the file anyway) and compared
//! right before saving and once more right before the original is replaced: size, modification time to the nanosecond,
//! inode and device. Any difference means somebody else changed, replaced or removed the file in the meantime, and the
//! user decides what happens (`conflict_choices`).

use std::fs::Metadata;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub len: u64,
    /// Modification time in nanoseconds since the epoch.
    pub modified_ns: i128,
    pub inode: u64,
    pub device: u64,
}

impl FileIdentity {
    pub fn of_metadata(meta: &Metadata) -> Self {
        Self {
            len: meta.len(),
            modified_ns: i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
            inode: meta.ino(),
            device: meta.dev(),
        }
    }

    /// The identity of the file `path` names (a symbolic link is followed: the picture is the file it points to).
    pub fn read(path: &Path) -> io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        if !meta.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
        }
        Ok(Self::of_metadata(&meta))
    }
}

/// What the file on disk looks like compared with the one that was edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskState {
    Unchanged,
    /// Changed, replaced by another file or touched.
    Modified,
    /// Not there any more (deleted, renamed, the drive was removed).
    Missing,
    /// The identity of the edited file is not known (it could not be read when the picture was decoded), so a change cannot
    /// be ruled out.
    Unverified,
    /// The file cannot be examined (no permission, an I/O error).
    Unreadable(String),
}

impl DiskState {
    pub fn is_conflict(&self) -> bool {
        !matches!(self, Self::Unchanged)
    }
}

/// Compares the file at `path` with the identity it had when the picture was decoded.
pub fn check(path: &Path, expected: Option<FileIdentity>) -> DiskState {
    match FileIdentity::read(path) {
        Ok(now) => match expected {
            Some(expected) if expected == now => DiskState::Unchanged,
            Some(_) => DiskState::Modified,
            None => DiskState::Unverified,
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => DiskState::Missing,
        Err(e) => DiskState::Unreadable(e.to_string()),
    }
}

/// What the user can do when the file changed under the edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictChoice {
    /// Write the edited picture anyway (replaces the other change, or creates the file again where it was).
    OverwriteAnyway,
    /// Write the edited picture to a new file (the Save As dialog); the file on disk stays as it is.
    SaveAsCopy,
    /// Drop the edits and show the file as it is on disk now.
    DiscardAndRefresh,
    /// Keep editing; nothing is written.
    Cancel,
}

impl ConflictChoice {
    pub const fn code(self) -> i32 {
        match self {
            Self::OverwriteAnyway => 0,
            Self::SaveAsCopy => 1,
            Self::DiscardAndRefresh => 2,
            Self::Cancel => 3,
        }
    }

    pub fn from_code(code: i32) -> Option<Self> {
        [
            Self::OverwriteAnyway,
            Self::SaveAsCopy,
            Self::DiscardAndRefresh,
            Self::Cancel,
        ]
        .into_iter()
        .find(|c| c.code() == code)
    }
}

/// The texts and the choices of the conflict dialog for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictPrompt {
    pub title: String,
    pub message: String,
    /// Left to right; the safe choice (a copy) is the default button.
    pub choices: Vec<(ConflictChoice, String)>,
}

/// The dialog for `state` (`None` when there is no conflict).
pub fn conflict_prompt(state: &DiskState, filename: &str) -> Option<ConflictPrompt> {
    let (title, message, overwrite_label, reload) = match state {
        DiskState::Unchanged => return None,
        DiskState::Modified => (
            "File Changed on Disk",
            format!(
                "'{filename}' was changed by another program after RustView opened it. Saving now replaces that change with \
                 your edited picture."
            ),
            "Overwrite Anyway",
            true,
        ),
        DiskState::Unverified => (
            "File May Have Changed",
            format!(
                "RustView could not check whether '{filename}' was changed by another program after it was opened. Saving \
                 replaces whatever is on disk now."
            ),
            "Overwrite Anyway",
            true,
        ),
        DiskState::Missing => (
            "File Not Found",
            format!(
                "'{filename}' is no longer where it was (deleted, renamed or on a drive that was removed). Saving here creates \
                 it again with your edited picture."
            ),
            "Save Here Anyway",
            false,
        ),
        DiskState::Unreadable(reason) => (
            "File Cannot Be Checked",
            format!("'{filename}' cannot be examined ({reason}). It is safer to save a copy."),
            "Overwrite Anyway",
            false,
        ),
    };
    let mut choices = vec![
        (ConflictChoice::OverwriteAnyway, overwrite_label.to_string()),
        (ConflictChoice::SaveAsCopy, "Save As Copy...".to_string()),
    ];
    if reload {
        choices.push((ConflictChoice::DiscardAndRefresh, "Discard & Refresh".to_string()));
    }
    choices.push((ConflictChoice::Cancel, "Cancel".to_string()));
    Some(ConflictPrompt {
        title: title.to_string(),
        message,
        choices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{Duration, SystemTime};

    #[test]
    fn an_untouched_file_is_unchanged_and_any_change_is_seen() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, b"one").expect("write");
        let identity = FileIdentity::read(&path).ok();
        assert_eq!(check(&path, identity), DiskState::Unchanged);

        // Same size, new content and time.
        let file = std::fs::OpenOptions::new().write(true).open(&path).expect("open");
        (&file).write_all(b"two").expect("write");
        file.set_modified(SystemTime::now() + Duration::from_secs(5))
            .expect("mtime");
        drop(file);
        assert_eq!(check(&path, identity), DiskState::Modified);

        // Replaced by another file with the same size and time (a rename over it): the inode tells.
        let identity = FileIdentity::read(&path).ok();
        let other = dir.path().join("b.jpg");
        std::fs::write(&other, b"two").expect("write");
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).expect("mtime");
        std::fs::File::options()
            .write(true)
            .open(&other)
            .and_then(|f| f.set_modified(mtime))
            .expect("mtime");
        std::fs::rename(&other, &path).expect("rename");
        assert_eq!(check(&path, identity), DiskState::Modified);

        std::fs::remove_file(&path).expect("remove");
        assert_eq!(check(&path, identity), DiskState::Missing);
    }

    #[test]
    fn an_unknown_identity_is_never_taken_for_unchanged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"x").expect("write");
        assert_eq!(check(&path, None), DiskState::Unverified);
        assert!(check(dir.path(), None).is_conflict());
    }

    #[test]
    fn the_dialog_offers_what_makes_sense_for_each_state() {
        assert!(conflict_prompt(&DiskState::Unchanged, "a.jpg").is_none());
        let modified = conflict_prompt(&DiskState::Modified, "a.jpg").expect("prompt");
        assert_eq!(
            modified.choices.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            [
                ConflictChoice::OverwriteAnyway,
                ConflictChoice::SaveAsCopy,
                ConflictChoice::DiscardAndRefresh,
                ConflictChoice::Cancel
            ]
        );
        assert!(modified.message.contains("a.jpg"));
        let missing = conflict_prompt(&DiskState::Missing, "a.jpg").expect("prompt");
        assert_eq!(missing.choices[0].1, "Save Here Anyway");
        assert!(
            !missing
                .choices
                .iter()
                .any(|(c, _)| *c == ConflictChoice::DiscardAndRefresh)
        );
        for choice in [
            ConflictChoice::OverwriteAnyway,
            ConflictChoice::SaveAsCopy,
            ConflictChoice::DiscardAndRefresh,
            ConflictChoice::Cancel,
        ] {
            assert_eq!(ConflictChoice::from_code(choice.code()), Some(choice));
        }
    }
}
