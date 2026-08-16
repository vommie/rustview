use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_THUMBNAIL_BOUND: u32 = 256;
pub const THUMBNAIL_CHANNELS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ThumbnailPriority {
    P0Visible = 0,
    P1Scrolling = 1,
    P2Prefetch = 2,
    P3Background = 3,
    P4Maintenance = 4,
}

/// Supported thumbnail binary compression codecs stored inside the L2 PackStore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThumbnailFormat {
    Webp = 0,
    Jpeg = 1,
}

impl ThumbnailFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThumbnailFormat::Webp => "WebP",
            ThumbnailFormat::Jpeg => "JPEG",
        }
    }

    pub fn from_str_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => ThumbnailFormat::Jpeg,
            _ => ThumbnailFormat::Webp,
        }
    }

    /// Sniffs format from the initial magic bytes of compressed thumbnail data with zero overhead.
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
            return Some(ThumbnailFormat::Jpeg);
        }
        if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            return Some(ThumbnailFormat::Webp);
        }
        None
    }
}

/// Supported high-performance SIMD resizing interpolation algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScalingAlgorithm {
    Nearest = 0,
    Bilinear = 1,
    CatmullRom = 2,
    Lanczos3 = 3,
}

impl ScalingAlgorithm {
    pub fn to_resize_alg(&self) -> fast_image_resize::ResizeAlg {
        match self {
            ScalingAlgorithm::Nearest => fast_image_resize::ResizeAlg::Nearest,
            ScalingAlgorithm::Bilinear => {
                fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Bilinear)
            }
            ScalingAlgorithm::CatmullRom => {
                fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::CatmullRom)
            }
            ScalingAlgorithm::Lanczos3 => {
                fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3)
            }
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ScalingAlgorithm::Nearest => "Nearest",
            ScalingAlgorithm::Bilinear => "Bilinear",
            ScalingAlgorithm::CatmullRom => "CatmullRom",
            ScalingAlgorithm::Lanczos3 => "Lanczos3",
        }
    }

    pub fn from_str_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "nearest" => ScalingAlgorithm::Nearest,
            "bilinear" | "triangle" => ScalingAlgorithm::Bilinear,
            "lanczos" | "lanczos3" => ScalingAlgorithm::Lanczos3,
            _ => ScalingAlgorithm::CatmullRom,
        }
    }
}

/// Fully resolved thumbnail generation parameters passed to worker threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailConfig {
    pub max_bound: u32,
    pub format: ThumbnailFormat,
    pub quality: u8,
    pub scaling_algorithm: ScalingAlgorithm,
    pub batch_commit_size: usize,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            max_bound: DEFAULT_THUMBNAIL_BOUND,
            format: ThumbnailFormat::Webp,
            quality: 80,
            scaling_algorithm: ScalingAlgorithm::CatmullRom,
            batch_commit_size: 32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailLocation {
    pub pack_id: u32,
    pub offset: u64,
    pub length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Contain,
    Cover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailHandle {
    pub slot_index: u32,
    pub generation: u32,
}

/// Computes the target thumbnail dimensions preserving aspect ratio without black letterbox borders.
/// If `max_bound` is 256, the longest edge (width or height) will be exactly 256px.
#[inline]
pub fn calculate_aspect_dimensions(orig_w: u32, orig_h: u32, max_bound: u32) -> (u32, u32) {
    if orig_w == 0 || orig_h == 0 {
        return (max_bound.max(16), max_bound.max(16));
    }
    let max_b = max_bound.max(16) as f32;
    let w = orig_w as f32;
    let h = orig_h as f32;

    let scale = (max_b / w).min(max_b / h);
    let target_w = ((w * scale).round() as u32).clamp(1, max_bound);
    let target_h = ((h * scale).round() as u32).clamp(1, max_bound);
    (target_w, target_h)
}

/// Anonymizes a filesystem path for logging to guarantee privacy while preserving debugging utility.
pub fn anonymize_target(path: &Path, media_id: Option<u64>) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_else(|| ".unknown".to_string());

    let size_hint = std::fs::metadata(path)
        .map(|m| format!(", {:.2} KB", m.len() as f64 / 1024.0))
        .unwrap_or_default();

    match media_id {
        Some(id) => format!("MediaId({}) [*{}{}]", id, ext, size_hint),
        None => format!("Anonymous [*{}{}]", ext, size_hint),
    }
}
