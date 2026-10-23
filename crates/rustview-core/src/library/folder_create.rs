//! Creating folders from a relative path typed by the user (like `mkdir -p`): the missing components are created on disk and
//! registered in the library one by one, without scanning anything. Components that already exist are left alone silently.

use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folder_operations::FolderValidator;
use super::folders::FolderManager;
use super::models::{FolderId, FolderRecord};
use crate::settings::LogObscurer;
use std::io::ErrorKind;
use std::path::PathBuf;

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
            return Err(LibraryError::FileOperationInvalidFilename("Folder name cannot be empty".to_string()));
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

        let parent = manager.get_folder(parent_id)?.ok_or(LibraryError::FolderNotFound(parent_id))?;
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
                    return Err(LibraryError::FileOperationAlreadyExists(format!("'{name}' already exists and is not a folder")));
                }
                Err(e) => return Err(LibraryError::Io(e)),
            };
            let record = manager.add_folder(&current)?;
            tracing::debug!(
                folder = %LogObscurer::folder(Some(record.id), &current),
                created_on_disk,
                "[FolderCreate: Component] Path component ready"
            );
            report.components.push(CreatedComponent { record, created_on_disk });
        }
        Ok(report)
    }
}
