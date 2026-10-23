use super::error::ThumbnailError;
use super::types::{ScalingAlgorithm, calculate_aspect_dimensions};
use super::video::{FfmpegVideoThumbnailer, VideoFrameDecoder};
use crate::media::{MediaFormat, image_decode, svg, tile_grid};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DecodedThumbnailFrame {
    pub width: u32,
    pub height: u32,
    pub rgb_pixels: Vec<u8>,
    pub orig_width: u32,
    pub orig_height: u32,
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
        if MediaFormat::from_path(path) == MediaFormat::Svg {
            let raster = svg::rasterize(path, max_bound).map_err(|e| ThumbnailError::Svg(e.to_string()))?;
            return Ok(DecodedThumbnailFrame {
                width: raster.width,
                height: raster.height,
                rgb_pixels: raster.rgb,
                orig_width: raster.intrinsic_width,
                orig_height: raster.intrinsic_height,
            });
        }

        // 1. High-Speed Buffered File Read & Native Image Crate Decoding
        if let Ok(reader_guessed) = image_decode::open_reader(path, 128 * 1024)
            && let Ok(dynamic_img) = reader_guessed.decode()
        {
            let (orig_w, orig_h) = (dynamic_img.width(), dynamic_img.height());
            return Self::resize_rgb(
                dynamic_img.into_rgb8().into_raw(),
                orig_w,
                orig_h,
                max_bound,
                scaling_algo,
            );
        }

        // 2. FFmpeg: a picture stored as a grid of tiles (HEIC photos, AVIF grids) is put together first
        match tile_grid::decode(path) {
            Ok(Some(grid)) => return Self::resize_rgb(grid.rgb, grid.width, grid.height, max_bound, scaling_algo),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(error = %e, "[ImageThumbnailer] Tile grid could not be composed; falling back to the first stream");
            }
        }

        // 3. Resilient Fallback to FFmpeg decoder (AVIF, JPEG XL, JPEG 2000, PSD, DPX, legacy formats, exotic variants)
        let ffmpeg_decoder = FfmpegVideoThumbnailer::new();
        ffmpeg_decoder.extract_frame(path, 0.0, max_bound, scaling_algo)
    }
}

impl SimdImageThumbnailer {
    fn resize_rgb(
        rgb: Vec<u8>,
        orig_w: u32,
        orig_h: u32,
        max_bound: u32,
        scaling_algo: ScalingAlgorithm,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        let (target_w, target_h) = calculate_aspect_dimensions(orig_w, orig_h, max_bound);
        let src_image = Image::from_vec_u8(orig_w, orig_h, rgb, PixelType::U8x3)?;
        let mut dst_image = Image::new(target_w, target_h, PixelType::U8x3);
        let mut resizer = Resizer::new();
        let options = ResizeOptions::default().resize_alg(scaling_algo.to_resize_alg());

        resizer.resize(&src_image, &mut dst_image, &options)?;

        Ok(DecodedThumbnailFrame {
            width: target_w,
            height: target_h,
            rgb_pixels: dst_image.into_vec(),
            orig_width: orig_w,
            orig_height: orig_h,
        })
    }
}
