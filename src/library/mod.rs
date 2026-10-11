pub mod db;
pub mod error;
pub mod external_media_completer;
pub mod file_operations;
pub mod folder_operations;
pub mod folders;
pub mod media;
pub mod models;
pub mod refresh;
pub mod refresh_media_files;
pub mod schema;
pub mod tags;

pub use db::LibraryDatabase;
pub use error::LibraryError;
pub use external_media_completer::{
    ExternalMediaCompleter, ExternalMediaCompletionReport, ProcessedCompletionItem,
};
pub use file_operations::{is_same_file_path, robust_move_file, FileOperations, FilenameValidator};
pub use folder_operations::{
    FolderDeleteReport, FolderMoveReport, FolderOperations, FolderSubtreeMetrics, FolderValidator,
};
pub use folders::FolderManager;
pub use media::MediaRepository;
pub use models::{
    BatchMoveSummary, CachedFolderNode, DatabaseMeta, ExternalHashRecord, FileConflictItemDetails,
    FileConflictReport, FileConflictResolution, FileTransferKind, FolderHierarchySnapshot,
    FolderId, FolderRecord, FolderRecordCodec, MediaId, MediaRecord, MediaSearchParams, TagId,
    TagRecord,
};
pub use refresh::{FolderRefresher, RefreshReport};
pub use refresh_media_files::{MediaItemRefresher, RefreshedFileAttributes};
pub use schema::{DB_EXTERNAL_HASHES, DB_FOLDERS_BY_PARENT, SEQ_EXTERNAL_HASH_ID};
pub use tags::TagManager;

use std::path::{Path, PathBuf};

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

    pub fn external_media_completer(&self) -> ExternalMediaCompleter<'_> {
        ExternalMediaCompleter::new(&self.db)
    }

    pub fn tags(&self) -> TagManager<'_> {
        TagManager::new(&self.db)
    }

    pub fn refresher(&self) -> FolderRefresher<'_> {
        FolderRefresher::new(&self.db)
    }

    #[inline]
    pub fn resolve_media_path(&self, media_id: MediaId) -> Result<PathBuf, LibraryError> {
        self.media().resolve_path(media_id)
    }

    pub fn raw_db(&self) -> &LibraryDatabase {
        &self.db
    }
}
