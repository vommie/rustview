use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SystemHandlerError {
    #[error("Target file does not exist on disk: {0}")]
    FileNotFound(PathBuf),

    #[error("I/O error during system operation: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image processing error: {0}")]
    Image(#[from] image::ImageError),

    #[error("No standard system file handler launcher (xdg-open, gio, etc.) available on system")]
    NoHandlerAvailable,

    #[error("Clipboard operation failed: {0}")]
    Clipboard(String),

    #[error("Drag and drop operation failed: {0}")]
    DragAndDrop(String),
}
