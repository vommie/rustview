//! Creating folders from a relative path typed by the user (like `mkdir -p`): the missing components are created on disk and
//! registered in the library one by one, without scanning anything. Components that already exist are left alone silently.
//! Also the records of a folder an explicit save wrote into (`register_existing`), inside the tree's root or outside it.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folder_operations::FolderValidator;
use super::folders::{FolderManager, normalize_path};
use super::models::{FolderId, FolderRecord};
use crate::settings::LogObscurer;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Turns the text of the "New Folder" dialog into folder names.
pub struct FolderPathPlanner;

impl FolderPathPlanner {
    /// Splits `input` at `/` into validated names. Empty parts and `.` are skipped; an absolute path, `..` and invalid names are
    /// rejected, so a path can never leave the folder it is created in.
    pub fn parse(input: &str) -> Result<Vec<String>, LibraryError> {
        let trimmed = input.trim();
        if trimmed.starts_with('/') {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Enter a path relative to the folder, without a leading '/'".to_string(),
            ));
        }

        let mut names = Vec::new();
        for part in trimmed.split('/') {
            let part = part.trim();
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return Err(LibraryError::FileOperationInvalidFilename(
                    "A path cannot contain '..' (it would leave the folder)".to_string(),
                ));
            }
            names.push(FolderValidator::validate(part)?);
        }

        if names.is_empty() {
            return Err(LibraryError::FileOperationInvalidFilename(
                "Folder name cannot be empty".to_string(),
            ));
        }
        Ok(names)
    }
}

/// One component of a created path.
#[derive(Debug, Clone)]
pub struct CreatedComponent {
    pub record: FolderRecord,
    /// The directory did not exist before (it exists already for the other components, which are only registered).
    pub created_on_disk: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FolderCreateReport {
    /// Every component of the path from the top, existing ones included.
    pub components: Vec<CreatedComponent>,
}

impl FolderCreateReport {
    /// Number of directories that were created on disk.
    pub fn created_count(&self) -> usize {
        self.components.iter().filter(|c| c.created_on_disk).count()
    }

    /// All records of the path, for patching the folder snapshot.
    pub fn records(&self) -> Vec<FolderRecord> {
        self.components.iter().map(|c| c.record.clone()).collect()
    }
}

pub struct FolderCreator<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderCreator<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Creates the folders of `relative_path` below the folder `parent_id`. Existing components are skipped without a message,
    /// a component that exists as a file is an error. Explicit user action only (it touches the drive).
    pub fn create_path(&self, parent_id: FolderId, relative_path: &str) -> Result<FolderCreateReport, LibraryError> {
        let names = FolderPathPlanner::parse(relative_path)?;
        let manager = FolderManager::new(self.db);

        let parent = manager
            .get_folder(parent_id)?
            .ok_or(LibraryError::FolderNotFound(parent_id))?;
        if parent.is_deleted {
            return Err(LibraryError::FileOperationInvalidFilename(
                "The folder does not exist on disk any more".to_string(),
            ));
        }
        let mut current = PathBuf::from(&parent.path);
        if !current.is_dir() {
            return Err(LibraryError::FileOperationInvalidFilename(
                "The folder does not exist on disk any more".to_string(),
            ));
        }

        let mut report = FolderCreateReport::default();
        for name in names {
            current.push(&name);
            let created_on_disk = match std::fs::create_dir(&current) {
                Ok(()) => true,
                Err(e) if e.kind() == ErrorKind::AlreadyExists && current.is_dir() => false,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    return Err(LibraryError::FileOperationAlreadyExists(format!(
                        "'{name}' already exists and is not a folder"
                    )));
                }
                Err(e) => return Err(LibraryError::Io(e)),
            };
            let record = manager.add_folder(&current)?;
            tracing::debug!(
                folder = %LogObscurer::folder(Some(record.id), &current),
                created_on_disk,
                "[FolderCreate: Component] Path component ready"
            );
            report.components.push(CreatedComponent {
                record,
                created_on_disk,
            });
        }
        Ok(report)
    }

    /// Gives the existing folder `folder` (an explicit save just wrote into it) a record, without scanning anything. Inside the
    /// tree's root (`scope_root`) the folders between it and the nearest folder the library knows are registered too, top down,
    /// so the tree links them at once. Outside the root, or when no folder above it is known, only `folder` itself is registered:
    /// the tree does not show such a record, and a later scan of a root that contains it takes the record over with its media.
    /// Returns the record and how many records were added.
    pub fn register_existing(
        &self,
        folder: &Path,
        scope_root: Option<&Path>,
    ) -> Result<(FolderRecord, usize), LibraryError> {
        let manager = FolderManager::new(self.db);
        let folder = normalize_path(folder);
        let known = |path: &Path| -> Result<bool, LibraryError> {
            Ok(manager.get_folder_by_path(path)?.is_some_and(|r| !r.is_deleted))
        };
        let mut chain = vec![folder.clone()];
        if let Some(root) = scope_root.map(normalize_path).filter(|root| folder.starts_with(root)) {
            let mut above = folder.parent();
            let mut linked = false;
            while let Some(path) = above.filter(|p| p.starts_with(&root)) {
                if known(path)? {
                    linked = true;
                    break;
                }
                chain.push(path.to_path_buf());
                above = path.parent();
            }
            if !linked {
                chain.truncate(1);
            }
        }
        let mut added = 0;
        let mut record = None;
        for path in chain.iter().rev() {
            record = Some(match manager.get_folder_by_path(path)? {
                Some(existing) if !existing.is_deleted => existing,
                _ => {
                    added += 1;
                    manager.add_folder(path)?
                }
            });
        }
        let record = record.ok_or_else(|| LibraryError::FolderNotFound(0))?;
        tracing::info!(
            folder = %LogObscurer::folder(Some(record.id), &folder),
            added,
            in_scope = scope_root.is_some_and(|root| folder.starts_with(normalize_path(root))),
            "[FolderCreate: Register] Folder of a saved file registered"
        );
        Ok((record, added))
    }
}

#[cfg(test)]
mod tests {
    use crate::library::Library;

    #[test]
    fn a_saved_folder_inside_the_root_is_linked_to_the_nearest_known_folder() {
        let store = tempfile::tempdir().expect("store");
        let disk = tempfile::tempdir().expect("disk");
        let library = Library::open(store.path()).expect("library");
        let root = disk.path().join("root");
        let deep = root.join("a").join("b");
        std::fs::create_dir_all(&deep).expect("mkdir");
        let root_record = library.folders().add_folder(&root).expect("root");

        let (record, added) = library
            .folder_creator()
            .register_existing(&deep, Some(&root))
            .expect("register");
        assert_eq!(added, 2, "a and b are registered");
        let a = library
            .folders()
            .get_folder_by_path(&root.join("a"))
            .expect("read")
            .expect("a");
        assert_eq!(a.parent_id, Some(root_record.id));
        assert_eq!(record.parent_id, Some(a.id));

        let (again, added) = library
            .folder_creator()
            .register_existing(&deep, Some(&root))
            .expect("register again");
        assert_eq!(
            (again.id, added),
            (record.id, 0),
            "a known folder is not registered twice"
        );
    }

    #[test]
    fn a_saved_folder_outside_the_root_gets_a_record_of_its_own() {
        let store = tempfile::tempdir().expect("store");
        let disk = tempfile::tempdir().expect("disk");
        let library = Library::open(store.path()).expect("library");
        let root = disk.path().join("root");
        let outside = disk.path().join("elsewhere").join("pictures");
        std::fs::create_dir_all(&root).expect("mkdir root");
        std::fs::create_dir_all(&outside).expect("mkdir outside");
        library.folders().add_folder(&root).expect("root");

        let (record, added) = library
            .folder_creator()
            .register_existing(&outside, Some(&root))
            .expect("register");
        assert_eq!(added, 1, "only the folder itself");
        assert_eq!(record.parent_id, None);
        assert!(
            library
                .folders()
                .get_folder_by_path(&disk.path().join("elsewhere"))
                .expect("read")
                .is_none()
        );
    }

    #[test]
    fn without_a_known_folder_above_only_the_folder_itself_is_registered() {
        let store = tempfile::tempdir().expect("store");
        let disk = tempfile::tempdir().expect("disk");
        let library = Library::open(store.path()).expect("library");
        let root = disk.path().join("root");
        let deep = root.join("x").join("y");
        std::fs::create_dir_all(&deep).expect("mkdir");

        let (_, added) = library
            .folder_creator()
            .register_existing(&deep, Some(&root))
            .expect("register");
        assert_eq!(added, 1, "an unscanned root does not get a partial tree");
        assert!(library.folders().get_folder_by_path(&root).expect("read").is_none());
    }
}
