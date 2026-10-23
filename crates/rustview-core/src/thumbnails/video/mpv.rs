//! The libmpv based video thumbnailer (fallback for formats `FFmpeg` cannot seek).

use super::{FfmpegVideoThumbnailer, VideoExtractionResult, VideoFrameDecoder};
use crate::thumbnails::error::ThumbnailError;
use crate::thumbnails::image::DecodedThumbnailFrame;
use crate::thumbnails::types::{ScalingAlgorithm, ThumbnailConfig, VideoThumbnailEngine};
use crate::util::cast::FloatToInt;
use libmpv2::Mpv;
use std::path::Path;

#[derive(Default)]
pub struct MpvVideoThumbnailer;

impl MpvVideoThumbnailer {
    pub fn new() -> Self {
        Self
    }
}

impl VideoFrameDecoder for MpvVideoThumbnailer {
    fn extract_video(&self, path: &Path, config: &ThumbnailConfig) -> Result<VideoExtractionResult, ThumbnailError> {
        if let Ok(mpv) = Mpv::with_initializer(|init| {
            init.set_property("vo", "null")?;
            init.set_property("ao", "null")?;
            init.set_property("audio", "no")?;
            init.set_property("hwdec", "auto-safe")?;
            init.set_property("pause", "yes")?;
            Ok(())
        }) {
            let path_str = path.to_string_lossy();
            let _ = mpv.command("loadfile", &[&path_str, "replace"]);
        }

        FfmpegVideoThumbnailer::new().extract_video(path, config)
    }

    fn extract_frame(
        &self,
        path: &Path,
        time_percent: f64,
        max_bound: u32,
        scaling_algo: ScalingAlgorithm,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        let config = ThumbnailConfig {
            max_bound,
            scaling_algorithm: scaling_algo,
            video_offset_percent: (time_percent * 100.0).clamp(1.0, 99.0).to_int::<u8>(),
            generate_video_previews: false,
            video_engine: VideoThumbnailEngine::Mpv,
            ..Default::default()
        };
        let res = self.extract_video(path, &config)?;
        Ok(res.primary_frame)
    }
}
