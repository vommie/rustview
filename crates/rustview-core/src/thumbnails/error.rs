use thiserror::Error;

#[derive(Debug, Error)]
pub enum ThumbnailError {
    #[error("I/O error during thumbnail processing: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image decoding error: {0}")]
    Image(#[from] image::ImageError),

    #[error("SIMD resize error: {0}")]
    Resize(#[from] fast_image_resize::ResizeError),

    #[error("Image buffer error: {0}")]
    ImageBuffer(#[from] fast_image_resize::ImageBufferError),

    #[error("FFmpeg extraction error: {0}")]
    Ffmpeg(String),

    #[error("SVG rendering error: {0}")]
    Svg(String),

    #[error("Pack store corruption or missing pack ID: {0}")]
    PackNotFound(u32),

    #[error("Out of bounds in pack {pack_id} at offset {offset} with length {length}")]
    PackOutOfBounds { pack_id: u32, offset: u64, length: u32 },

    #[error("Database engine error: {0}")]
    Heed(#[from] heed::Error),

    #[error("Library error: {0}")]
    Library(#[from] crate::library::error::LibraryError),
}
