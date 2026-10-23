use super::error::ThumbnailError;
use super::image::DecodedThumbnailFrame;
use super::types::{ScalingAlgorithm, ThumbnailConfig, VideoThumbnailEngine};

use std::path::Path;

mod ffmpeg;
mod mpv;
mod scaling;
pub use ffmpeg::FfmpegVideoThumbnailer;
pub use ffmpeg::SeekForwardExtractionParams;
pub use ffmpeg::SequentialExtractionParams;
pub use ffmpeg::VideoStreamContext;
pub use mpv::MpvVideoThumbnailer;
pub use scaling::DynamicScaler;
pub use scaling::apply_rotation_if_needed;
pub use scaling::map_scaling_algorithm_to_sws_flags;

#[derive(Debug, Clone)]
pub struct VideoExtractionResult {
    pub primary_frame: DecodedThumbnailFrame,
    pub preview_frames: Vec<DecodedThumbnailFrame>,
    pub orig_width: u32,
    pub orig_height: u32,
    pub duration_ms: u64,
}

pub trait VideoFrameDecoder: Send + Sync {
    /// Extracts both primary thumbnail and optional multi-frame preview in a single open-context pass.
    fn extract_video(&self, path: &Path, config: &ThumbnailConfig) -> Result<VideoExtractionResult, ThumbnailError>;

    /// Extracts a single frame at the given timestamp percentage (fallback).
    fn extract_frame(
        &self,
        path: &Path,
        time_percent: f64,
        max_bound: u32,
        scaling_algo: ScalingAlgorithm,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError>;
}

// =============================================================================
// 1. Ultra-Fast & Resilient FFmpeg Direct Video Thumbnailer
// =============================================================================

/// Longest edge `extract_native_frame` returns; a larger picture is scaled down to it.
const MAX_NATIVE_FRAME_EDGE: u32 = 16384;

// =============================================================================
// 2. MPV Headless Video Thumbnailer (libmpv2 Engine)
// =============================================================================

// =============================================================================
// 3. Dynamic Video Thumbnail Dispatcher
// =============================================================================

pub struct DynamicVideoThumbnailer {
    ffmpeg: FfmpegVideoThumbnailer,
    mpv: MpvVideoThumbnailer,
}

impl DynamicVideoThumbnailer {
    pub fn new() -> Self {
        Self {
            ffmpeg: FfmpegVideoThumbnailer::new(),
            mpv: MpvVideoThumbnailer::new(),
        }
    }

    pub fn decoder_for(&self, engine: VideoThumbnailEngine) -> &dyn VideoFrameDecoder {
        match engine {
            VideoThumbnailEngine::Ffmpeg => &self.ffmpeg,
            VideoThumbnailEngine::Mpv => &self.mpv,
        }
    }
}

impl Default for DynamicVideoThumbnailer {
    fn default() -> Self {
        Self::new()
    }
}
