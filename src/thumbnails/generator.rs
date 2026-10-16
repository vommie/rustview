use super::error::ThumbnailError;
use super::image::{DecodedThumbnailFrame, ImageDecoder, SimdImageThumbnailer};
use super::types::{ScalingAlgorithm, ThumbnailConfig, ThumbnailFormat};
use super::video::{DynamicVideoThumbnailer, VideoExtractionResult};
use image::codecs::jpeg::JpegEncoder;
use image::ColorType;
use std::path::Path;
use std::sync::Arc;

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
    /// Uses native lossy WebP compression via libwebp or high-performance JPEG compression via JpegEncoder.
    pub fn compress(
        &self,
        rgb8_pixels: &[u8],
        width: u32,
        height: u32,
        format: ThumbnailFormat,
        quality: u8,
    ) -> Result<Vec<u8>, ThumbnailError> {
        let quality = quality.clamp(10, 100);

        if rgb8_pixels.len() != (width as usize) * (height as usize) * 3 {
            return Err(ThumbnailError::ImageBuffer(
                fast_image_resize::ImageBufferError::InvalidBufferSize,
            ));
        }

        let buffer = match format {
            ThumbnailFormat::Webp => {
                let encoder = webp::Encoder::from_rgb(rgb8_pixels, width, height);
                let webp_memory = encoder.encode(quality as f32);
                webp_memory.to_vec()
            }
            ThumbnailFormat::Jpeg => {
                let mut buf = Vec::with_capacity((width * height) as usize / 2);
                let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality);
                encoder.encode(rgb8_pixels, width, height, ColorType::Rgb8.into())?;
                buf
            }
        };

        Ok(buffer)
    }

    /// Decompresses packed thumbnail slice to (width, height, raw RGB8 bytes), automatically detecting WebP/JPEG.
    pub fn decompress(&self, compressed: &[u8]) -> Result<(u32, u32, Vec<u8>), ThumbnailError> {
        let detected = ThumbnailFormat::detect(compressed);

        let img = match detected {
            Some(ThumbnailFormat::Jpeg) => {
                image::load_from_memory_with_format(compressed, image::ImageFormat::Jpeg)?
            }
            Some(ThumbnailFormat::Webp) => match decode_webp_rgb(compressed) {
                Some(decoded) => return Ok(decoded),
                None => image::load_from_memory_with_format(compressed, image::ImageFormat::WebP)?,
            },
            None => image::load_from_memory(compressed)?,
        };

        let width = img.width();
        let height = img.height();
        let raw = img.into_rgb8().into_raw();

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

/// libwebp decodes a lossy RGB thumbnail several times faster than the pure Rust decoder of the `image` crate and writes RGB8 directly.
/// Anything else (alpha, animation, damaged data) returns `None` and is left to the generic decoder.
fn decode_webp_rgb(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoded = webp::Decoder::new(bytes).decode()?;
    if decoded.is_alpha() {
        return None;
    }
    Some((decoded.width(), decoded.height(), decoded.to_vec()))
}

impl Default for ThumbnailGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&[(x * 255 / width) as u8, (y * 255 / height) as u8, ((x + y) % 256) as u8]);
            }
        }
        rgb
    }

    #[test]
    fn a_lossy_webp_round_trips_through_libwebp() {
        let generator = ThumbnailGenerator::new();
        let rgb = gradient(64, 48);
        let packed = generator.compress(&rgb, 64, 48, ThumbnailFormat::Webp, 90).expect("compress");
        let (width, height, decoded) = generator.decompress(&packed).expect("decompress");
        assert_eq!((width, height), (64, 48));
        assert_eq!(decoded.len(), rgb.len());
        let max_error = rgb.iter().zip(&decoded).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        assert!(max_error < 40, "lossy error too large: {max_error}");
    }

    #[test]
    fn a_jpeg_still_decodes_through_the_generic_path() {
        let generator = ThumbnailGenerator::new();
        let rgb = gradient(32, 32);
        let packed = generator.compress(&rgb, 32, 32, ThumbnailFormat::Jpeg, 90).expect("compress");
        let (width, height, decoded) = generator.decompress(&packed).expect("decompress");
        assert_eq!((width, height, decoded.len()), (32, 32, rgb.len()));
    }

    #[test]
    fn damaged_data_is_an_error_not_a_panic() {
        let generator = ThumbnailGenerator::new();
        let rgb = gradient(16, 16);
        let mut packed = generator.compress(&rgb, 16, 16, ThumbnailFormat::Webp, 80).expect("compress");
        packed.truncate(packed.len() / 2);
        let _ = generator.decompress(&packed);
        assert!(generator.decompress(&[1, 2, 3, 4]).is_err());
    }
}
