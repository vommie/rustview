pub mod db;
pub mod error;
pub mod folders;
pub mod media;
pub mod models;
pub mod refresh;
pub mod schema;
pub mod tags;

pub use db::LibraryDatabase;
pub use error::LibraryError;
pub use folders::FolderManager;
pub use media::MediaRepository;
pub use models::{DatabaseMeta, FolderId, FolderRecord, MediaId, MediaRecord, TagId, TagRecord};
pub use refresh::{FolderRefresher, RefreshReport};
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

    pub fn media(&self) -> MediaRepository<'_> {
        MediaRepository::new(&self.db)
    }

    pub fn tags(&self) -> TagManager<'_> {
        TagManager::new(&self.db)
    }

    pub fn refresher(&self) -> FolderRefresher<'_> {
        FolderRefresher::new(&self.db)
    }

    pub fn raw_db(&self) -> &LibraryDatabase {
        &self.db
    }
}
