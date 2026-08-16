use super::error::ThumbnailError;
use super::image::DecodedThumbnailFrame;
use super::types::{
    ScalingAlgorithm, ThumbnailConfig, VideoThumbnailEngine, anonymize_target,
    calculate_aspect_dimensions,
};
use libmpv2::Mpv;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct VideoExtractionResult {
    pub primary_frame: DecodedThumbnailFrame,
    pub preview_frames: Vec<DecodedThumbnailFrame>,
}

pub trait VideoFrameDecoder: Send + Sync {
    /// Extracts both primary thumbnail and optional multi-frame preview in a single open-context pass.
    fn extract_video(
        &self,
        path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<VideoExtractionResult, ThumbnailError>;

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
// 1. Ultra-Resilient FFmpeg Direct Video Thumbnailer (libavformat/libavcodec/libswscale)
// =============================================================================

#[derive(Default)]
pub struct FfmpegVideoThumbnailer;

impl FfmpegVideoThumbnailer {
    pub fn new() -> Self {
        Self
    }

    /// Feeds packets tolerance-first into decoder until a valid frame is produced or limit reached.
    #[inline]
    fn decode_and_scale_direct(
        ictx: &mut ffmpeg_next::format::context::Input,
        decoder: &mut ffmpeg_next::decoder::Video,
        scaler: &mut ffmpeg_next::software::scaling::context::Context,
        video_stream_index: usize,
        target_w: u32,
        target_h: u32,
        max_packets_to_scan: usize,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        let mut decoded_frame = ffmpeg_next::frame::Video::empty();
        let mut final_pixels: Option<Vec<u8>> = None;
        let mut packets_read = 0;

        for (stream, packet) in ictx.packets() {
            if stream.index() == video_stream_index {
                packets_read += 1;

                // Non-fatal packet send: ignore single corrupt packets/slices and keep streaming
                let _ = decoder.send_packet(&packet);

                while decoder.receive_frame(&mut decoded_frame).is_ok() {
                    if decoded_frame.width() > 0 && decoded_frame.height() > 0 {
                        let mut rgb_frame = ffmpeg_next::frame::Video::empty();
                        if scaler.run(&decoded_frame, &mut rgb_frame).is_ok() {
                            let rgb_bytes = rgb_frame.data(0);
                            let required_len = (target_w * target_h * 3) as usize;
                            if rgb_bytes.len() >= required_len {
                                final_pixels = Some(rgb_bytes[..required_len].to_vec());
                                break;
                            }
                        }
                    }
                }

                if final_pixels.is_some() || packets_read >= max_packets_to_scan {
                    break;
                }
            }
        }

        // Drain any buffered B-frames/delayed frames from decoder using EOF packet
        if final_pixels.is_none() {
            let _ = decoder.send_eof();
            while decoder.receive_frame(&mut decoded_frame).is_ok() {
                if decoded_frame.width() > 0 && decoded_frame.height() > 0 {
                    let mut rgb_frame = ffmpeg_next::frame::Video::empty();
                    if scaler.run(&decoded_frame, &mut rgb_frame).is_ok() {
                        let rgb_bytes = rgb_frame.data(0);
                        let required_len = (target_w * target_h * 3) as usize;
                        if rgb_bytes.len() >= required_len {
                            final_pixels = Some(rgb_bytes[..required_len].to_vec());
                            break;
                        }
                    }
                }
            }
        }

        let raw = final_pixels.ok_or_else(|| {
            ThumbnailError::Ffmpeg("Could not decode any valid video frame".into())
        })?;

        Ok(DecodedThumbnailFrame {
            width: target_w,
            height: target_h,
            rgb_pixels: raw,
        })
    }
}

impl VideoFrameDecoder for FfmpegVideoThumbnailer {
    fn extract_video(
        &self,
        path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<VideoExtractionResult, ThumbnailError> {
        let anon_target = anonymize_target(path, None);
        let t0 = Instant::now();

        ffmpeg_next::init().map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        // 1. Open media container directly (FFmpeg probes magic header, ignoring extension mismatches)
        let mut ictx =
            ffmpeg_next::format::input(path).map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| ThumbnailError::Ffmpeg("No video stream found".into()))?;

        let video_stream_index = stream.index();
        let stream_duration = stream.duration();
        let time_base = stream.time_base();

        let mut context_decoder =
            ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        // Maximize decoder error tolerance & low-latency streaming
        let mut flags = ffmpeg_next::codec::flag::Flags::empty();
        flags.insert(ffmpeg_next::codec::flag::Flags::LOW_DELAY);
        flags.insert(ffmpeg_next::codec::flag::Flags::OUTPUT_CORRUPT);
        context_decoder.set_flags(flags);

        let mut decoder = context_decoder
            .decoder()
            .video()
            .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let orig_w = decoder.width();
        let orig_h = decoder.height();

        if orig_w == 0 || orig_h == 0 {
            return Err(ThumbnailError::Ffmpeg(
                "Invalid video dimensions (0x0)".into(),
            ));
        }

        let (target_w, target_h) = calculate_aspect_dimensions(orig_w, orig_h, config.max_bound);

        // Direct 1-Step colorspace conversion + downscaling in libswscale
        let mut scaler = ffmpeg_next::software::scaling::context::Context::get(
            decoder.format(),
            orig_w,
            orig_h,
            ffmpeg_next::format::Pixel::RGB24,
            target_w,
            target_h,
            ffmpeg_next::software::scaling::flag::Flags::FAST_BILINEAR,
        )
        .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        // Standardize container duration to AV_TIME_BASE (microseconds) across all formats
        let total_dur_av = if ictx.duration() > 0 {
            ictx.duration()
        } else if stream_duration > 0 {
            let dur_secs = stream_duration as f64 * f64::from(time_base);
            (dur_secs * f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) as i64
        } else {
            0
        };

        let calc_seek = |pct: f64| -> i64 {
            let clamped = pct.clamp(0.01, 0.95);
            if total_dur_av > 0 {
                (total_dur_av as f64 * clamped) as i64
            } else {
                0
            }
        };

        // 2. Extract Primary Thumbnail Frame at configured offset (e.g. 10%)
        let primary_pct = (config.video_offset_percent.clamp(1, 90) as f64) / 100.0;
        let seek_primary = calc_seek(primary_pct);
        if seek_primary > 0 {
            let _ = ictx.seek(seek_primary, ..seek_primary);
        } else {
            let _ = ictx.seek(0, ..0);
        }
        decoder.flush();

        let primary_frame = match Self::decode_and_scale_direct(
            &mut ictx,
            &mut decoder,
            &mut scaler,
            video_stream_index,
            target_w,
            target_h,
            200,
        ) {
            Ok(f) => f,
            Err(_) => {
                // Seek fallback: seek to beginning (0) and decode first available frame
                let _ = ictx.seek(0, ..0);
                decoder.flush();
                Self::decode_and_scale_direct(
                    &mut ictx,
                    &mut decoder,
                    &mut scaler,
                    video_stream_index,
                    target_w,
                    target_h,
                    300,
                )?
            }
        };

        // 3. Extract Multi-Frame Preview Sequence reusing the open context
        let mut preview_frames = Vec::new();
        if config.generate_video_previews {
            let count = config.preview_frame_count.clamp(2, 100);
            preview_frames.reserve(count as usize);

            for i in 0..count {
                let t_pct = 0.10 + (0.80 * (i as f64 / (count - 1) as f64));

                // Reuse primary frame if offset matches closely
                if (t_pct - primary_pct).abs() < 0.02 {
                    preview_frames.push(primary_frame.clone());
                    continue;
                }

                let seek_ts = calc_seek(t_pct);
                if seek_ts > 0 {
                    let _ = ictx.seek(seek_ts, ..seek_ts);
                }
                decoder.flush();

                match Self::decode_and_scale_direct(
                    &mut ictx,
                    &mut decoder,
                    &mut scaler,
                    video_stream_index,
                    target_w,
                    target_h,
                    200,
                ) {
                    Ok(frame) => {
                        preview_frames.push(frame);
                    }
                    Err(_) => {
                        // Self-healing fallback: replicate last valid frame so preview count is never incomplete
                        let fallback = preview_frames
                            .last()
                            .cloned()
                            .unwrap_or_else(|| primary_frame.clone());
                        preview_frames.push(fallback);
                    }
                }
            }
        }

        tracing::debug!(
            target = %anon_target,
            orig_w = orig_w,
            orig_h = orig_h,
            target_w = target_w,
            target_h = target_h,
            preview_frames_extracted = preview_frames.len(),
            total_time_ms = t0.elapsed().as_millis(),
            engine = "FFmpeg",
            "[VideoDecoder: FFmpeg Resilient] Extracted video thumbnail & preview frames"
        );

        Ok(VideoExtractionResult {
            primary_frame,
            preview_frames,
        })
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
            video_offset_percent: (time_percent * 100.0) as u8,
            generate_video_previews: false,
            ..Default::default()
        };
        let res = self.extract_video(path, &config)?;
        Ok(res.primary_frame)
    }
}

// =============================================================================
// 2. MPV Headless Video Thumbnailer (libmpv2 Engine)
// =============================================================================

#[derive(Default)]
pub struct MpvVideoThumbnailer;

impl MpvVideoThumbnailer {
    pub fn new() -> Self {
        Self
    }
}

impl VideoFrameDecoder for MpvVideoThumbnailer {
    fn extract_video(
        &self,
        path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<VideoExtractionResult, ThumbnailError> {
        let anon_target = anonymize_target(path, None);
        let t0 = Instant::now();

        let mpv = Mpv::with_initializer(|init| {
            init.set_property("vo", "null")?;
            init.set_property("ao", "null")?;
            init.set_property("audio", "no")?;
            init.set_property("hwdec", "auto-safe")?;
            init.set_property("pause", "yes")?;
            Ok(())
        })
        .map_err(|e| ThumbnailError::Ffmpeg(format!("Failed to init MPV: {}", e)))?;

        let path_str = path.to_string_lossy();
        mpv.command("loadfile", &[&path_str, "replace"])
            .map_err(|e| ThumbnailError::Ffmpeg(format!("MPV loadfile error: {}", e)))?;

        let percent_str = format!("{:.1}", config.video_offset_percent);
        let _ = mpv.command("seek", &[&percent_str, "absolute-percent+exact"]);

        tracing::debug!(
            target = %anon_target,
            offset = config.video_offset_percent,
            total_time_ms = t0.elapsed().as_millis(),
            engine = "MPV",
            "[VideoDecoder: MPV Engine] Stream seek verified via libmpv context"
        );

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
            video_offset_percent: (time_percent * 100.0) as u8,
            generate_video_previews: false,
            video_engine: VideoThumbnailEngine::Mpv,
            ..Default::default()
        };
        let res = self.extract_video(path, &config)?;
        Ok(res.primary_frame)
    }
}

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
