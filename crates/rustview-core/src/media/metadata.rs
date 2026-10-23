use super::detector::MediaDetector;
use super::formats::{MediaFormat, MediaType};
use std::fs;
use std::path::Path;
use std::sync::Once;
use thiserror::Error;

static FFMPEG_INIT: Once = Once::new();

/// Ensures `FFmpeg` global subsystems are initialized exactly once across the process lifecycle.
pub fn ensure_ffmpeg_initialized() {
    FFMPEG_INIT.call_once(|| {
        let _ = ffmpeg_next::init();
    });
}

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("I/O error during metadata extraction: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse image dimensions: {0}")]
    ImageError(String),
    #[error("Failed to parse video dimensions: {0}")]
    VideoError(String),
    #[error("Unsupported or unknown format")]
    UnsupportedFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMetadata {
    pub file_size: u64,
    pub modification_time: i64,
    pub creation_time: i64,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    /// What the file really is: the content decides when it disagrees with the extension (an AVIF photo called `.mp4`).
    pub media_type: MediaType,
    /// The format of the extension; it is what the format switches of the Settings act on.
    pub format: MediaFormat,
}

pub struct MetadataExtractor;

impl MetadataExtractor {
    /// Extracts metadata by inspecting container headers and filesystem attributes without decoding pixel data.
    pub fn extract(path: &Path) -> Result<RawMetadata, MetadataError> {
        let format = MediaFormat::from_path(path);
        let extension_type = format.media_type().ok_or(MetadataError::UnsupportedFormat)?;
        let media_type = Self::resolve_media_type(path, extension_type);

        let fs_meta = fs::metadata(path)?;
        let file_size = fs_meta.len();

        let modification_time = fs_meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);

        let creation_time = fs_meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(modification_time, |d| d.as_secs() as i64);

        let (width, height, duration_ms) = match media_type {
            MediaType::Image => {
                let (w, h) = Self::extract_image_dimensions(path)?;
                (w, h, 0u64)
            }
            MediaType::Video => Self::extract_video_details(path)?,
        };

        Ok(RawMetadata {
            file_size,
            modification_time,
            creation_time,
            duration_ms,
            width,
            height,
            media_type,
            format,
        })
    }

    /// Image or video by content where the header is conclusive, else by extension.
    pub fn resolve_media_type(path: &Path, extension_type: MediaType) -> MediaType {
        MediaDetector::read_header(path)
            .and_then(|header| MediaDetector::content_media_type(&header))
            .unwrap_or(extension_type)
    }

    fn extract_image_dimensions(path: &Path) -> Result<(u32, u32), MetadataError> {
        if MediaFormat::from_path(path) == MediaFormat::Svg {
            return super::svg::intrinsic_size(path).map_err(|e| MetadataError::ImageError(e.to_string()));
        }
        if let Ok(reader) = super::image_decode::open_reader(path, 64 * 1024)
            && let Ok(dims) = reader.into_dimensions()
        {
            return Ok(dims);
        }
        // Fallback for formats not natively handled by the image crate (AVIF, JPEG XL, PSD, ...): a picture stored as a
        // grid of tiles (HEIC photos) is as large as the grid, not as its first tile
        if let Ok(Some(size)) = super::tile_grid::display_size(path) {
            return Ok(size);
        }
        let (w, h, _) = Self::extract_video_details(path).map_err(|e| MetadataError::ImageError(e.to_string()))?;
        Ok((w, h))
    }

    /// Width, height (after rotation) and duration of the first video stream, read through `FFmpeg`.
    pub fn probe_video_details(path: &Path) -> Result<(u32, u32, u64), MetadataError> {
        Self::extract_video_details(path)
    }

    fn extract_video_details(path: &Path) -> Result<(u32, u32, u64), MetadataError> {
        ensure_ffmpeg_initialized();

        let ictx = ffmpeg_next::format::input(path).map_err(|e| MetadataError::VideoError(e.to_string()))?;

        let stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| MetadataError::VideoError("No video stream found in file".to_string()))?;

        let context_decoder = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| MetadataError::VideoError(e.to_string()))?;
        let video_decoder = context_decoder
            .decoder()
            .video()
            .map_err(|e| MetadataError::VideoError(e.to_string()))?;

        let mut width = video_decoder.width();
        let mut height = video_decoder.height();

        if width == 0 || height == 0 {
            return Err(MetadataError::VideoError("Invalid video dimensions (0x0)".to_string()));
        }

        // A phone video stores its rotation next to the pixels: the picture is shown with width and height swapped
        if super::orientation::swaps_axes(super::orientation::stream_rotation(&stream, &ictx.metadata())) {
            std::mem::swap(&mut width, &mut height);
        }

        let duration_ms = if ictx.duration() > 0 {
            ((ictx.duration() as f64 / f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) * 1000.0).round() as u64
        } else if stream.duration() > 0 {
            ((stream.duration() as f64 * f64::from(stream.time_base())) * 1000.0).round() as u64
        } else {
            0
        };

        Ok((width, height, duration_ms))
    }
}
