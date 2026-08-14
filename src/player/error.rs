use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("libmpv error: {0}")]
    Mpv(#[from] libmpv2::Error),

    #[error("I/O error during video playback: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to set MPV property '{property}': {reason}")]
    PropertySetError { property: String, reason: String },

    #[error("Failed to get MPV property '{property}': {reason}")]
    PropertyGetError { property: String, reason: String },

    #[error("Player command failed: {0}")]
    CommandError(String),

    #[error("Player engine is not initialized or in an invalid state")]
    NotInitialized,
}
