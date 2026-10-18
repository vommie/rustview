pub mod db;
pub mod error;
pub mod external_media_completer;
pub mod file_operations;
pub mod folder_create;
pub mod folder_diagnostics;
pub mod folder_mount_filter;
pub mod folder_move;
pub mod folder_operations;
pub mod folder_projection;
// TEMPORARY DEBUG FEATURE: remove this module together with src/library/folder_reset.rs
pub mod folder_reset;
pub mod folder_snapshot;
pub mod folder_snapshot_patch;
pub mod folder_walker;
pub mod folders;
pub mod media;
pub mod models;
pub mod refresh;
pub mod refresh_media_files;
pub mod schema;
pub mod selection_delete;
pub mod tags;

pub use db::LibraryDatabase;
pub use error::LibraryError;
pub use external_media_completer::{
    ExternalMediaCompleter, ExternalMediaCompletionReport, ProcessedCompletionItem,
};
pub use file_operations::{is_same_file_path, robust_move_file, FileOperations, FilenameValidator};
pub use folder_diagnostics::FolderDiagnostics;
pub use folder_mount_filter::FolderMountFilter;
pub use folder_move::{FolderMoveExecutor, FolderMovePatch};
pub use folder_operations::{
    FolderDeleteReport, FolderMoveReport, FolderOperations, FolderSubtreeMetrics, FolderValidator,
};
// TEMPORARY DEBUG FEATURE: remove together with src/library/folder_reset.rs
pub use folder_reset::FolderTreeReset;
pub use folders::{FolderManager, FolderScanOptions};
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

    pub fn folder_creator(&self) -> folder_create::FolderCreator<'_> {
        folder_create::FolderCreator::new(&self.db)
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

    /// Registers the configured root directory as the scope of the in-memory folder tree snapshot.
    /// `None` yields an empty folder tree.
    pub fn set_folder_tree_root(&self, root: Option<PathBuf>) {
        self.db.set_tree_root(root);
    }

    pub fn raw_db(&self) -> &LibraryDatabase {
        &self.db
    }
}
