use super::error::ThumbnailError;
use super::types::{calculate_aspect_dimensions, ScalingAlgorithm};
use super::video::{FfmpegVideoThumbnailer, VideoFrameDecoder};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use image::ImageReader;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

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
        // 1. High-Speed Buffered File Read & Native Image Crate Decoding
        if let Ok(file) = File::open(path) {
            let reader = BufReader::with_capacity(128 * 1024, file);
            if let Ok(reader_guessed) = ImageReader::new(reader).with_guessed_format()
                && let Ok(dynamic_img) = reader_guessed.decode()
            {
                let orig_w = dynamic_img.width();
                let orig_h = dynamic_img.height();
                let (target_w, target_h) = calculate_aspect_dimensions(orig_w, orig_h, max_bound);

                let rgb_src = dynamic_img.into_rgb8();
                let src_raw = rgb_src.into_raw();

                let src_image = Image::from_vec_u8(orig_w, orig_h, src_raw, PixelType::U8x3)?;
                let mut dst_image = Image::new(target_w, target_h, PixelType::U8x3);
                let mut resizer = Resizer::new();
                let options = ResizeOptions::default().resize_alg(scaling_algo.to_resize_alg());

                resizer.resize(&src_image, &mut dst_image, &options)?;
                let resized_raw = dst_image.into_vec();

                return Ok(DecodedThumbnailFrame {
                    width: target_w,
                    height: target_h,
                    rgb_pixels: resized_raw,
                });
            }
        }

        // 2. Resilient Fallback to FFmpeg decoder (for PSD, exotic TIFF or chunk variations)
        let ffmpeg_decoder = FfmpegVideoThumbnailer::new();
        ffmpeg_decoder.extract_frame(path, 0.0, max_bound, scaling_algo)
    }
}
