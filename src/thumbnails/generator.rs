use super::error::ThumbnailError;
use super::image::{ImageDecoder, SimdImageThumbnailer};
use super::types::{FitMode, THUMBNAIL_SIZE};
use super::video::{FfmpegVideoThumbnailer, VideoFrameDecoder};
use crate::media::{MediaFormat, MediaType};
use image::ColorType;
use image::codecs::jpeg::JpegEncoder;
use std::path::Path;
use std::sync::Arc;

pub struct ThumbnailGenerator {
    image_decoder: Arc<dyn ImageDecoder>,
    video_decoder: Arc<dyn VideoFrameDecoder>,
}

impl ThumbnailGenerator {
    pub fn new() -> Self {
        Self {
            image_decoder: Arc::new(SimdImageThumbnailer::new()),
            video_decoder: Arc::new(FfmpegVideoThumbnailer::new()),
        }
    }

    /// Decodes, SIMD resizes to 256x256 RGB8, and returns raw RGB8 bytes.
    pub fn generate_rgb8(&self, path: &Path, fit_mode: FitMode) -> Result<Vec<u8>, ThumbnailError> {
        let format = MediaFormat::from_path(path);
        match format.media_type() {
            Some(MediaType::Image) => self.image_decoder.decode_and_resize(path, fit_mode),
            Some(MediaType::Video) => self.video_decoder.extract_and_resize(path, fit_mode),
            None => Err(ThumbnailError::Ffmpeg("Unsupported media type".into())),
        }
    }

    /// Encodes raw 256x256 RGB8 into compressed JPEG bytes with high quality (85%).
    pub fn compress_jpeg(&self, rgb8_pixels: &[u8]) -> Result<Vec<u8>, ThumbnailError> {
        let mut buffer = Vec::with_capacity(32 * 1024);
        let mut encoder = JpegEncoder::new_with_quality(&mut buffer, 85);
        encoder.encode(
            rgb8_pixels,
            THUMBNAIL_SIZE,
            THUMBNAIL_SIZE,
            ColorType::Rgb8.into(),
        )?;
        Ok(buffer)
    }

    /// Decompresses packed JPEG bytes into raw 256x256 RGB8.
    pub fn decompress_jpeg(&self, compressed: &[u8]) -> Result<Vec<u8>, ThumbnailError> {
        let img = image::load_from_memory_with_format(compressed, image::ImageFormat::Jpeg)?;
        Ok(img.to_rgb8().into_raw())
    }
}

impl Default for ThumbnailGenerator {
    fn default() -> Self {
        Self::new()
    }
}
