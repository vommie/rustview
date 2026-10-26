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
    Library(#[from] rustview_core::library::error::LibraryError),

    #[error("Unsupported media format: {0:?}")]
    UnsupportedFormat(rustview_core::media::MediaFormat),

    #[error("Failed to decode media frame: {0}")]
    DecodeFailed(String),

    #[error("MPV player error: {0}")]
    Mpv(String),

    #[error("X11 error: {0}")]
    X11(String),

    #[error("This action needs a tab that shows exactly one medium")]
    MultiviewTab,
}
