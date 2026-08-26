use super::error::ViewerError;
use crate::settings::LogObscurer;
use crate::thumbnails::video::VideoFrameDecoder;
use image::ImageReader;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;

pub struct HighResImageLoader;

impl HighResImageLoader {
    /// Loads and decodes a full-resolution image from disk into a thread-safe `SharedPixelBuffer`.
    /// First attempts native decoding via the `image` crate with a 256 KiB read buffer;
    /// falls back to the resilient FFmpeg decoder for specialized formats (e.g. PSD, exotic TIFF).
    pub fn load_full_resolution(
        path: &Path,
    ) -> Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), ViewerError> {
        let t0 = Instant::now();

        if !path.exists() {
            tracing::warn!(
                path = %LogObscurer::path(path),
                "[HighResImageLoader] Target image file does not exist on disk"
            );
            return Err(ViewerError::FileNotFound(path.to_path_buf()));
        }

        // 1. High-Speed Buffered File Read & Native Image Crate Decoding
        if let Ok(file) = File::open(path) {
            let reader = BufReader::with_capacity(256 * 1024, file);
            if let Ok(reader_guessed) = ImageReader::new(reader).with_guessed_format()
                && let Ok(dynamic_img) = reader_guessed.decode()
            {
                let width = dynamic_img.width();
                let height = dynamic_img.height();
                let rgb = dynamic_img.into_rgb8();
                let raw = rgb.into_raw();

                let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
                pixel_buffer.make_mut_bytes().copy_from_slice(&raw);

                tracing::info!(
                    path = %LogObscurer::path(path),
                    width = width,
                    height = height,
                    elapsed_ms = t0.elapsed().as_millis(),
                    "[HighResImageLoader] Decoded full-resolution image via image crate"
                );

                return Ok((width, height, pixel_buffer));
            }
        }

        // 2. Resilient Fallback to FFmpeg decoder (for PSD, exotic TIFF or chunk variations)
        let ffmpeg_decoder = crate::thumbnails::video::FfmpegVideoThumbnailer::new();
        match ffmpeg_decoder.extract_frame(
            path,
            0.0,
            8192,
            crate::thumbnails::ScalingAlgorithm::Nearest,
        ) {
            Ok(frame) => {
                let width = frame.width;
                let height = frame.height;
                let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
                pixel_buffer.make_mut_bytes().copy_from_slice(&frame.rgb_pixels);

                tracing::info!(
                    path = %LogObscurer::path(path),
                    width = width,
                    height = height,
                    elapsed_ms = t0.elapsed().as_millis(),
                    "[HighResImageLoader] Decoded full-resolution image via FFmpeg fallback"
                );

                Ok((width, height, pixel_buffer))
            }
            Err(e) => {
                tracing::error!(
                    path = %LogObscurer::path(path),
                    error = %e,
                    "[HighResImageLoader] Failed to decode full-resolution image across all decoders"
                );
                Err(ViewerError::DecodeFailed(e.to_string()))
            }
        }
    }
}
