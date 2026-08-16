use super::error::ThumbnailError;
use super::image::{DecodedThumbnailFrame, ImageDecoder, SimdImageThumbnailer};
use super::types::{ScalingAlgorithm, ThumbnailConfig, ThumbnailFormat};
use super::video::{DynamicVideoThumbnailer, VideoExtractionResult};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ColorType, ExtendedColorType};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub struct ThumbnailGenerator {
    image_decoder: Arc<dyn ImageDecoder>,
    video_decoder: Arc<DynamicVideoThumbnailer>,
}

impl ThumbnailGenerator {
    pub fn new() -> Self {
        Self {
            image_decoder: Arc::new(SimdImageThumbnailer::new()),
            video_decoder: Arc::new(DynamicVideoThumbnailer::new()),
        }
    }

    /// Decodes image frame and SIMD resizes to aspect-ratio target within max_bound.
    pub fn generate_image_thumbnail(
        &self,
        path: &Path,
        max_bound: u32,
        scaling_algo: ScalingAlgorithm,
    ) -> Result<DecodedThumbnailFrame, ThumbnailError> {
        self.image_decoder
            .decode_and_resize(path, max_bound, scaling_algo)
    }

    /// Extracts video primary thumbnail and optional multi-frame previews in a single pass.
    pub fn generate_video_thumbnails(
        &self,
        path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<VideoExtractionResult, ThumbnailError> {
        self.video_decoder
            .decoder_for(config.video_engine)
            .extract_video(path, config)
    }

    /// Compresses raw RGB8 pixels to configured format (WebP or JPEG) with specified quality factor (10..100).
    pub fn compress(
        &self,
        rgb8_pixels: &[u8],
        width: u32,
        height: u32,
        format: ThumbnailFormat,
        quality: u8,
    ) -> Result<Vec<u8>, ThumbnailError> {
        let t0 = Instant::now();
        let quality = quality.clamp(10, 100);
        let mut buffer = Vec::with_capacity((width * height) as usize / 2);

        match format {
            ThumbnailFormat::Webp => {
                let encoder = WebPEncoder::new_lossless(&mut buffer);
                encoder.encode(rgb8_pixels, width, height, ExtendedColorType::Rgb8)?;
            }
            ThumbnailFormat::Jpeg => {
                let mut encoder = JpegEncoder::new_with_quality(&mut buffer, quality);
                encoder.encode(rgb8_pixels, width, height, ColorType::Rgb8.into())?;
            }
        }

        tracing::trace!(
            format = %format.as_str(),
            width = width,
            height = height,
            quality = quality,
            output_bytes = buffer.len(),
            encode_time_us = t0.elapsed().as_micros(),
            "[Thumbnail Compression] Encoded RGB8 thumbnail into binary payload"
        );

        Ok(buffer)
    }

    /// Decompresses packed thumbnail slice to (width, height, raw RGB8 bytes), automatically detecting WebP/JPEG.
    pub fn decompress(&self, compressed: &[u8]) -> Result<(u32, u32, Vec<u8>), ThumbnailError> {
        let t0 = Instant::now();
        let detected = ThumbnailFormat::detect(compressed);

        let img = match detected {
            Some(ThumbnailFormat::Jpeg) => {
                image::load_from_memory_with_format(compressed, image::ImageFormat::Jpeg)?
            }
            Some(ThumbnailFormat::Webp) => {
                image::load_from_memory_with_format(compressed, image::ImageFormat::WebP)?
            }
            None => image::load_from_memory(compressed)?,
        };

        let width = img.width();
        let height = img.height();
        let raw = img.to_rgb8().into_raw();

        tracing::trace!(
            detected_format = ?detected,
            width = width,
            height = height,
            decompressed_bytes = raw.len(),
            decode_time_us = t0.elapsed().as_micros(),
            "[Thumbnail Decompression] Decompressed packfile thumbnail slice"
        );

        Ok((width, height, raw))
    }

    // Backward-compatibility wrappers
    pub fn compress_jpeg(&self, rgb8_pixels: &[u8]) -> Result<Vec<u8>, ThumbnailError> {
        self.compress(rgb8_pixels, 256, 256, ThumbnailFormat::Jpeg, 85)
    }

    pub fn decompress_jpeg(&self, compressed: &[u8]) -> Result<Vec<u8>, ThumbnailError> {
        let (_, _, raw) = self.decompress(compressed)?;
        Ok(raw)
    }
}

impl Default for ThumbnailGenerator {
    fn default() -> Self {
        Self::new()
    }
}
