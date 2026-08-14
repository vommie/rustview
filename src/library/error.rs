use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("Database engine error: {0}")]
    Heed(#[from] heed::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization / Deserialization error: {0}")]
    Bincode(#[from] bincode::Error),

    #[error("Media processing error: {0}")]
    Media(#[from] crate::media::metadata::MetadataError),

    #[error("Hash calculation error: {0}")]
    Hash(#[from] crate::media::hashing::HashError),

    #[error("Schema version mismatch: found {found}, expected {expected}")]
    SchemaVersionMismatch { found: u32, expected: u32 },

    #[error("Folder not found: {0}")]
    FolderNotFound(u64),

    #[error("Media not found: {0}")]
    MediaNotFound(u64),

    #[error("Tag not found: {0}")]
    TagNotFound(u32),

    #[error("Parent tag cycle or invalid parent reference")]
    InvalidTagHierarchy,

    #[error("Folder path already registered: {0}")]
    DuplicateFolder(String),
}
