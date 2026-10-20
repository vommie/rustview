use super::error::ThumbnailError;
use super::image::DecodedThumbnailFrame;
use super::types::{
    calculate_aspect_dimensions, ScalingAlgorithm, ThumbnailConfig, VideoThumbnailEngine,
};
use crate::media::metadata::ensure_ffmpeg_initialized;
use crate::settings::LogObscurer;
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
    pub orig_width: u32,
    pub orig_height: u32,
    pub duration_ms: u64,
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
pub struct DynamicScaler {
    scaler: Option<SwsContext>,
    src_fmt: Pixel,
    src_w: u32,
    src_h: u32,
    pub dst_w: u32,
    pub dst_h: u32,
    flags: SwsFlags,
}

impl DynamicScaler {
    pub fn new(dst_w: u32, dst_h: u32, flags: SwsFlags) -> Self {
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

    /// Converts and scales a decoded video frame to tightly packed RGB24 bytes via FFmpeg's native libswscale.
    pub fn scale_frame(
        &mut self,
        decoded: &ffmpeg_next::frame::Video,
    ) -> Result<Vec<u8>, ThumbnailError> {
        let fmt = decoded.format();
        let w = decoded.width();
        let h = decoded.height();

        if self.scaler.is_none() || self.src_fmt != fmt || self.src_w != w || self.src_h != h {
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
pub fn apply_rotation_if_needed(
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

/// Context and stream metadata bundled for decoding video frames.
#[derive(Debug, Clone, Copy)]
pub struct VideoStreamContext {
    pub video_stream_index: usize,
    pub time_base: ffmpeg_next::Rational,
    pub fps: f64,
    pub norm_rot: i32,
    pub orig_disp_w: u32,
    pub orig_disp_h: u32,
}

/// Parameters for seek-and-decode forward frame extraction across the timeline.
pub struct SeekForwardExtractionParams<'a> {
    pub ictx: ffmpeg_next::format::context::Input,
    pub decoder: ffmpeg_next::decoder::Video,
    pub scaler: DynamicScaler,
    pub stream_ctx: &'a VideoStreamContext,
    pub duration_secs: f64,
    pub target_preview_count: usize,
    pub primary_offset_pct: f64,
    pub config: &'a ThumbnailConfig,
}

/// Parameters for sequential timeline frame extraction.
pub struct SequentialExtractionParams<'a> {
    pub ictx: ffmpeg_next::format::context::Input,
    pub decoder: ffmpeg_next::decoder::Video,
    pub scaler: DynamicScaler,
    pub stream_ctx: &'a VideoStreamContext,
    pub duration_secs: Option<f64>,
    pub target_preview_count: usize,
    pub primary_offset_pct: f64,
}

// =============================================================================
// 1. Ultra-Fast & Resilient FFmpeg Direct Video Thumbnailer
// =============================================================================

#[derive(Default)]
pub struct FfmpegVideoThumbnailer;

/// Longest edge `extract_native_frame` returns; a larger picture is scaled down to it.
const MAX_NATIVE_FRAME_EDGE: u32 = 16384;

impl FfmpegVideoThumbnailer {
    pub fn new() -> Self {
        Self
    }

    /// The first frame at the size the picture has (no scaling up like a thumbnail): what the viewer shows for the
    /// image formats only FFmpeg decodes.
    pub fn extract_native_frame(&self, path: &Path) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        let (width, height, _) = crate::media::MetadataExtractor::probe_video_details(path)
            .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;
        let edge = width.max(height).clamp(16, MAX_NATIVE_FRAME_EDGE);
        self.extract_frame(path, 0.0, edge, ScalingAlgorithm::Nearest)
    }

    /// Chronological Forward-Seek Extraction (D > 30.0s).
    fn extract_seek_forward(
        &self,
        params: SeekForwardExtractionParams<'_>,
    ) -> Result<VideoExtractionResult, ThumbnailError> {
        let mut ictx = params.ictx;
        let mut decoder = params.decoder;
        let mut scaler = params.scaler;
        let stream_ctx = params.stream_ctx;
        let duration_secs = params.duration_secs;
        let num_previews = params.target_preview_count.clamp(2, 100);
        let primary_offset_pct = params.primary_offset_pct;

        let calc_seek_ts = |sec: f64| -> i64 {
            (sec * f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) as i64
        };

        // 1. Extract Primary Thumbnail Frame
        let prim_sec = (duration_secs * primary_offset_pct).clamp(0.0, duration_secs);
        let primary_frame = Self::seek_and_decode_to_target(
            &mut ictx,
            &mut decoder,
            &mut scaler,
            stream_ctx,
            calc_seek_ts(prim_sec),
            prim_sec,
        )?;

        // 2. Extract Preview Frames across 10% to 90% in forward chronological order
        let mut preview_frames = Vec::with_capacity(num_previews);

        if params.config.generate_video_previews {
            let start_pct = 0.10;
            let end_pct = 0.90;

            for k in 0..num_previews {
                let pct = if num_previews > 1 {
                    start_pct + (end_pct - start_pct) * (k as f64 / (num_previews - 1) as f64)
                } else {
                    prim_sec
                };
                let target_sec = duration_secs * pct;

                if (pct - primary_offset_pct).abs() < 0.03 {
                    preview_frames.push(primary_frame.clone());
                    continue;
                }

                match Self::seek_and_decode_to_target(
                    &mut ictx,
                    &mut decoder,
                    &mut scaler,
                    stream_ctx,
                    calc_seek_ts(target_sec),
                    target_sec,
                ) {
                    Ok(frame) => {
                        preview_frames.push(frame);
                    }
                    Err(_) => {
                        let fallback = preview_frames
                            .last()
                            .cloned()
                            .unwrap_or_else(|| primary_frame.clone());
                        preview_frames.push(fallback);
                    }
                }
            }
        }

        let duration_ms = (duration_secs * 1000.0).round() as u64;

        Ok(VideoExtractionResult {
            primary_frame,
            preview_frames,
            orig_width: stream_ctx.orig_disp_w,
            orig_height: stream_ctx.orig_disp_h,
            duration_ms,
        })
    }

    #[inline]
    fn seek_and_decode_to_target(
        ictx: &mut ffmpeg_next::format::context::Input,
        decoder: &mut ffmpeg_next::decoder::Video,
        scaler: &mut DynamicScaler,
        stream_ctx: &VideoStreamContext,
        seek_ts: i64,
        target_pts_sec: f64,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        if seek_ts > 0 {
            let _ = ictx.seek(seek_ts, ..seek_ts);
        } else {
            let _ = ictx.seek(0, ..0);
        }
        decoder.flush();

        let mut decoded_frame = ffmpeg_next::frame::Video::empty();
        let mut packets_scanned = 0;
        let mut frame_idx = 0usize;
        let mut latest_frame_unscaled: Option<ffmpeg_next::frame::Video> = None;

        for (stream, packet) in ictx.packets() {
            if stream.index() == stream_ctx.video_stream_index {
                packets_scanned += 1;
                let _ = decoder.send_packet(&packet);

                while decoder.receive_frame(&mut decoded_frame).is_ok() {
                    if decoded_frame.width() > 0 && decoded_frame.height() > 0 {
                        frame_idx += 1;
                        let pts_sec = decoded_frame
                            .pts()
                            .map(|pts| pts as f64 * f64::from(stream_ctx.time_base))
                            .unwrap_or(frame_idx as f64 / stream_ctx.fps);

                        if pts_sec >= target_pts_sec - 0.02 {
                            let raw_rgb = scaler.scale_frame(&decoded_frame)?;
                            let (w, h, rot_pixels) = apply_rotation_if_needed(
                                raw_rgb,
                                scaler.dst_w,
                                scaler.dst_h,
                                stream_ctx.norm_rot,
                            );
                            return Ok(DecodedThumbnailFrame {
                                width: w,
                                height: h,
                                rgb_pixels: rot_pixels,
                                orig_width: stream_ctx.orig_disp_w,
                                orig_height: stream_ctx.orig_disp_h,
                            });
                        }

                        latest_frame_unscaled = Some(decoded_frame.clone());
                    }
                }

                if packets_scanned >= 30 {
                    break;
                }
            }
        }

        if let Some(frame) = latest_frame_unscaled {
            let raw_rgb = scaler.scale_frame(&frame)?;
            let (w, h, rot_pixels) = apply_rotation_if_needed(
                raw_rgb,
                scaler.dst_w,
                scaler.dst_h,
                stream_ctx.norm_rot,
            );
            return Ok(DecodedThumbnailFrame {
                width: w,
                height: h,
                rgb_pixels: rot_pixels,
                orig_width: stream_ctx.orig_disp_w,
                orig_height: stream_ctx.orig_disp_h,
            });
        }

        let _ = decoder.send_eof();
        while decoder.receive_frame(&mut decoded_frame).is_ok() {
            if decoded_frame.width() > 0 && decoded_frame.height() > 0 {
                let raw_rgb = scaler.scale_frame(&decoded_frame)?;
                let (w, h, rot_pixels) = apply_rotation_if_needed(
                    raw_rgb,
                    scaler.dst_w,
                    scaler.dst_h,
                    stream_ctx.norm_rot,
                );
                return Ok(DecodedThumbnailFrame {
                    width: w,
                    height: h,
                    rgb_pixels: rot_pixels,
                    orig_width: stream_ctx.orig_disp_w,
                    orig_height: stream_ctx.orig_disp_h,
                });
            }
        }

        Err(ThumbnailError::Ffmpeg("Failed to decode video frame at target timestamp".into()))
    }

    /// Single Linear Forward Sweep for videos <= 30.0s (or unknown duration).
    /// Features hard packet/frame limits to prevent hangs on truncated or in-flight renders.
    fn extract_targeted_sequential(
        &self,
        params: SequentialExtractionParams<'_>,
    ) -> Result<VideoExtractionResult, ThumbnailError> {
        let mut ictx = params.ictx;
        let mut decoder = params.decoder;
        let mut scaler = params.scaler;
        let stream_ctx = params.stream_ctx;
        let duration_secs = params.duration_secs;
        let num_previews = params.target_preview_count.clamp(2, 100);
        let primary_offset_pct = params.primary_offset_pct;

        // A stream without a known duration (a raw H.264 file, a recording in progress) is read from where it is: a seek
        // on it can lose the stream.
        if duration_secs.is_some() {
            let _ = ictx.seek(0, ..0);
            decoder.flush();
        }

        let (target_primary_sec, target_preview_secs) = if let Some(dur) = duration_secs {
            let prim_sec = (dur * primary_offset_pct).clamp(0.0, dur);
            let start_pct = if dur < 2.0 { 0.05 } else { 0.10 };
            let end_pct = if dur < 2.0 { 0.95 } else { 0.90 };

            let mut prev_secs = Vec::with_capacity(num_previews);
            for k in 0..num_previews {
                let pct = if num_previews > 1 {
                    start_pct + (end_pct - start_pct) * (k as f64 / (num_previews - 1) as f64)
                } else {
                    prim_sec
                };
                prev_secs.push((dur * pct).clamp(0.0, dur));
            }
            (prim_sec, Some(prev_secs))
        } else {
            (0.0, None)
        };

        let mut primary_frame: Option<DecodedThumbnailFrame> = None;
        let mut preview_frames: Vec<Option<DecodedThumbnailFrame>> = vec![None; num_previews];
        let mut first_decoded_frame: Option<DecodedThumbnailFrame> = None;
        let mut all_scaled_reservoir: Vec<DecodedThumbnailFrame> = Vec::with_capacity(32);
        let mut decoded_video_frame = ffmpeg_next::frame::Video::empty();
        let mut frame_idx = 0usize;
        let mut packets_scanned = 0usize;

        for (stream, packet) in ictx.packets() {
            if stream.index() != stream_ctx.video_stream_index {
                continue;
            }
            packets_scanned += 1;
            let _ = decoder.send_packet(&packet);

            while decoder.receive_frame(&mut decoded_video_frame).is_ok() {
                if decoded_video_frame.width() == 0 || decoded_video_frame.height() == 0 {
                    continue;
                }

                let pts_sec = decoded_video_frame
                    .pts()
                    .map(|pts| pts as f64 * f64::from(stream_ctx.time_base))
                    .unwrap_or(frame_idx as f64 / stream_ctx.fps);
                frame_idx += 1;

                let is_primary_hit = if target_preview_secs.is_some() {
                    primary_frame.is_none() && (pts_sec >= target_primary_sec || (primary_offset_pct == 0.0 && frame_idx == 1))
                } else {
                    primary_frame.is_none()
                };

                let matching_preview_slots: Vec<usize> = if let Some(ref prev_targets) = target_preview_secs {
                    (0..num_previews)
                        .filter(|&k| preview_frames[k].is_none() && pts_sec >= prev_targets[k])
                        .collect()
                } else {
                    Vec::new()
                };

                if (is_primary_hit || !matching_preview_slots.is_empty() || (target_preview_secs.is_none() && all_scaled_reservoir.len() < num_previews) || first_decoded_frame.is_none())
                    && let Ok(raw_rgb) = scaler.scale_frame(&decoded_video_frame)
                {
                    let (w, h, rot_pixels) = apply_rotation_if_needed(
                        raw_rgb,
                        scaler.dst_w,
                        scaler.dst_h,
                        stream_ctx.norm_rot,
                    );
                    let frame = DecodedThumbnailFrame {
                        width: w,
                        height: h,
                        rgb_pixels: rot_pixels,
                        orig_width: stream_ctx.orig_disp_w,
                        orig_height: stream_ctx.orig_disp_h,
                    };

                    if first_decoded_frame.is_none() {
                        first_decoded_frame = Some(frame.clone());
                    }

                    if is_primary_hit && primary_frame.is_none() {
                        primary_frame = Some(frame.clone());
                    }

                    for slot in matching_preview_slots {
                        preview_frames[slot] = Some(frame.clone());
                    }

                    if target_preview_secs.is_none() && all_scaled_reservoir.len() < num_previews {
                        all_scaled_reservoir.push(frame);
                    }
                }

                // Circuit breaker: If we have all required frames, stop immediately!
                let has_all_previews = if target_preview_secs.is_some() {
                    preview_frames.iter().all(|f| f.is_some())
                } else {
                    all_scaled_reservoir.len() >= num_previews
                };

                if primary_frame.is_some() && has_all_previews {
                    break;
                }
            }

            // Circuit breaker: stop packet decoding loop as soon as frames are gathered
            let has_all_previews = if target_preview_secs.is_some() {
                preview_frames.iter().all(|f| f.is_some())
            } else {
                all_scaled_reservoir.len() >= num_previews
            };

            if primary_frame.is_some() && has_all_previews {
                break;
            }

            // Hard safety bailout: Never read indefinitely across incomplete/corrupt stream
            if frame_idx >= 150 || packets_scanned >= 300 {
                tracing::debug!(
                    frame_idx,
                    packets_scanned,
                    "[VideoThumbnailer: CircuitBreaker] Reached maximum sequential scan limit, concluding extraction"
                );
                break;
            }
        }

        // Flush decoder EOF if primary frame still missing
        if primary_frame.is_none() || preview_frames.iter().any(|f| f.is_none()) {
            let _ = decoder.send_eof();
            while decoder.receive_frame(&mut decoded_video_frame).is_ok() {
                if decoded_video_frame.width() > 0 && decoded_video_frame.height() > 0
                    && let Ok(raw_rgb) = scaler.scale_frame(&decoded_video_frame)
                {
                    let (w, h, rot_pixels) = apply_rotation_if_needed(
                        raw_rgb,
                        scaler.dst_w,
                        scaler.dst_h,
                        stream_ctx.norm_rot,
                    );
                    let frame = DecodedThumbnailFrame {
                        width: w,
                        height: h,
                        rgb_pixels: rot_pixels,
                        orig_width: stream_ctx.orig_disp_w,
                        orig_height: stream_ctx.orig_disp_h,
                    };
                    if first_decoded_frame.is_none() {
                        first_decoded_frame = Some(frame.clone());
                    }
                    if primary_frame.is_none() {
                        primary_frame = Some(frame.clone());
                    }
                    for slot in preview_frames.iter_mut() {
                        if slot.is_none() {
                            *slot = Some(frame.clone());
                        }
                    }
                }
            }
        }

        let fallback = primary_frame
            .or(first_decoded_frame)
            .ok_or_else(|| ThumbnailError::Ffmpeg("No video frames could be decoded".into()))?;

        let final_primary = fallback.clone();
        let mut final_previews = Vec::with_capacity(num_previews);

        if target_preview_secs.is_some() {
            for opt in preview_frames {
                final_previews.push(opt.unwrap_or_else(|| fallback.clone()));
            }
        } else if !all_scaled_reservoir.is_empty() {
            let total = all_scaled_reservoir.len();
            for k in 0..num_previews {
                let idx = if num_previews > 1 {
                    let start_idx = 0;
                    let end_idx = total.saturating_sub(1);
                    start_idx + (((end_idx - start_idx) as f64) * (k as f64 / (num_previews - 1) as f64)).round() as usize
                } else {
                    0
                };
                let safe_idx = idx.min(total - 1);
                final_previews.push(all_scaled_reservoir[safe_idx].clone());
            }
        } else {
            for _ in 0..num_previews {
                final_previews.push(fallback.clone());
            }
        }

        let dur_ms = duration_secs.map(|d| (d * 1000.0).round() as u64).unwrap_or(0);

        Ok(VideoExtractionResult {
            primary_frame: final_primary,
            preview_frames: final_previews,
            orig_width: stream_ctx.orig_disp_w,
            orig_height: stream_ctx.orig_disp_h,
            duration_ms: dur_ms,
        })
    }
}

impl VideoFrameDecoder for FfmpegVideoThumbnailer {
    fn extract_video(
        &self,
        path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<VideoExtractionResult, ThumbnailError> {
        let t0 = Instant::now();
        ensure_ffmpeg_initialized();

        let ictx = ffmpeg_next::format::input(path).map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| ThumbnailError::Ffmpeg("No video stream found in container".into()))?;

        let video_stream_index = stream.index();
        let stream_duration = stream.duration();
        let time_base = stream.time_base();

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

        let mut flags = ffmpeg_next::codec::flag::Flags::empty();
        flags.insert(ffmpeg_next::codec::flag::Flags::LOW_DELAY);
        flags.insert(ffmpeg_next::codec::flag::Flags::OUTPUT_CORRUPT);
        context_decoder.set_flags(flags);

        let decoder = context_decoder
            .decoder()
            .video()
            .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let orig_w = decoder.width();
        let orig_h = decoder.height();

        if orig_w == 0 || orig_h == 0 {
            return Err(ThumbnailError::Ffmpeg("Invalid video dimensions (0x0)".into()));
        }

        let (disp_w, disp_h) = if norm_rot == 90 || norm_rot == 270 {
            (orig_h, orig_w)
        } else {
            (orig_w, orig_h)
        };

        let (target_disp_w, target_disp_h) = calculate_aspect_dimensions(disp_w, disp_h, config.max_bound);
        let (scaler_target_w, scaler_target_h) = if norm_rot == 90 || norm_rot == 270 {
            (target_disp_h, target_disp_w)
        } else {
            (target_disp_w, target_disp_h)
        };

        let sws_flags = map_scaling_algorithm_to_sws_flags(config.scaling_algorithm);
        let dynamic_scaler = DynamicScaler::new(scaler_target_w, scaler_target_h, sws_flags);

        // Discover duration in seconds
        let mut duration_secs: Option<f64> = None;

        if ictx.duration() > 0 {
            let secs = ictx.duration() as f64 / f64::from(ffmpeg_next::ffi::AV_TIME_BASE);
            if secs > 0.05 {
                duration_secs = Some(secs);
            }
        }

        if duration_secs.is_none() && stream_duration > 0 {
            let secs = stream_duration as f64 * f64::from(time_base);
            if secs > 0.05 {
                duration_secs = Some(secs);
            }
        }

        let fps = {
            let r = stream.avg_frame_rate();
            let val = f64::from(r);
            if val > 0.1 && val.is_finite() {
                val
            } else {
                let r2 = stream.rate();
                let val2 = f64::from(r2);
                if val2 > 0.1 && val2.is_finite() {
                    val2
                } else {
                    25.0
                }
            }
        };

        if duration_secs.is_none() && stream.frames() > 0 {
            let secs = stream.frames() as f64 / fps;
            if secs > 0.05 {
                duration_secs = Some(secs);
            }
        }

        // Support full 1..=99% position extraction for dynamic random position selection
        let primary_offset_pct = (config.video_offset_percent.clamp(1, 99) as f64) / 100.0;
        let preview_count = config.preview_frame_count.clamp(2, 100) as usize;

        let stream_ctx = VideoStreamContext {
            video_stream_index,
            time_base,
            fps,
            norm_rot,
            orig_disp_w: disp_w,
            orig_disp_h: disp_h,
        };

        let threshold = (config.short_video_threshold_secs as f64).max(30.0);
        let is_short_or_unknown = match duration_secs {
            Some(d) => d <= threshold,
            None => true,
        };

        let result = if is_short_or_unknown {
            tracing::debug!(
                file = %LogObscurer::path(path),
                duration = ?duration_secs,
                threshold_secs = threshold,
                strategy = "SinglePassForwardSweep",
                target_previews = preview_count,
                primary_offset_pct = primary_offset_pct,
                "[VideoThumbnailer] Executing single forward-pass sweep for video"
            );
            self.extract_targeted_sequential(SequentialExtractionParams {
                ictx,
                decoder,
                scaler: dynamic_scaler,
                stream_ctx: &stream_ctx,
                duration_secs,
                target_preview_count: preview_count,
                primary_offset_pct,
            })
        } else {
            let dur = duration_secs.unwrap();
            tracing::debug!(
                file = %LogObscurer::path(path),
                duration_secs = dur,
                threshold_secs = threshold,
                strategy = "ForwardKeyframeSeekAndDecode",
                target_previews = preview_count,
                primary_offset_pct = primary_offset_pct,
                "[VideoThumbnailer] Executing forward keyframe seeks across video timeline"
            );
            self.extract_seek_forward(SeekForwardExtractionParams {
                ictx,
                decoder,
                scaler: dynamic_scaler,
                stream_ctx: &stream_ctx,
                duration_secs: dur,
                target_preview_count: preview_count,
                primary_offset_pct,
                config,
            })
        };

        match result {
            Ok(res) => {
                tracing::debug!(
                    file = %LogObscurer::path(path),
                    preview_frames_extracted = res.preview_frames.len(),
                    orig_width = res.orig_width,
                    orig_height = res.orig_height,
                    duration_ms = res.duration_ms,
                    primary_offset_pct = primary_offset_pct,
                    elapsed_us = t0.elapsed().as_micros(),
                    "[VideoThumbnailer] Successfully extracted primary and preview frames with metadata"
                );
                Ok(res)
            }
            Err(err) => {
                tracing::warn!(
                    file = %LogObscurer::path(path),
                    error = %err,
                    "[VideoThumbnailer] Video extraction failed"
                );
                Err(err)
            }
        }
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
            video_offset_percent: (time_percent * 100.0).clamp(1.0, 99.0) as u8,
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
            video_offset_percent: (time_percent * 100.0).clamp(1.0, 99.0) as u8,
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
