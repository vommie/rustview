use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaType {
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaFormat {
    // Image Formats
    Jpeg,
    Png,
    Webp,
    Avif,
    Gif,
    Bmp,
    Tiff,

    // Video Formats
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,

    Unknown,
}

impl MediaFormat {
    /// Detects the media format based on the file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Self::Jpeg,
            "png" => Self::Png,
            "webp" => Self::Webp,
            "avif" => Self::Avif,
            "gif" => Self::Gif,
            "bmp" => Self::Bmp,
            "tif" | "tiff" => Self::Tiff,
            "mp4" | "m4v" => Self::Mp4,
            "mkv" => Self::Mkv,
            "webm" => Self::Webm,
            "mov" => Self::Mov,
            "avi" => Self::Avi,
            _ => Self::Unknown,
        }
    }

    /// Detects the media format from a file path.
    pub fn from_path(path: &Path) -> Self {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(Self::from_extension)
            .unwrap_or(Self::Unknown)
    }

    /// Identifies the high-level media type.
    pub fn media_type(&self) -> Option<MediaType> {
        match self {
            Self::Jpeg
            | Self::Png
            | Self::Webp
            | Self::Avif
            | Self::Gif
            | Self::Bmp
            | Self::Tiff => Some(MediaType::Image),
            Self::Mp4 | Self::Mkv | Self::Webm | Self::Mov | Self::Avi => Some(MediaType::Video),
            Self::Unknown => None,
        }
    }

    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}
