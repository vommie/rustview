use super::formats::{MediaFormat, MediaType};
use std::fs;
use std::path::Path;
use thiserror::Error;

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
    pub width: u32,
    pub height: u32,
    pub media_type: MediaType,
    pub format: MediaFormat,
}

pub struct MetadataExtractor;

impl MetadataExtractor {
    /// Extracts metadata by inspecting container headers without fully decoding pixel data.
    pub fn extract(path: &Path) -> Result<RawMetadata, MetadataError> {
        let format = MediaFormat::from_path(path);
        let media_type = format
            .media_type()
            .ok_or(MetadataError::UnsupportedFormat)?;

        let fs_meta = fs::metadata(path)?;
        let file_size = fs_meta.len();
        let modification_time = fs_meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let (width, height) = match media_type {
            MediaType::Image => Self::extract_image_dimensions(path)?,
            MediaType::Video => Self::extract_video_dimensions(path)?,
        };

        Ok(RawMetadata {
            file_size,
            modification_time,
            width,
            height,
            media_type,
            format,
        })
    }

    fn extract_image_dimensions(path: &Path) -> Result<(u32, u32), MetadataError> {
        image::image_dimensions(path).map_err(|e| MetadataError::ImageError(e.to_string()))
    }

    fn extract_video_dimensions(path: &Path) -> Result<(u32, u32), MetadataError> {
        ffmpeg_next::init().map_err(|e| MetadataError::VideoError(e.to_string()))?;

        let ictx = ffmpeg_next::format::input(path)
            .map_err(|e| MetadataError::VideoError(e.to_string()))?;

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
            return Err(MetadataError::VideoError(
                "Invalid video dimensions (0x0)".to_string(),
            ));
        }

        // Apply display orientation rotation if container/stream specifies it safely
        let rotation: i32 = stream
            .metadata()
            .get("rotate")
            .and_then(|r| r.parse::<i32>().ok())
            .or_else(|| {
                ictx.metadata()
                    .get("rotate")
                    .and_then(|r| r.parse::<i32>().ok())
            })
            .unwrap_or(0);
        let norm_rot = rotation.rem_euclid(360);
        if norm_rot == 90 || norm_rot == 270 {
            std::mem::swap(&mut width, &mut height);
        }

        Ok((width, height))
    }
}
