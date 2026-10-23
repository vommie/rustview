use super::error::ThumbnailError;
use super::types::{ScalingAlgorithm, calculate_aspect_dimensions};
use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use image::AnimationDecoder;
use image::codecs::gif::GifDecoder;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;

/// Individual decoded and scaled frame belonging to an animated GIF.
#[derive(Debug, Clone)]
pub struct AnimatedFrame {
    pub width: u32,
    pub height: u32,
    pub delay_ms: u32,
    pub rgb_pixels: Vec<u8>,
}

/// Container holding all scaled frames and timing metadata of an animated GIF.
#[derive(Debug, Clone)]
pub struct AnimatedPreview {
    pub frames: Vec<AnimatedFrame>,
    pub orig_width: u32,
    pub orig_height: u32,
    pub total_duration_ms: u64,
}

pub struct GifThumbnailer;

impl GifThumbnailer {
    /// Extracts, composites, and scales all frames of an animated GIF, preserving exact frame delays.
    pub fn extract_frames(
        path: &Path,
        max_bound: u32,
        scaling_algo: ScalingAlgorithm,
    ) -> Result<AnimatedPreview, ThumbnailError> {
        let t0 = Instant::now();
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(128 * 1024, file);
        let decoder = GifDecoder::new(reader).map_err(|_e| {
            ThumbnailError::Image(image::ImageError::Decoding(
                image::error::DecodingError::from_format_hint(image::error::ImageFormatHint::Exact(
                    image::ImageFormat::Gif,
                )),
            ))
        })?;

        // collect_frames() in image 0.25 automatically composites GIF disposal methods (RestoreBackground, RestorePrevious, etc.)
        let raw_frames = decoder.into_frames().collect_frames().map_err(ThumbnailError::Image)?;

        if raw_frames.is_empty() {
            return Err(ThumbnailError::Image(image::ImageError::Limits(
                image::error::LimitError::from_kind(image::error::LimitErrorKind::DimensionError),
            )));
        }

        let orig_w = raw_frames[0].buffer().width();
        let orig_h = raw_frames[0].buffer().height();

        if orig_w == 0 || orig_h == 0 {
            return Err(ThumbnailError::Ffmpeg("GIF dimensions are 0x0".into()));
        }

        let (target_w, target_h) = calculate_aspect_dimensions(orig_w, orig_h, max_bound);

        let mut resizer = Resizer::new();
        let options = ResizeOptions::default().resize_alg(scaling_algo.to_resize_alg());

        let mut animated_frames = Vec::with_capacity(raw_frames.len());
        let mut total_duration_ms = 0u64;

        for frame in raw_frames {
            let (num, denom) = frame.delay().numer_denom_ms();
            let raw_delay = if denom > 0 {
                (num as f64 / denom as f64).round() as u32
            } else {
                num
            };

            // Standard Web Browser Compliance: If delay is <= 10ms or 0ms, default to 100ms (10 FPS)
            let delay_ms = if raw_delay <= 10 { 100 } else { raw_delay };
            total_duration_ms += delay_ms as u64;

            let rgba = frame.into_buffer();
            let src_w = rgba.width();
            let src_h = rgba.height();

            // Convert RGBA to high-quality opaque RGB8 with alpha blending against a solid black background
            let mut rgb_raw = Vec::with_capacity((src_w * src_h * 3) as usize);
            for pixel in rgba.pixels() {
                let [r, g, b, a] = pixel.0;
                if a == 255 {
                    rgb_raw.push(r);
                    rgb_raw.push(g);
                    rgb_raw.push(b);
                } else if a == 0 {
                    rgb_raw.push(0);
                    rgb_raw.push(0);
                    rgb_raw.push(0);
                } else {
                    let alpha = a as f32 / 255.0;
                    rgb_raw.push((r as f32 * alpha).round() as u8);
                    rgb_raw.push((g as f32 * alpha).round() as u8);
                    rgb_raw.push((b as f32 * alpha).round() as u8);
                }
            }

            let src_img = FirImage::from_vec_u8(src_w, src_h, rgb_raw, PixelType::U8x3)?;
            let mut dst_img = FirImage::new(target_w, target_h, PixelType::U8x3);
            resizer.resize(&src_img, &mut dst_img, &options)?;

            animated_frames.push(AnimatedFrame {
                width: target_w,
                height: target_h,
                delay_ms,
                rgb_pixels: dst_img.into_vec(),
            });
        }

        tracing::debug!(
            frames_count = animated_frames.len(),
            duration_ms = total_duration_ms,
            elapsed_us = t0.elapsed().as_micros(),
            "[GifThumbnailer] Successfully decoded, composited, and scaled animated GIF frames"
        );

        Ok(AnimatedPreview {
            frames: animated_frames,
            orig_width: orig_w,
            orig_height: orig_h,
            total_duration_ms,
        })
    }
}
