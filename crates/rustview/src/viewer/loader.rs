use super::error::ViewerError;
use rustview_core::media::{MediaFormat, image_decode, svg, tile_grid};
use rustview_core::settings::LogObscurer;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::Path;
use std::time::Instant;

pub struct HighResImageLoader;

impl HighResImageLoader {
    /// Loads and decodes a full-resolution image from disk into a thread-safe `SharedPixelBuffer`.
    /// First attempts native decoding via the `image` crate with a 256 KiB read buffer;
    /// falls back to the resilient FFmpeg decoder for specialized formats (e.g. PSD, exotic TIFF).
    pub fn load_full_resolution(path: &Path) -> Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), ViewerError> {
        let t0 = Instant::now();

        if !path.exists() {
            tracing::warn!(
                path = %LogObscurer::path(path),
                "[HighResImageLoader] Target image file does not exist on disk"
            );
            return Err(ViewerError::FileNotFound(path.to_path_buf()));
        }

        if MediaFormat::from_path(path) == MediaFormat::Svg {
            return Self::load_svg(path, t0);
        }

        // 1. High-Speed Buffered File Read & Native Image Crate Decoding
        if let Ok(reader_guessed) = image_decode::open_reader(path, 256 * 1024)
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

        // 2. FFmpeg: a picture stored as a grid of tiles (HEIC photos, AVIF grids) is put together first
        match tile_grid::decode(path) {
            Ok(Some(grid)) if !grid.rgb.is_empty() => {
                let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(grid.width, grid.height);
                pixel_buffer.make_mut_bytes().copy_from_slice(&grid.rgb);
                tracing::info!(
                    path = %LogObscurer::path(path),
                    width = grid.width,
                    height = grid.height,
                    elapsed_ms = t0.elapsed().as_millis(),
                    "[HighResImageLoader] Composed a tiled image via FFmpeg"
                );
                return Ok((grid.width, grid.height, pixel_buffer));
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "[HighResImageLoader] Tile grid could not be composed; falling back to the first stream")
            }
        }

        // 3. Resilient Fallback to FFmpeg decoder (AVIF, JPEG XL, JPEG 2000, PSD, DPX, legacy formats, exotic variants),
        // at the size the picture has
        let ffmpeg_decoder = rustview_core::thumbnails::video::FfmpegVideoThumbnailer::new();
        match ffmpeg_decoder.extract_native_frame(path) {
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

    fn load_svg(path: &Path, t0: Instant) -> Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), ViewerError> {
        let (iw, ih) = svg::intrinsic_size(path).map_err(|e| ViewerError::DecodeFailed(e.to_string()))?;
        let raster =
            svg::rasterize(path, svg::viewer_edge(iw, ih)).map_err(|e| ViewerError::DecodeFailed(e.to_string()))?;
        let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(raster.width, raster.height);
        pixel_buffer.make_mut_bytes().copy_from_slice(&raster.rgb);

        tracing::info!(
            path = %LogObscurer::path(path),
            width = raster.width,
            height = raster.height,
            elapsed_ms = t0.elapsed().as_millis(),
            "[HighResImageLoader] Rasterized SVG via resvg"
        );
        Ok((raster.width, raster.height, pixel_buffer))
    }
}
