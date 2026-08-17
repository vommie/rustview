pub mod db;
pub mod error;
pub mod file_operations;
pub mod folder_operations;
pub mod folders;
pub mod media;
pub mod models;
pub mod refresh;
pub mod refresh_folders;
pub mod refresh_media_files;
pub mod schema;
pub mod tags;

pub use db::LibraryDatabase;
pub use error::LibraryError;
pub use file_operations::{FileOperations, FilenameValidator};
pub use folder_operations::{
    FolderDeleteReport, FolderMoveReport, FolderOperations, FolderSubtreeMetrics, FolderValidator,
};
pub use folders::FolderManager;
pub use media::MediaRepository;
pub use models::{
    DatabaseMeta, ExternalHashRecord, FolderId, FolderRecord, FolderRecordCodec, MediaId,
    MediaRecord, MediaSearchParams, TagId, TagRecord,
};
pub use refresh::{FolderRefresher, RefreshReport};
pub use refresh_folders::{FolderTreeRefreshReport, FolderTreeRefresher};
pub use refresh_media_files::{MediaItemRefresher, RefreshedFileAttributes};
pub use schema::{DB_EXTERNAL_HASHES, SEQ_EXTERNAL_HASH_ID};
pub use tags::TagManager;

use std::path::Path;

/// Unified Library Facade coordinating all storage subsystems.
pub struct Library {
    db: LibraryDatabase,
}

impl Library {
    pub fn open(storage_path: &Path) -> Result<Self, LibraryError> {
        let db = LibraryDatabase::open(storage_path)?;
        Ok(Self { db })
    }

    pub fn folders(&self) -> FolderManager<'_> {
        FolderManager::new(&self.db)
    }

    pub fn folder_operations(&self) -> FolderOperations<'_> {
        FolderOperations::new(&self.db)
    }

    pub fn media(&self) -> MediaRepository<'_> {
        MediaRepository::new(&self.db)
    }

    pub fn file_operations(&self) -> FileOperations<'_> {
        FileOperations::new(&self.db)
    }

    pub fn media_refresher(&self) -> MediaItemRefresher<'_> {
        MediaItemRefresher::new(&self.db)
    }

    pub fn tags(&self) -> TagManager<'_> {
        TagManager::new(&self.db)
    }

    pub fn refresher(&self) -> FolderRefresher<'_> {
        FolderRefresher::new(&self.db)
    }

    pub fn folder_tree_refresher(&self) -> FolderTreeRefresher<'_> {
        FolderTreeRefresher::new(&self.db)
    }

    pub fn raw_db(&self) -> &LibraryDatabase {
        &self.db
    }
}
