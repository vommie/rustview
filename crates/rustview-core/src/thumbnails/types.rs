use crate::settings::{
    AppSettings, DEFAULT_BATCH_COMMIT_SIZE, DEFAULT_CUSTOM_PREVIEW_SETTINGS, DEFAULT_GENERATE_VIDEO_PREVIEWS,
    DEFAULT_PREVIEW_FORMAT, DEFAULT_PREVIEW_FRAME_COUNT, DEFAULT_PREVIEW_QUALITY, DEFAULT_SCALING_ALGORITHM,
    DEFAULT_SHORT_VIDEO_THRESHOLD_SECS, DEFAULT_THUMBNAIL_DB_SIZE, DEFAULT_THUMBNAIL_FORMAT, DEFAULT_THUMBNAIL_QUALITY,
    DEFAULT_VIDEO_ENGINE, DEFAULT_VIDEO_OFFSET_PERCENT, LogObscurer, MAX_SHORT_VIDEO_THRESHOLD_SECS,
    MIN_SHORT_VIDEO_THRESHOLD_SECS, PerformanceSettings,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_THUMBNAIL_BOUND: u32 = DEFAULT_THUMBNAIL_DB_SIZE;
pub const THUMBNAIL_SIZE: u32 = DEFAULT_THUMBNAIL_DB_SIZE;
pub const THUMBNAIL_CHANNELS: usize = 3;
pub const THUMBNAIL_PIXELS: usize = (THUMBNAIL_SIZE * THUMBNAIL_SIZE) as usize;
pub const THUMBNAIL_BYTE_SIZE: usize = THUMBNAIL_PIXELS * THUMBNAIL_CHANNELS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ThumbnailPriority {
    P0Visible = 0,
    P1Scrolling = 1,
    P2Prefetch = 2,
    P3Background = 3,
    P4Maintenance = 4,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoThumbnailEngine {
    Ffmpeg = 0,
    Mpv = 1,
}

impl VideoThumbnailEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            VideoThumbnailEngine::Ffmpeg => "FFmpeg",
            VideoThumbnailEngine::Mpv => "MPV",
        }
    }

    pub fn from_str_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "mpv" | "libmpv" => VideoThumbnailEngine::Mpv,
            _ => VideoThumbnailEngine::Ffmpeg,
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
    pub video_engine: VideoThumbnailEngine,
    pub video_offset_percent: u8,
    pub short_video_threshold_secs: u32,
    pub generate_video_previews: bool,
    pub preview_frame_count: u32,
    pub custom_preview_settings: bool,
    pub preview_format: ThumbnailFormat,
    pub preview_quality: u8,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            max_bound: DEFAULT_THUMBNAIL_DB_SIZE,
            format: ThumbnailFormat::from_str_name(DEFAULT_THUMBNAIL_FORMAT),
            quality: DEFAULT_THUMBNAIL_QUALITY,
            scaling_algorithm: ScalingAlgorithm::from_str_name(DEFAULT_SCALING_ALGORITHM),
            batch_commit_size: DEFAULT_BATCH_COMMIT_SIZE,
            video_engine: VideoThumbnailEngine::from_str_name(DEFAULT_VIDEO_ENGINE),
            video_offset_percent: DEFAULT_VIDEO_OFFSET_PERCENT,
            short_video_threshold_secs: DEFAULT_SHORT_VIDEO_THRESHOLD_SECS,
            generate_video_previews: DEFAULT_GENERATE_VIDEO_PREVIEWS,
            preview_frame_count: DEFAULT_PREVIEW_FRAME_COUNT,
            custom_preview_settings: DEFAULT_CUSTOM_PREVIEW_SETTINGS,
            preview_format: ThumbnailFormat::from_str_name(DEFAULT_PREVIEW_FORMAT),
            preview_quality: DEFAULT_PREVIEW_QUALITY,
        }
    }
}

impl ThumbnailConfig {
    /// Creates a resolved `ThumbnailConfig` instance directly from `PerformanceSettings`.
    pub fn from_performance_settings(perf: &PerformanceSettings) -> Self {
        Self {
            max_bound: perf.thumbnail_db_size,
            format: ThumbnailFormat::from_str_name(&perf.thumbnail_format),
            quality: perf.thumbnail_quality,
            scaling_algorithm: ScalingAlgorithm::from_str_name(&perf.scaling_algorithm),
            batch_commit_size: perf.batch_commit_size,
            video_engine: VideoThumbnailEngine::from_str_name(&perf.video_engine),
            video_offset_percent: perf.video_offset_percent,
            short_video_threshold_secs: perf
                .short_video_threshold_secs
                .clamp(MIN_SHORT_VIDEO_THRESHOLD_SECS, MAX_SHORT_VIDEO_THRESHOLD_SECS),
            generate_video_previews: perf.generate_video_previews,
            preview_frame_count: perf.preview_frame_count.clamp(2, 100),
            custom_preview_settings: perf.custom_preview_settings,
            preview_format: ThumbnailFormat::from_str_name(&perf.preview_format),
            preview_quality: perf.preview_quality,
        }
    }

    /// Creates a resolved `ThumbnailConfig` from full `AppSettings`.
    pub fn from_app_settings(settings: &AppSettings) -> Self {
        Self::from_performance_settings(&settings.performance)
    }

    /// Returns a copy of this configuration with an overridden video timestamp position offset (1..=99%).
    #[inline]
    pub fn with_video_offset_percent(&self, offset: u8) -> Self {
        let mut cfg = self.clone();
        cfg.video_offset_percent = offset.clamp(1, 99);
        cfg
    }
}

/// Generates a thread-safe, non-blocking pseudo-random percentage between 1 and 99 (inclusive)
/// using splitmix64 bit mixing and nanosecond timestamp entropy with zero external crate dependencies.
pub fn generate_random_video_offset_percent() -> u8 {
    static SEED: AtomicU64 = AtomicU64::new(0x8543_6234_7235_2981);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(42, |d| d.as_nanos() as u64);

    let prev = SEED.fetch_add(0x9E37_79B9_7F4A_7C15 ^ nanos, Ordering::Relaxed);
    let mut x = prev ^ nanos;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;

    // Uniformly maps to range 1..=99
    ((x % 99) + 1) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailLocation {
    pub pack_id: u32,
    pub offset: u64,
    pub length: u32,
}

/// Container recording multi-frame preview positions inside the `PackStore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoPreviewRecord {
    pub frame_count: u32,
    pub frame_locations: Vec<ThumbnailLocation>,
}

/// Comprehensive storage footprint statistics across `PackStore` packfiles and LMDB database tables.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThumbnailStorageStats {
    pub pack_files_count: usize,
    pub pack_total_bytes: u64,
    pub pack_total_mb: f64,
    pub lmdb_thumbnails_count: usize,
    pub lmdb_previews_count: usize,
    pub lmdb_preview_frames_count: usize,
    pub lmdb_data_bytes: u64,
    pub lmdb_data_mb: f64,
    pub total_cache_bytes: u64,
    pub total_cache_mb: f64,
}

impl ThumbnailStorageStats {
    #[inline]
    pub fn avg_thumbnail_kb(&self) -> f64 {
        let total_frames = self.lmdb_thumbnails_count + self.lmdb_preview_frames_count;
        if total_frames > 0 {
            (self.pack_total_bytes as f64 / total_frames as f64) / 1024.0
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Contain,
    Cover,
}

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

#[inline]
pub fn anonymize_target(path: &Path, media_id: Option<u64>) -> String {
    LogObscurer::target(path, media_id)
}
