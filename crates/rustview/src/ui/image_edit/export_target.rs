//! Where "Save As" writes: the folder the user typed (an existing folder, one that is created when saving, or a path that cannot
//! be used), the extension of the new file and the words for the rating and tags it can take over. The folder is looked at on
//! every change while the dialog is open (an explicit save; nothing else reads the drive for it).

use crate::system::fs_access::can_write_directory;
use rustview_core::image_edit::formats::WriteFormat;
use rustview_core::media::MediaFormat;
use std::path::{Component, Path, PathBuf};

/// The folder field of the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderCheck {
    /// It exists and files can be written there.
    Ready(PathBuf),
    /// It does not exist yet; it is created when saving (the nearest existing folder above it is writable).
    Create(PathBuf),
    /// It cannot be used; the text says why.
    Invalid(String),
}

impl FolderCheck {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Self::Ready(path) | Self::Create(path) => Some(path),
            Self::Invalid(_) => None,
        }
    }
}

/// Longest name of one folder (bytes; the limit of the usual Linux file systems).
const MAX_NAME_BYTES: usize = 255;

/// What the folder field `text` means. `home` expands a leading `~`.
pub fn check_folder(text: &str, home: Option<&Path>) -> FolderCheck {
    let text = text.trim();
    if text.is_empty() {
        return FolderCheck::Invalid("Enter the folder to save in.".to_string());
    }
    if text.contains('\0') {
        return FolderCheck::Invalid("The folder contains a character that cannot be used.".to_string());
    }
    let path = match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(text),
    };
    if !path.is_absolute() {
        return FolderCheck::Invalid("Enter the full path of the folder (starting with /).".to_string());
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            Component::Normal(name) if name.len() > MAX_NAME_BYTES => {
                return FolderCheck::Invalid("A folder name in the path is too long.".to_string());
            }
            other => clean.push(other.as_os_str()),
        }
    }
    if clean.is_dir() {
        return if can_write_directory(&clean) {
            FolderCheck::Ready(clean)
        } else {
            FolderCheck::Invalid("The folder is read-only.".to_string())
        };
    }
    if clean.symlink_metadata().is_ok() {
        return FolderCheck::Invalid(format!("'{}' is a file, not a folder.", clean.display()));
    }
    let mut above = clean.parent();
    while let Some(parent) = above {
        if parent.is_dir() {
            return if can_write_directory(parent) {
                FolderCheck::Create(clean)
            } else {
                FolderCheck::Invalid(format!(
                    "The folder cannot be created: '{}' is read-only.",
                    parent.display()
                ))
            };
        }
        if parent.symlink_metadata().is_ok() {
            return FolderCheck::Invalid(format!(
                "The folder cannot be created: '{}' is a file.",
                parent.display()
            ));
        }
        above = parent.parent();
    }
    FolderCheck::Invalid("The folder cannot be created.".to_string())
}

/// The extension of a new file of `format`: the original's own spelling ("JPG", "jpeg", "tif") when the original is of that
/// format, so the unchanged name means the original file; otherwise the format's usual one.
pub fn extension_for(format: WriteFormat, original_filename: &str) -> String {
    let own = Path::new(original_filename)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .filter(|_| MediaFormat::from_path(Path::new(original_filename)) == format.media_format());
    own.unwrap_or_else(|| format.extension().to_string())
}

/// "4 stars and 2 tags", "1 tag", "the rating of 3 stars": what a copy takes over; empty when there is nothing.
pub fn describe_assignments(rating: u8, tags: usize) -> String {
    let stars = match rating {
        0 => None,
        1 => Some("rating (1 star)".to_string()),
        n => Some(format!("rating ({n} stars)")),
    };
    let tags = match tags {
        0 => None,
        1 => Some("tag".to_string()),
        n => Some(format!("{n} tags")),
    };
    match (stars, tags) {
        (Some(s), Some(t)) => format!("{s} and {t}"),
        (Some(s), None) => s,
        (None, Some(t)) => t,
        (None, None) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn an_existing_folder_is_ready_and_a_missing_one_is_created() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        assert_eq!(
            check_folder(&root.display().to_string(), None),
            FolderCheck::Ready(root.to_path_buf())
        );
        let deep = root.join("new").join("deeper");
        assert_eq!(
            check_folder(&format!("  {}/  ", deep.display()), None),
            FolderCheck::Create(deep.clone())
        );
        assert_eq!(
            check_folder(&format!("{}/new/../new/./deeper", root.display()), None),
            FolderCheck::Create(deep)
        );
    }

    #[test]
    fn unusable_paths_say_why() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").expect("write");
        let reason = |text: &str| match check_folder(text, None) {
            FolderCheck::Invalid(reason) => reason,
            other => panic!("{text}: {other:?}"),
        };
        assert!(reason("").contains("Enter the folder"));
        assert!(reason("relative/path").contains("full path"));
        assert!(reason(&file.display().to_string()).contains("is a file"));
        assert!(reason(&format!("{}/below", file.display())).contains("is a file"));
        assert!(reason(&format!("/tmp/{}", "x".repeat(300))).contains("too long"));
        assert!(reason("/tmp/a\0b").contains("character"));
    }

    #[test]
    fn a_read_only_folder_is_refused() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).expect("mkdir");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).expect("chmod");
        let inside = check_folder(&locked.display().to_string(), None);
        let below = check_folder(&locked.join("new").display().to_string(), None);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).expect("chmod back");
        assert_eq!(inside, FolderCheck::Invalid("The folder is read-only.".to_string()));
        assert!(matches!(below, FolderCheck::Invalid(reason) if reason.contains("read-only")));
    }

    #[test]
    fn a_tilde_means_the_home_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            check_folder("~", Some(dir.path())),
            FolderCheck::Ready(dir.path().to_path_buf())
        );
        assert_eq!(
            check_folder("~/pictures", Some(dir.path())),
            FolderCheck::Create(dir.path().join("pictures"))
        );
    }

    #[test]
    fn the_original_spelling_of_the_extension_is_kept_for_its_own_format() {
        assert_eq!(extension_for(WriteFormat::Jpeg, "beach.JPG"), "JPG");
        assert_eq!(extension_for(WriteFormat::Jpeg, "beach.jpeg"), "jpeg");
        assert_eq!(extension_for(WriteFormat::Tiff, "scan.tif"), "tif");
        assert_eq!(extension_for(WriteFormat::Png, "beach.JPG"), "png");
        assert_eq!(extension_for(WriteFormat::Webp, "photo.heic"), "webp");
    }

    #[test]
    fn the_assignments_are_named_in_plain_words() {
        assert_eq!(describe_assignments(4, 2), "rating (4 stars) and 2 tags");
        assert_eq!(describe_assignments(1, 0), "rating (1 star)");
        assert_eq!(describe_assignments(0, 1), "tag");
        assert_eq!(describe_assignments(0, 0), "");
    }
}
