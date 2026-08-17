use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Library error: {0}")]
    Library(#[from] crate::library::error::LibraryError),

    #[error("LMDB database error: {0}")]
    Heed(#[from] heed::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Missing required argument: {0}")]
    MissingArgument(String),

    #[error("Unknown API command: {0}")]
    UnknownCommand(String),

    #[error("Media file not found: {0}")]
    MediaNotFound(String),
}
