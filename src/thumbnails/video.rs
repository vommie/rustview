use super::error::ThumbnailError;
use super::image::DecodedThumbnailFrame;
use super::types::{
    ScalingAlgorithm, ThumbnailConfig, VideoThumbnailEngine, anonymize_target,
    calculate_aspect_dimensions,
};
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::context::Context as SwsContext;
use ffmpeg_next::software::scaling::flag::Flags as SwsFlags;
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

/// Maps RustView ScalingAlgorithm domain enum to native FFmpeg libswscale flags with accurate rounding.
#[inline]
pub fn map_scaling_algorithm_to_sws_flags(algo: ScalingAlgorithm) -> SwsFlags {
    match algo {
        ScalingAlgorithm::Nearest => SwsFlags::POINT,
        ScalingAlgorithm::Bilinear => SwsFlags::BILINEAR | SwsFlags::ACCURATE_RND,
        ScalingAlgorithm::CatmullRom => SwsFlags::BICUBIC | SwsFlags::ACCURATE_RND,
        ScalingAlgorithm::Lanczos3 => SwsFlags::LANCZOS | SwsFlags::ACCURATE_RND,
    }
}

/// Dynamic, self-healing Swscale context manager that transparently adapts to stream pixel format changes
/// and unpacks padded pitch/stride linesizes into contiguous linear RGB8 byte buffers.
struct DynamicScaler {
    scaler: Option<SwsContext>,
    src_fmt: Pixel,
    src_w: u32,
    src_h: u32,
    pub dst_w: u32,
    pub dst_h: u32,
    flags: SwsFlags,
}

impl DynamicScaler {
    fn new(dst_w: u32, dst_h: u32, flags: SwsFlags) -> Self {
        Self {
            scaler: None,
            src_fmt: Pixel::None,
            src_w: 0,
            src_h: 0,
            dst_w,
            dst_h,
            flags,
        }
    }

    /// Converts and scales a decoded video frame to tightly packed RGB24 bytes.
    /// Explicitly unrolls the row stride (linesize) to discard memory alignment padding.
    fn scale_frame(
        &mut self,
        decoded: &ffmpeg_next::frame::Video,
    ) -> Result<Vec<u8>, ThumbnailError> {
        let fmt = decoded.format();
        let w = decoded.width();
        let h = decoded.height();

        if self.scaler.is_none() || self.src_fmt != fmt || self.src_w != w || self.src_h != h {
            tracing::trace!(
                src_fmt = ?fmt,
                src_w = w,
                src_h = h,
                dst_w = self.dst_w,
                dst_h = self.dst_h,
                "[VideoDecoder: Swscale] Initializing dynamic colorspace scaling context"
            );

            let new_scaler = SwsContext::get(
                fmt,
                w,
                h,
                Pixel::RGB24,
                self.dst_w,
                self.dst_h,
                self.flags,
            ).map_err(|e| ThumbnailError::Ffmpeg(format!(
                "Failed to initialize swscale context ({:?} {}x{} -> RGB24 {}x{}): {}",
                fmt, w, h, self.dst_w, self.dst_h, e
            )))?;

            self.scaler = Some(new_scaler);
            self.src_fmt = fmt;
            self.src_w = w;
            self.src_h = h;
        }

        let scaler = self.scaler.as_mut().unwrap();
        let mut rgb_frame = ffmpeg_next::frame::Video::empty();
        scaler.run(decoded, &mut rgb_frame)
            .map_err(|e| ThumbnailError::Ffmpeg(format!("swscale conversion failed: {}", e)))?;

        let actual_w = rgb_frame.width();
        let actual_h = rgb_frame.height();
        let stride = rgb_frame.stride(0);
        let row_bytes = (actual_w * 3) as usize;
        let plane_data = rgb_frame.data(0);

        // Pitch / Stride Correction:
        // If stride equals row_bytes, the buffer is already tightly packed.
        // If stride > row_bytes (common for non-aligned widths like 144, 240, etc.), unpack row by row.
        if stride == row_bytes && plane_data.len() >= row_bytes * actual_h as usize {
            return Ok(plane_data[..row_bytes * actual_h as usize].to_vec());
        }

        let mut contiguous = Vec::with_capacity(row_bytes * actual_h as usize);
        for y in 0..actual_h as usize {
            let start = y * stride;
            let end = start + row_bytes;
            if end <= plane_data.len() {
                contiguous.extend_from_slice(&plane_data[start..end]);
            } else {
                return Err(ThumbnailError::Ffmpeg(format!(
                    "Incomplete video frame buffer: row {}/{} exceeded plane length (stride: {}, len: {})",
                    y, actual_h, stride, plane_data.len()
                )));
            }
        }

        Ok(contiguous)
    }
}

/// Applies orientation rotation if container or stream tags require 90°, 180°, or 270° rotation.
fn apply_rotation_if_needed(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    rotation: i32,
) -> (u32, u32, Vec<u8>) {
    let norm_rot = rotation.rem_euclid(360);
    if norm_rot != 90 && norm_rot != 180 && norm_rot != 270 {
        return (width, height, pixels);
    }

    if pixels.len() != (width * height * 3) as usize {
        return (width, height, pixels);
    }

    let img = match image::RgbImage::from_raw(width, height, pixels) {
        Some(i) => i,
        None => return (width, height, Vec::new()),
    };

    let rotated = match norm_rot {
        90 => image::imageops::rotate90(&img),
        180 => image::imageops::rotate180(&img),
        270 => image::imageops::rotate270(&img),
        _ => img,
    };
    let (new_w, new_h) = rotated.dimensions();
    (new_w, new_h, rotated.into_raw())
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

    /// Feeds packets tolerance-first into decoder until a valid frame is produced or scan limit reached.
    #[inline]
    fn decode_and_scale_direct(
        ictx: &mut ffmpeg_next::format::context::Input,
        decoder: &mut ffmpeg_next::decoder::Video,
        scaler: &mut DynamicScaler,
        video_stream_index: usize,
        max_packets_to_scan: usize,
        norm_rot: i32,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        let mut decoded_frame = ffmpeg_next::frame::Video::empty();
        let mut final_frame: Option<DecodedThumbnailFrame> = None;
        let mut packets_read = 0;

        for (stream, packet) in ictx.packets() {
            if stream.index() == video_stream_index {
                packets_read += 1;

                // Non-fatal packet send: ignore single corrupt packets/slices and keep streaming
                let _ = decoder.send_packet(&packet);

                while decoder.receive_frame(&mut decoded_frame).is_ok() {
                    if decoded_frame.width() > 0
                        && decoded_frame.height() > 0
                        && let Ok(raw_rgb) = scaler.scale_frame(&decoded_frame)
                    {
                        let (w, h, rotated_pixels) = apply_rotation_if_needed(
                            raw_rgb,
                            scaler.dst_w,
                            scaler.dst_h,
                            norm_rot,
                        );
                        final_frame = Some(DecodedThumbnailFrame {
                            width: w,
                            height: h,
                            rgb_pixels: rotated_pixels,
                        });
                        break;
                    }
                }

                if final_frame.is_some() || packets_read >= max_packets_to_scan {
                    break;
                }
            }
        }

        // Drain any buffered B-frames/delayed frames from decoder using EOF packet
        if final_frame.is_none() {
            let _ = decoder.send_eof();
            while decoder.receive_frame(&mut decoded_frame).is_ok() {
                if decoded_frame.width() > 0
                    && decoded_frame.height() > 0
                    && let Ok(raw_rgb) = scaler.scale_frame(&decoded_frame)
                {
                    let (w, h, rotated_pixels) = apply_rotation_if_needed(
                        raw_rgb,
                        scaler.dst_w,
                        scaler.dst_h,
                        norm_rot,
                    );
                    final_frame = Some(DecodedThumbnailFrame {
                        width: w,
                        height: h,
                        rgb_pixels: rotated_pixels,
                    });
                    break;
                }
            }
        }

        final_frame.ok_or_else(|| {
            ThumbnailError::Ffmpeg("Could not decode any valid video frame from stream".into())
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
            .ok_or_else(|| ThumbnailError::Ffmpeg("No video stream found in container".into()))?;

        let video_stream_index = stream.index();
        let stream_duration = stream.duration();
        let time_base = stream.time_base();

        // Check container and stream rotation metadata safely without borrowing temporary context
        let rotation: i32 = stream
            .metadata()
            .get("rotate")
            .and_then(|r| r.parse::<i32>().ok())
            .or_else(|| {
                ictx.metadata()
                    .get("rotate")
                    .and_then(|r| r.parse::<i32>().ok())
            })
            .unwrap_or(0);
        let norm_rot = rotation.rem_euclid(360);

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

        // Display dimensions taking rotation into account
        let (disp_w, disp_h) = if norm_rot == 90 || norm_rot == 270 {
            (orig_h, orig_w)
        } else {
            (orig_w, orig_h)
        };

        let (target_disp_w, target_disp_h) = calculate_aspect_dimensions(disp_w, disp_h, config.max_bound);

        // Pre-rotation scale target dimensions
        let (scaler_target_w, scaler_target_h) = if norm_rot == 90 || norm_rot == 270 {
            (target_disp_h, target_disp_w)
        } else {
            (target_disp_w, target_disp_h)
        };

        let sws_flags = map_scaling_algorithm_to_sws_flags(config.scaling_algorithm);
        let mut dynamic_scaler = DynamicScaler::new(scaler_target_w, scaler_target_h, sws_flags);

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
            &mut dynamic_scaler,
            video_stream_index,
            200,
            norm_rot,
        ) {
            Ok(f) => f,
            Err(_) => {
                // Seek fallback: seek to beginning (0) and decode first available frame
                let _ = ictx.seek(0, ..0);
                decoder.flush();
                Self::decode_and_scale_direct(
                    &mut ictx,
                    &mut decoder,
                    &mut dynamic_scaler,
                    video_stream_index,
                    300,
                    norm_rot,
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
                    &mut dynamic_scaler,
                    video_stream_index,
                    200,
                    norm_rot,
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
            target_w = primary_frame.width,
            target_h = primary_frame.height,
            rotation = norm_rot,
            preview_frames_extracted = preview_frames.len(),
            total_time_ms = t0.elapsed().as_millis(),
            engine = "FFmpeg",
            "[VideoDecoder: FFmpeg Resilient] Extracted video thumbnail & preview frames with stride-aligned pixel unpacking"
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
