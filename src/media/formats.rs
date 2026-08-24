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
    Psd,

    // Video Formats
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,
    Mpeg,
    Flv,

    Unknown,
}

impl MediaFormat {
    /// Detects the media format based on the file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "jfif" | "jpe" => Self::Jpeg,
            "png" => Self::Png,
            "webp" => Self::Webp,
            "avif" => Self::Avif,
            "gif" => Self::Gif,
            "bmp" => Self::Bmp,
            "tif" | "tiff" => Self::Tiff,
            "psd" => Self::Psd,
            "mp4" | "m4v" => Self::Mp4,
            "mkv" => Self::Mkv,
            "webm" => Self::Webm,
            "mov" => Self::Mov,
            "avi" => Self::Avi,
            "mpg" | "mpeg" => Self::Mpeg,
            "flv" => Self::Flv,
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
            | Self::Tiff
            | Self::Psd => Some(MediaType::Image),
            Self::Mp4
            | Self::Mkv
            | Self::Webm
            | Self::Mov
            | Self::Avi
            | Self::Mpeg
            | Self::Flv => Some(MediaType::Video),
            Self::Unknown => None,
        }
    }

    /// Returns true if this format supports multi-frame animations (e.g. animated GIFs).
    #[inline]
    pub fn is_animated(&self) -> bool {
        matches!(self, Self::Gif)
    }

    /// Returns the recommended default file extension for this format in lowercase.
    pub fn default_extension(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Psd => "psd",
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::Avi => "avi",
            Self::Mpeg => "mpg",
            Self::Flv => "flv",
            Self::Unknown => "",
        }
    }

    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

/// Formats a byte size in Megabytes with 2 decimal places and no suffix (e.g. "310.01").
#[inline]
pub fn format_filesize_mb(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    format!("{:.2}", mb)
}

/// Formats a video duration in milliseconds to "hh:mm:ss.ms" format (e.g. "00:00:05.332").
/// For static images or unspecified duration, returns "-".
pub fn format_duration_hms_ms(duration_ms: u64, is_video: bool) -> String {
    if !is_video || duration_ms == 0 {
        return "-".to_string();
    }
    let ms = duration_ms % 1000;
    let total_secs = duration_ms / 1000;
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
}

/// Approximates image/video dimensions to the closest standard Aspect Ratio bucket:
/// 1:1, 5:4, 4:3, 16:9, 4:5, 3:4, 9:16.
pub fn approximate_aspect_ratio(width: u32, height: u32) -> &'static str {
    if width == 0 || height == 0 {
        return "-";
    }

    let q = (width as f64) / (height as f64);

    const BUCKETS: [(&str, f64); 7] = [
        ("9:16", 9.0 / 16.0), // 0.5625
        ("3:4", 3.0 / 4.0),   // 0.7500
        ("4:5", 4.0 / 5.0),   // 0.8000
        ("1:1", 1.0),         // 1.0000
        ("5:4", 5.0 / 4.0),   // 1.2500
        ("4:3", 4.0 / 3.0),   // 1.3333
        ("16:9", 16.0 / 9.0), // 1.7778
    ];

    let mut best_label = "1:1";
    let mut min_diff = f64::MAX;

    for (label, target) in BUCKETS {
        let diff = (q - target).abs();
        if diff < min_diff {
            min_diff = diff;
            best_label = label;
        }
    }

    best_label
}
