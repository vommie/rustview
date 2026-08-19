use super::error::ThumbnailError;
use super::types::{calculate_aspect_dimensions, ScalingAlgorithm};
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
        // 1. High-Speed Buffered File Read (Zero kernel page-table lock contention across parallel threads)
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(128 * 1024, file);

        // 2. Image Decoding
        let dynamic_img = ImageReader::new(reader)
            .with_guessed_format()?
            .decode()?;

        let orig_w = dynamic_img.width();
        let orig_h = dynamic_img.height();

        // 3. Compute exact aspect-ratio target dimensions inside max_bound
        let (target_w, target_h) = calculate_aspect_dimensions(orig_w, orig_h, max_bound);

        let rgb_src = dynamic_img.into_rgb8();
        let src_raw = rgb_src.into_raw();

        // 4. Pure Direct Native SIMD Resizing via fast_image_resize
        let src_image = Image::from_vec_u8(orig_w, orig_h, src_raw, PixelType::U8x3)?;
        let mut dst_image = Image::new(target_w, target_h, PixelType::U8x3);
        let mut resizer = Resizer::new();
        let options = ResizeOptions::default().resize_alg(scaling_algo.to_resize_alg());

        resizer.resize(&src_image, &mut dst_image, &options)?;
        let resized_raw = dst_image.into_vec();

        Ok(DecodedThumbnailFrame {
            width: target_w,
            height: target_h,
            rgb_pixels: resized_raw,
        })
    }
}
