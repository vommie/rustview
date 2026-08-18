use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SystemHandlerError {
    #[error("Target file does not exist on disk: {0}")]
    FileNotFound(PathBuf),

    #[error("I/O error while launching default application: {0}")]
    Io(#[from] std::io::Error),

    #[error("No standard system file handler launcher (xdg-open, gio, etc.) available on system")]
    NoHandlerAvailable,
}
