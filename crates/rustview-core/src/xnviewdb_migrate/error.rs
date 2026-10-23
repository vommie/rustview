use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("SQLite database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error during migration: {0}")]
    Io(#[from] std::io::Error),

    #[error("LMDB library database error: {0}")]
    Library(#[from] crate::library::error::LibraryError),

    #[error("LMDB heed error: {0}")]
    Heed(#[from] heed::Error),

    #[error("XnView database file not found: {0}")]
    FileNotFound(String),

    #[error("Migration was cancelled by the user")]
    UserCancelled,

    #[error("Invalid XnView database format: {0}")]
    InvalidDatabase(String),
}
