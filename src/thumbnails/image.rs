use super::error::ThumbnailError;
use super::types::{ScalingAlgorithm, anonymize_target, calculate_aspect_dimensions};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use image::ImageReader;
use memmap2::Mmap;
use std::io::Cursor;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct DecodedThumbnailFrame {
    pub width: u32,
    pub height: u32,
    pub rgb_pixels: Vec<u8>,
}

pub trait ImageDecoder: Send + Sync {
    fn decode_and_resize(
        &self,
        path: &Path,
        max_bound: u32,
        scaling_algo: ScalingAlgorithm,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError>;
}

#[derive(Default)]
pub struct SimdImageThumbnailer;

impl SimdImageThumbnailer {
    pub fn new() -> Self {
        Self
    }
}

impl ImageDecoder for SimdImageThumbnailer {
    fn decode_and_resize(
        &self,
        path: &Path,
        max_bound: u32,
        scaling_algo: ScalingAlgorithm,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        let anon_target = anonymize_target(path, None);
        let t_total = Instant::now();

        // 1. High-Performance Zero-Copy Memory Mapped File Read
        let t_io = Instant::now();
        let file = std::fs::File::open(path)?;
        let mmap_opt = unsafe { Mmap::map(&file).ok() };
        let io_elapsed = t_io.elapsed();

        // 2. Fast Image Decoding from memory slice
        let t_decode = Instant::now();
        let dynamic_img = if let Some(ref mmap) = mmap_opt {
            ImageReader::new(Cursor::new(&mmap[..]))
                .with_guessed_format()?
                .decode()?
        } else {
            let reader = std::io::BufReader::with_capacity(128 * 1024, file);
            ImageReader::new(reader).with_guessed_format()?.decode()?
        };
        let decode_elapsed = t_decode.elapsed();

        let orig_w = dynamic_img.width();
        let orig_h = dynamic_img.height();
        let color_type = dynamic_img.color();

        // 3. Compute exact aspect-ratio target dimensions inside max_bound
        let (target_w, target_h) = calculate_aspect_dimensions(orig_w, orig_h, max_bound);

        // 4. Convert to RGB8 pixel slice
        let rgb_src = dynamic_img.to_rgb8();
        let src_raw = rgb_src.into_raw();

        // 5. SIMD-Accelerated Resizing via fast_image_resize
        let t_resize = Instant::now();
        let src_image = Image::from_vec_u8(orig_w, orig_h, src_raw, PixelType::U8x3)?;
        let mut dst_image = Image::new(target_w, target_h, PixelType::U8x3);
        let mut resizer = Resizer::new();
        let options = ResizeOptions::default().resize_alg(scaling_algo.to_resize_alg());

        resizer.resize(&src_image, &mut dst_image, &options)?;
        let resize_elapsed = t_resize.elapsed();
        let resized_raw = dst_image.into_vec();

        let total_elapsed = t_total.elapsed();

        tracing::debug!(
            target = %anon_target,
            orig_w = orig_w,
            orig_h = orig_h,
            target_w = target_w,
            target_h = target_h,
            color = ?color_type,
            io_time_us = io_elapsed.as_micros(),
            decode_time_ms = decode_elapsed.as_millis(),
            resize_time_ms = resize_elapsed.as_millis(),
            total_time_ms = total_elapsed.as_millis(),
            algorithm = %scaling_algo.as_str(),
            "[ImageDecoder: SIMD FastResize] Successfully decoded and scaled thumbnail frame"
        );

        Ok(DecodedThumbnailFrame {
            width: target_w,
            height: target_h,
            rgb_pixels: resized_raw,
        })
    }
}
