use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ViewerError {
    #[error("Target media file does not exist on disk: {0}")]
    FileNotFound(PathBuf),

    #[error("Media record #{0} not found in database")]
    MediaNotFound(u64),

    #[error("I/O error during viewer operation: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image decoding error: {0}")]
    Image(#[from] image::ImageError),

    #[error("Database error: {0}")]
    Library(#[from] crate::library::error::LibraryError),

    #[error("Unsupported media format: {0:?}")]
    UnsupportedFormat(crate::media::MediaFormat),

    #[error("Failed to decode media frame: {0}")]
    DecodeFailed(String),

    #[error("MPV player error: {0}")]
    Mpv(String),

    #[error("X11 subwindow error: {0}")]
    X11(String),

    #[error("This action is not available for a Multiview tab")]
    MultiviewTab,
}
